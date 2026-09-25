use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use uuid::Uuid;

use crate::config::env_value;

#[derive(Clone)]
struct Config {
    bind: IpAddr,
    port: u16,
    advertise: Ipv4Addr,
    passive_min: u16,
    passive_max: u16,
    root: PathBuf,
    access_code: String,
    acceptor: TlsAcceptor,
    max_upload: u64,
    max_total: u64,
}

impl Config {
    fn from_env() -> Result<Self> {
        let access_code = env_value("FTPS_ACCESS_CODE", "");
        if access_code.len() != 8 || !access_code.bytes().all(|v| v.is_ascii_alphanumeric()) {
            bail!("FTPS_ACCESS_CODE must be exactly eight letters or digits")
        }
        let cert: PathBuf = env_value("FTPS_CERT_FILE", "/secrets/certs/server.crt").into();
        let key: PathBuf = env_value("FTPS_KEY_FILE", "/secrets/certs/server.key").into();
        let passive_min = env_value("FTPS_PASSIVE_MIN", "50000").parse()?;
        let passive_max = env_value("FTPS_PASSIVE_MAX", "50009").parse()?;
        if passive_min > passive_max {
            bail!("FTPS_PASSIVE_MIN must not exceed FTPS_PASSIVE_MAX")
        }
        let max_upload = env_value("FTPS_MAX_UPLOAD_MB", "1024").parse::<u64>()? * 1024 * 1024;
        let max_total = env_value("FTPS_MAX_TOTAL_MB", "10240").parse::<u64>()? * 1024 * 1024;
        if max_total < max_upload {
            bail!("FTPS_MAX_TOTAL_MB must be at least FTPS_MAX_UPLOAD_MB")
        }
        Ok(Self {
            bind: env_value("FTPS_BIND_HOST", "0.0.0.0").parse()?,
            port: env_value("FTPS_PORT", "9990").parse()?,
            advertise: env_value("FTPS_ADVERTISE_IP", "127.0.0.2").parse()?,
            passive_min,
            passive_max,
            root: env_value("FTPS_ROOT", "/staging").into(),
            access_code,
            acceptor: crate::tls::acceptor(&cert, &key)?,
            max_upload,
            max_total,
        })
    }
}

struct Session {
    config: Arc<Config>,
    stream: BufReader<TlsStream<TcpStream>>,
    authenticated: bool,
    username: String,
    cwd: PathBuf,
    passive: Option<TcpListener>,
    restart: u64,
    rename_from: Option<PathBuf>,
    private_data: bool,
}

