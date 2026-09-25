use crate::config::env_value;
use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Map, Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::server::TlsStream;
use uuid::Uuid;

const CLIENT_LOGIN: u32 = 0x0101013f;
const SERVER_LOGIN: u32 = 0x0001013f;
const CLIENT_CTRL: u32 = 0x0102013f;
const SERVER_CTRL: u32 = 0x0002013f;
const MAX_FRAME: usize = 2 * 1024 * 1024;

#[derive(Clone)]
struct Remote {
    client: reqwest::Client,
    base: String,
    serial: String,
    cache: PathBuf,
}
impl Remote {
    async fn post(&self, op: &str, mut body: Value) -> Result<Value> {
        body["serial"] = self.serial.clone().into();
        let response = self
            .client
            .post(format!("{}/{}", self.base, op))
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }
    async fn cached(&self, op: &str, body: Value) -> Result<PathBuf> {
        let value = self.post(op, body).await?;
        let key = value
            .get("cache_key")
            .and_then(Value::as_str)
            .context("storage response omitted cache key")?;
        if key.len() != 32 || !key.bytes().all(|v| v.is_ascii_alphanumeric()) {
            bail!("invalid cache key")
        }
        let path = self.cache.join(key);
        if !path.is_file() {
            bail!("downloaded file is unavailable")
        }
        Ok(path)
    }
    async fn discard(&self, path: &Path) {
        let _ = self
            .post(
                "discard",
                json!({"cache_key":path.file_name().and_then(|v|v.to_str()).unwrap_or("")}),
            )
            .await;
    }
}

struct Upload {
    sequence: u64,
    partial: PathBuf,
    name: String,
    storage: String,
    total: u64,
    offset: u64,
    digest: Md5,
}
struct Session {
    stream: TlsStream<TcpStream>,
    access: Arc<str>,
    remote: Remote,
    staging: PathBuf,
    max_upload: u64,
    send_sequence: u32,
    upload: Option<Upload>,
}
impl Session {
    async fn read_frame(&mut self) -> Result<(u32, u32, Vec<u8>)> {
        let mut h = [0u8; 16];
        self.stream.read_exact(&mut h).await?;
        let size = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
        let magic = u32::from_le_bytes(h[4..8].try_into().unwrap());
        let sequence = u32::from_le_bytes(h[8..12].try_into().unwrap());
        if size > MAX_FRAME || h[12..16] != [0; 4] {
            bail!("invalid native file frame")
        }
        let mut payload = vec![0; size];
        self.stream.read_exact(&mut payload).await?;
        Ok((magic, sequence, payload))
    }
    async fn send_frame(&mut self, magic: u32, payload: &[u8]) -> Result<()> {
        let mut h = Vec::with_capacity(16);
        h.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        h.extend_from_slice(&magic.to_le_bytes());
        h.extend_from_slice(&self.send_sequence.to_le_bytes());
        h.extend_from_slice(&0u32.to_le_bytes());
        self.send_sequence = self.send_sequence.wrapping_add(1);
        self.stream.write_all(&h).await?;
        self.stream.write_all(payload).await?;
        self.stream.flush().await?;
        Ok(())
    }
    async fn reply(
        &mut self,
        command: u64,
        sequence: u64,
        result: u64,
        reply: Value,
        data: &[u8],
    ) -> Result<()> {
        let mut payload = serde_json::to_vec(
            &json!({"mtype":0x3001,"cmdtype":command,"sequence":sequence,"result":result,"reply":reply}),
        )?;
        if !data.is_empty() {
            payload.extend_from_slice(b"\n\n");
            payload.extend_from_slice(data)
        }
        self.send_frame(SERVER_CTRL, &payload).await
    }
    async fn binary(&mut self, command: u64, sequence: u64, path: PathBuf) -> Result<()> {
        let bytes = fs::read(&path).await?;
        let digest = hex::encode(Md5::digest(&bytes));
        if bytes.is_empty() {
            self.reply(
                command,
                sequence,
                0,
                json!({"offset":0,"size":0,"total":0,"file_md5":digest}),
                &[],
            )
            .await?
        } else {
            let total = bytes.len();
            for (index, chunk) in bytes.chunks(512 * 1024).enumerate() {
                let start = index * 512 * 1024;
                let final_chunk = start + chunk.len() == total;
                let mut meta = json!({"offset":start,"size":chunk.len(),"total":total});
                if final_chunk {
                    meta["file_md5"] = digest.clone().into()
                }
                self.reply(
                    command,
                    sequence,
                    if final_chunk { 0 } else { 1 },
                    meta,
                    chunk,
                )
                .await?
            }
        }
        self.remote.discard(&path).await;
        Ok(())
    }
    async fn handle(
        &mut self,
        command: u64,
        sequence: u64,
        body: &Map<String, Value>,
        data: &[u8],
    ) -> Result<()> {
        match command {
            7 => {
                let value = self
                    .remote
                    .post("ability", json!({"api_version":2}))
                    .await?;
                self.reply(
                    command,
                    sequence,
                    0,
                    json!({"storage":value.get("storage").cloned().unwrap_or(json!(["internal"]))}),
                    &[],
                )
                .await?
            }
            1 => {
                let kind = body.get("type").and_then(Value::as_str).unwrap_or("");
                if !matches!(kind, "model" | "timelapse" | "video") {
                    self.reply(command, sequence, 15, json!({}), &[]).await?
                } else {
                    let value=self.remote.post("list",json!({"type":kind,"storage":body.get("storage").and_then(Value::as_str).unwrap_or("")})).await?;
                    self.reply(
                        command,
                        sequence,
                        0,
                        json!({"file_lists":value.get("files").cloned().unwrap_or(json!([]))}),
                        &[],
                    )
                    .await?
                }
            }
            2 => {
                let path = self
                    .remote
                    .cached("subfile", json!({"request":body}))
                    .await?;
                self.binary(command, sequence, path).await?
            }
            4 => {
                let path=self.remote.cached("download",json!({"path":body.get("path").and_then(Value::as_str).unwrap_or(""),"name":body.get("file").and_then(Value::as_str).unwrap_or("")})).await?;
                self.binary(command, sequence, path).await?
            }
            3 => {
                let paths = body.get("paths").cloned().unwrap_or_else(|| {
                    json!([body
                        .get("path")
                        .or_else(|| body.get("file"))
                        .cloned()
                        .unwrap_or(Value::Null)])
                });
                self.remote.post("delete", json!({"paths":paths})).await?;
                self.reply(command, sequence, 0, json!({}), &[]).await?;
                self.notify().await?
            }
            5 => self.upload(command, sequence, body, data).await?,
            0x1000 => {
                if let Some(upload) = self.upload.take() {
                    let cancel = body
                        .get("tasks")
                        .and_then(Value::as_array)
                        .is_some_and(|v| v.iter().any(|n| n.as_u64() == Some(upload.sequence)));
                    if cancel {
                        let _ = fs::remove_file(upload.partial).await;
                    } else {
                        self.upload = Some(upload)
                    }
                }
                self.reply(
                    command,
                    sequence,
                    0,
                    json!({"tasks":body.get("tasks").cloned().unwrap_or(json!([]))}),
                    &[],
                )
                .await?
            }
            _ => self.reply(command, sequence, 18, json!({}), &[]).await?,
        }
        Ok(())
    }
    async fn upload(
        &mut self,
        command: u64,
        sequence: u64,
        body: &Map<String, Value>,
        data: &[u8],
    ) -> Result<()> {
        if data.is_empty() {
            let total = body.get("total").and_then(Value::as_u64).unwrap_or(0);
            let name = Path::new(body.get("path").and_then(Value::as_str).unwrap_or(""))
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_owned();
            if total == 0 || total > self.max_upload || name.is_empty() {
                return self.reply(command, sequence, 21, json!({}), &[]).await;
            }
            let partial = self
                .staging
                .join(format!(".proxy-native-{}.part", Uuid::new_v4()));
            fs::File::create(&partial).await?;
            self.upload = Some(Upload {
                sequence,
                partial,
                name,
                storage: body
                    .get("storage")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                total,
                offset: 0,
                digest: Md5::new(),
            });
            return self
                .reply(
                    command,
                    sequence,
                    1,
                    json!({"chunk_size":255,"offset":0}),
                    &[],
                )
                .await;
        }
        let Some(mut state) = self.upload.take() else {
            return self.reply(command, sequence, 8, json!({}), &[]).await;
        };
        let offset = body
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX);
        if state.sequence != sequence
            || offset != state.offset
            || body.get("size").and_then(Value::as_u64) != Some(data.len() as u64)
            || offset + data.len() as u64 > state.total
        {
            let _ = fs::remove_file(state.partial).await;
            bail!("upload frame mismatch")
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&state.partial)
            .await?;
        file.write_all(data).await?;
        state.digest.update(data);
        state.offset += data.len() as u64;
        if state.offset < state.total {
            self.upload = Some(state);
            return Ok(());
        }
        let actual = hex::encode(state.digest.finalize());
        let expected = body.get("file_md5").and_then(Value::as_str).unwrap_or("");
        if !expected.is_empty() && expected.as_bytes().ct_eq(actual.as_bytes()).unwrap_u8() != 1 {
            let _ = fs::remove_file(state.partial).await;
            bail!("upload checksum mismatch")
        }
        let destination = self.staging.join("cache").join(&state.name);
        fs::create_dir_all(destination.parent().unwrap()).await?;
        fs::rename(&state.partial, &destination).await?;
        self.remote
            .post(
                "upload",
                json!({"source_path":destination,"path":state.name,"storage":state.storage}),
            )
            .await?;
        self.reply(command, sequence, 0, json!({}), &[]).await?;
        self.notify().await
    }
    async fn notify(&mut self) -> Result<()> {
        let payload =
            serde_json::to_vec(&json!({"mtype":0x3001,"cmdtype":0x100,"sequence":0,"notify":{}}))?;
        self.send_frame(SERVER_CTRL, &payload).await
    }
    async fn run(mut self) -> Result<()> {
        let (magic, _, login) =
            tokio::time::timeout(Duration::from_secs(15), self.read_frame()).await??;
        if magic != CLIENT_LOGIN || login.len() != 16 {
            bail!("invalid login frame")
        }
        let user = String::from_utf8_lossy(&login[..8])
            .trim_matches('\0')
            .to_owned();
        let password = String::from_utf8_lossy(&login[8..])
            .trim_matches('\0')
            .to_owned();
        if user != "bblp"
            || password
                .as_bytes()
                .ct_eq(self.access.as_bytes())
                .unwrap_u8()
                != 1
        {
            bail!("login rejected")
        }
        self.send_frame(SERVER_LOGIN, &[0; 4]).await?;
        let (magic, _, setup) = self.read_frame().await?;
        let setup: Value = serde_json::from_slice(&setup)?;
        if magic != CLIENT_CTRL || setup.get("mtype").and_then(Value::as_u64) != Some(0x3003) {
            bail!("invalid CTRL setup")
        }
        self.send_frame(SERVER_CTRL,&serde_json::to_vec(&json!({"mtype":0x3003,"sequence":setup.get("sequence").cloned().unwrap_or(0.into()),"result":0,"reply":{}}))?).await?;
        loop {
            let (magic, _, payload) = self.read_frame().await?;
            if magic != CLIENT_CTRL {
                bail!("invalid CTRL frame")
            }
            let (split, data) = payload
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|i| (&payload[..i], &payload[i + 2..]))
                .unwrap_or((&payload[..], &[][..]));
            let request: Value = serde_json::from_slice(split)?;
            let command = request.get("cmdtype").and_then(Value::as_u64).unwrap_or(0);
            let sequence = request.get("sequence").and_then(Value::as_u64).unwrap_or(0);
            let body = request
                .get("req")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Err(e) = self.handle(command, sequence, &body, data).await {
                tracing::warn!(error=%e,command,"native file command failed");
                self.reply(command, sequence, 17, json!({}), &[]).await?
            }
        }
    }
}