impl Session {
    async fn reply(&mut self, code: u16, message: &str) -> Result<()> {
        self.stream
            .get_mut()
            .write_all(format!("{code} {message}\r\n").as_bytes())
            .await?;
        self.stream.get_mut().flush().await?;
        Ok(())
    }
    async fn auth(&mut self) -> Result<bool> {
        if self.authenticated {
            Ok(true)
        } else {
            self.reply(530, "Not logged in").await?;
            Ok(false)
        }
    }
    fn virtual_path(&self, raw: &str) -> Result<PathBuf> {
        let mut value = if raw.trim().starts_with('/') {
            PathBuf::new()
        } else {
            self.cwd.clone()
        };
        for part in Path::new(raw.trim()).components() {
            match part {
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    value.pop();
                }
                Component::Normal(v) => {
                    let s = v.to_string_lossy();
                    if s.contains('\0') || s.starts_with(".upload-") || s.starts_with(".proxy-") {
                        bail!("invalid path")
                    }
                    value.push(v)
                }
                Component::Prefix(_) => bail!("invalid path"),
            }
        }
        Ok(value)
    }
    fn real(&self, raw: &str) -> Result<PathBuf> {
        let virtual_path = self.virtual_path(raw)?;
        let candidate = self.config.root.join(virtual_path);
        let root = fs::canonicalize(&self.config.root)?;
        let resolved = if candidate.exists() {
            fs::canonicalize(&candidate)?
        } else {
            fs::canonicalize(candidate.parent().context("path has no parent")?)?
        };
        if !resolved.starts_with(&root) {
            bail!("path escapes staging storage")
        }
        Ok(candidate)
    }
    fn used(root: &Path) -> u64 {
        fn walk(p: &Path) -> u64 {
            let Ok(entries) = fs::read_dir(p) else {
                return 0;
            };
            entries
                .flatten()
                .map(|e| {
                    let p = e.path();
                    if p.is_dir() {
                        walk(&p)
                    } else if p.is_file() && !e.file_name().to_string_lossy().starts_with('.') {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    } else {
                        0
                    }
                })
                .sum()
        }
        walk(root)
    }
    async fn open_passive(&mut self) -> Result<Option<u16>> {
        self.passive = None;
        for port in self.config.passive_min..=self.config.passive_max {
            if let Ok(listener) = TcpListener::bind((self.config.bind, port)).await {
                self.passive = Some(listener);
                return Ok(Some(port));
            }
        }
        Ok(None)
    }
    async fn data(&mut self) -> Result<TlsStream<TcpStream>> {
        let listener = self.passive.take().context("use PASV or EPSV first")?;
        let (socket, _) =
            tokio::time::timeout(Duration::from_secs(30), listener.accept()).await??;
        Ok(self.config.acceptor.accept(socket).await?)
    }
    async fn transfer_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.reply(150, "Opening protected data connection").await?;
        let mut data = self.data().await?;
        data.write_all(bytes).await?;
        data.shutdown().await?;
        self.reply(226, "Transfer complete").await
    }
    fn list_bytes(&self, raw: &str, machine: bool, names: bool) -> Result<Vec<u8>> {
        let target = self.real(
            raw.split_whitespace()
                .rfind(|v| !v.starts_with('-'))
                .unwrap_or(""),
        )?;
        let mut paths = if target.is_dir() {
            fs::read_dir(target)?.flatten().map(|v| v.path()).collect()
        } else if target.exists() {
            vec![target]
        } else {
            bail!("path not found")
        };
        paths.sort();
        let mut out = String::new();
        for path in paths {
            if path.is_symlink() {
                continue;
            }
            let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if name.starts_with(".upload-") || name.starts_with(".proxy-") {
                continue;
            }
            let metadata = fs::metadata(&path)?;
            let size = if metadata.is_file() {
                metadata.len()
            } else {
                0
            };
            let line = if names {
                name.to_owned()
            } else if machine {
                format!(
                    "type={};size={size};modify=19700101000000; {name}",
                    if metadata.is_dir() { "dir" } else { "file" }
                )
            } else {
                format!(
                    "{} 1 bblp bblp {size} Jan 01 00:00 {name}",
                    if metadata.is_dir() {
                        "drwxr-xr-x"
                    } else {
                        "-rw-r--r--"
                    }
                )
            };
            out.push_str(&line);
            out.push_str("\r\n");
        }
        Ok(out.into_bytes())
    }
    async fn command(&mut self, command: &str, arg: &str) -> Result<bool> {
        match command {
            "USER"=>{self.username=arg.into();self.reply(331,"Password required").await?},
            "PASS"=>{self.authenticated=self.username=="bblp"&&arg.as_bytes().ct_eq(self.config.access_code.as_bytes()).unwrap_u8()==1;self.reply(if self.authenticated{230}else{530},if self.authenticated{"Login successful"}else{"Login incorrect"}).await?},
            "QUIT"=>{self.reply(221,"Goodbye").await?;return Ok(false)},
            "FEAT"=>{self.stream.get_mut().write_all(b"211-Features\r\n AUTH TLS\r\n PBSZ\r\n PROT\r\n EPSV\r\n MLST type*;size*;modify*;\r\n UTF8\r\n211 End\r\n").await?},
            "SYST"=>self.reply(215,"UNIX Type: L8").await?,"OPTS"|"CLNT"|"NOOP"|"MODE"|"STRU"|"TYPE"|"ALLO"=>self.reply(200,"OK").await?,
            "PBSZ"=>self.reply(200,"PBSZ=0").await?,"PROT"=>{self.private_data=arg.eq_ignore_ascii_case("P");self.reply(if self.private_data{200}else{536},"Protection level set").await?},
            _ if !self.auth().await?=>{},
            "PWD"=>self.reply(257,&format!("\"/{}\" is current directory",self.cwd.display())).await?,
            "CWD"=>{let v=self.virtual_path(arg)?;if self.real(arg)?.is_dir(){self.cwd=v;self.reply(250,"Directory changed").await?}else{self.reply(550,"Directory not found").await?}},
            "CDUP"=>{self.cwd.pop();self.reply(250,"Directory changed").await?},
            "PASV"=>{let Some(port)=self.open_passive().await?else{self.reply(425,"No passive data port available").await?;return Ok(true)};let o=self.config.advertise.octets();self.reply(227,&format!("Entering Passive Mode ({},{},{},{},{},{})",o[0],o[1],o[2],o[3],port/256,port%256)).await?},
            "EPSV"=>{if arg.eq_ignore_ascii_case("ALL"){self.reply(200,"EPSV ALL accepted").await?}else{let Some(port)=self.open_passive().await?else{self.reply(425,"No passive data port available").await?;return Ok(true)};self.reply(229,&format!("Entering Extended Passive Mode (|||{port}|)")).await?}},
            "PORT"|"EPRT"=>self.reply(502,"Active mode is not supported").await?,
            "LIST"|"NLST"|"MLSD"=>{let bytes=self.list_bytes(arg,command=="MLSD",command=="NLST")?;self.transfer_bytes(&bytes).await?},
            "SIZE"=>{let p=self.real(arg)?;if p.is_file(){self.reply(213,&fs::metadata(p)?.len().to_string()).await?}else{self.reply(550,"File not found").await?}},
            "MDTM"=>{let p=self.real(arg)?;if p.exists(){self.reply(213,"19700101000000").await?}else{self.reply(550,"Path not found").await?}},
            "REST"=>{self.restart=arg.parse()?;self.reply(350,&format!("Restarting at {}",self.restart)).await?},
            "RETR"=>{let p=self.real(arg)?;if !p.is_file(){self.reply(550,"File not found").await?}else{let mut bytes=tokio::fs::read(p).await?;bytes.drain(..usize::try_from(self.restart.min(bytes.len() as u64)).unwrap());self.restart=0;self.transfer_bytes(&bytes).await?}},
            "STOR"=>{
                let destination=self.real(arg)?;
                if !destination.parent().is_some_and(Path::is_dir){
                    self.reply(550,"Destination directory does not exist").await?
                }else{
                    self.reply(150,"Opening protected data connection").await?;
                    let mut data=self.data().await?;
                    let partial=destination.parent().unwrap().join(format!(".upload-{}.part",Uuid::new_v4()));
                    let mut file=tokio::fs::File::create(&partial).await?;
                    let mut total=0u64;let mut buffer=[0u8;65536];
                    loop{let n=data.read(&mut buffer).await?;if n==0{break}total+=n as u64;if total>self.config.max_upload{let _=tokio::fs::remove_file(&partial).await;bail!("single-file upload limit exceeded")}file.write_all(&buffer[..n]).await?}
                    file.sync_all().await?;drop(file);
                    let old=fs::metadata(&destination).map(|v|v.len()).unwrap_or(0);
                    if Self::used(&self.config.root)-old+total>self.config.max_total{let _=fs::remove_file(&partial);bail!("staging storage quota exceeded")}
                    if destination.extension().is_some_and(|v|v.eq_ignore_ascii_case("3mf")){let f=fs::File::open(&partial)?;let _=zip::ZipArchive::new(f)?;}
                    fs::rename(partial,destination)?;self.reply(226,"Transfer complete").await?
                }
            },
            "DELE"=>{let p=self.real(arg)?;if p.is_file(){fs::remove_file(p)?;self.reply(250,"File deleted").await?}else{self.reply(550,"File not found").await?}},
            "MKD"=>{let p=self.real(arg)?;fs::create_dir(&p)?;self.reply(257,"Directory created").await?},
            "RMD"=>{let p=self.real(arg)?;if p==self.config.root{self.reply(550,"Cannot remove staging root").await?}else{fs::remove_dir(p)?;self.reply(250,"Directory removed").await?}},
            "RNFR"=>{let p=self.real(arg)?;if p==self.config.root||!p.exists(){self.reply(550,"Path not found").await?}else{self.rename_from=Some(p);self.reply(350,"Ready for RNTO").await?}},
            "RNTO"=>{if let Some(from)=self.rename_from.take(){let to=self.real(arg)?;if to==self.config.root{self.reply(550,"Cannot replace staging root").await?}else{fs::rename(from,to)?;self.reply(250,"Rename successful").await?}}else{self.reply(503,"RNFR required first").await?}},
            "AVBL"=>self.reply(213,&self.config.max_total.saturating_sub(Self::used(&self.config.root)).to_string()).await?,
            "STAT"=>{if arg.trim().is_empty(){self.reply(211,"Server status OK").await?}else{let listing=String::from_utf8(self.list_bytes(arg,false,false)?)?;self.stream.get_mut().write_all(format!("211-Status\r\n{listing}211 End\r\n").as_bytes()).await?}},
            "ABOR"=>{self.passive=None;self.reply(226,"Abort successful").await?},
            _=>self.reply(502,&format!("Command {command} not implemented")).await?,
        }
        Ok(true)
    }
    async fn run(mut self) -> Result<()> {
        self.reply(220, "Bambu Cloud LAN Proxy FTPS ready").await?;
        let mut line = String::new();
        loop {
            line.clear();
            if tokio::time::timeout(Duration::from_secs(300), self.stream.read_line(&mut line))
                .await??
                == 0
            {
                break;
            }
            if line.len() > 16384 {
                break;
            }
            let raw = line.trim_end_matches(['\r', '\n']);
            let (cmd, arg) = raw.split_once(' ').unwrap_or((raw, ""));
            match self.command(&cmd.to_ascii_uppercase(), arg).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    tracing::warn!(error=%e,"FTPS command failed");
                    self.reply(550, &e.to_string()).await?
                }
            }
        }
        Ok(())
    }
}

pub async fn run() -> Result<()> {
    let config = Arc::new(Config::from_env()?);
    fs::create_dir_all(&config.root)?;
    for p in ["cache", "timelapse", "video", "model", "data/Metadata"] {
        fs::create_dir_all(config.root.join(p))?
    }
    let listener = TcpListener::bind((config.bind, config.port)).await?;
    let limit = Arc::new(Semaphore::new(32));
    tracing::info!(port = config.port, "implicit FTPS listening");
    loop {
        let (socket, _) = listener.accept().await?;
        let permit = limit.clone().acquire_owned().await?;
        let c = config.clone();
        tokio::spawn(async move {
            let _permit = permit;
            match c.acceptor.accept(socket).await {
                Ok(stream) => {
                    let session = Session {
                        config: c,
                        stream: BufReader::new(stream),
                        authenticated: false,
                        username: String::new(),
                        cwd: PathBuf::new(),
                        passive: None,
                        restart: 0,
                        rename_from: None,
                        private_data: true,
                    };
                    if let Err(e) = session.run().await {
                        tracing::debug!(error=%e,"FTPS session ended")
                    }
                }
                Err(e) => tracing::debug!(error=%e,"FTPS handshake failed"),
            }
        });
    }
}