pub async fn run() -> Result<()> {
    let access: Arc<str> = env_value("FTPS_ACCESS_CODE", "").into();
    if access.len() != 8 {
        bail!("FTPS_ACCESS_CODE must be eight characters")
    }
    let cert = PathBuf::from(env_value("FTPS_CERT_FILE", "/secrets/certs/server.crt"));
    let key = PathBuf::from(env_value("FTPS_KEY_FILE", "/secrets/certs/server.key"));
    let acceptor = crate::tls::acceptor(&cert, &key)?;
    let staging = PathBuf::from(env_value("FTPS_ROOT", "/staging"));
    fs::create_dir_all(staging.join("cache")).await?;
    let mut headers = HeaderMap::new();
    headers.insert("x-cloud-lan-proxy-key", HeaderValue::from_str(&access)?);
    let remote = Remote {
        client: reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(21 * 60))
            .build()?,
        base: env_value(
            "FTPS_REMOTE_API_URL",
            "http://gateway:4844/internal/cloud-lan-proxy/storage",
        ),
        serial: env_value("FTPS_REMOTE_SERIAL", ""),
        cache: env_value("FTPS_REMOTE_CACHE_DIR", "/remote-cache").into(),
    };
    if remote.serial.is_empty() {
        bail!("FTPS_REMOTE_SERIAL is required")
    }
    let listener = TcpListener::bind((
        env_value("NATIVE_FILES_BIND_HOST", "0.0.0.0"),
        env_value("NATIVE_FILES_PORT", "6000").parse::<u16>()?,
    ))
    .await?;
    loop {
        let (socket, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let access = access.clone();
        let remote = remote.clone();
        let staging = staging.clone();
        let max_upload = env_value("FTPS_MAX_UPLOAD_MB", "1024").parse::<u64>()? * 1024 * 1024;
        tokio::spawn(async move {
            if let Ok(stream) = acceptor.accept(socket).await {
                let session = Session {
                    stream,
                    access,
                    remote,
                    staging,
                    max_upload,
                    send_sequence: 1,
                    upload: None,
                };
                if let Err(e) = session.run().await {
                    tracing::debug!(error=%e,"native file session ended")
                }
            }
        });
    }
}
