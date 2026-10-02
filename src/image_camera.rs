//! Local chamber-image transport. The native-file listener dispatches the
//! distinct login header before reading a body, so both protocols share :6000.
use anyhow::{Context, Result, bail};
use rustls::pki_types::ServerName;
use std::{net::IpAddr, path::PathBuf, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tokio_rustls::server::TlsStream;

use crate::{
    camera,
    config::{env_bool, env_value},
};

const MAX_IMAGE: usize = 10_000_000;
const IO_TIMEOUT: Duration = Duration::from_secs(20);
pub const LOGIN_HEADER: [u8; 16] = [64, 0, 0, 0, 0, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
type Frame = Option<Arc<Vec<u8>>>;

fn login(access: &str) -> Result<[u8; 80]> {
    if access.len() != 8 || !access.bytes().all(|v| v.is_ascii_alphanumeric()) {
        bail!("camera access code must be eight letters or digits")
    }
    let mut packet = [0; 80];
    packet[..16].copy_from_slice(&LOGIN_HEADER);
    packet[16..20].copy_from_slice(b"bblp");
    packet[48..56].copy_from_slice(access.as_bytes());
    Ok(packet)
}

pub fn is_image_login(header: &[u8; 16]) -> bool {
    *header == LOGIN_HEADER
}

async fn read_frame<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Vec<u8>> {
    let mut header = [0; 16];
    stream.read_exact(&mut header).await?;
    let length = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    if !(4..=MAX_IMAGE).contains(&length) {
        bail!("invalid camera frame length")
    }
    let mut packet = vec![0; 16 + length];
    packet[..16].copy_from_slice(&header);
    stream.read_exact(&mut packet[16..]).await?;
    if packet[16..18] != [0xff, 0xd8] || packet[packet.len() - 2..] != [0xff, 0xd9] {
        bail!("invalid or truncated camera JPEG")
    }
    Ok(packet)
}

#[derive(Clone)]
struct Upstream {
    host: IpAddr,
    port: u16,
    source: camera::CameraSource,
    state: PathBuf,
    fingerprint: String,
}
impl Upstream {
    async fn connect(&self) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
        let fingerprint = camera::resolve_fingerprint(
            &self.host.to_string(),
            self.port,
            &format!("{}-image", self.source.serial),
            &self.state,
            &self.fingerprint,
        )
        .await?;
        let connector = tokio_rustls::TlsConnector::from(Arc::new(
            crate::tls::pinned_client_config(&fingerprint)?,
        ));
        let socket = TcpStream::connect((self.host, self.port)).await?;
        let mut stream = connector
            .connect(ServerName::IpAddress(self.host.into()), socket)
            .await?;
        stream.write_all(&login(&self.source.access_code)?).await?;
        Ok(stream)
    }

    async fn pump(&self, sender: watch::Sender<Frame>) {
        // One physical connection per virtual printer, shared by all viewers.
        // Only the latest frame is retained; a slow viewer cannot grow a queue.
        loop {
            let result: Result<()> = tokio::select! {
                _ = sender.closed() => return,
                result = async {
                    let mut stream = tokio::time::timeout(IO_TIMEOUT, self.connect()).await??;
                    loop {
                        let packet = tokio::time::timeout(IO_TIMEOUT, read_frame(&mut stream)).await??;
                        sender.send(Some(Arc::new(packet))).context("camera has no viewers")?;
                    }
                } => result,
            };
            if let Err(error) = result {
                tracing::debug!(%error, "local image camera disconnected; retrying");
            }
            sender.send_replace(None);
            tokio::select! {
                _ = sender.closed() => return,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

struct Pump {
    sender: watch::Sender<Frame>,
    task: JoinHandle<()>,
}
impl Drop for Pump {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
pub struct ImageCamera {
    upstream: Upstream,
    pump: Arc<Mutex<Option<Pump>>>,
}
impl ImageCamera {
    #[cfg(test)]
    pub fn fixture(host: IpAddr, port: u16, fingerprint: String, state: PathBuf) -> Self {
        Self {
            upstream: Upstream {
                host,
                port,
                source: camera::CameraSource {
                    serial: "TESTIMAGE".into(),
                    access_code: "REAL1234".into(),
                },
                state,
                fingerprint,
            },
            pump: Arc::new(Mutex::new(None)),
        }
    }
    pub fn from_env() -> Result<Option<Self>> {
        if !env_bool("IMAGE_CAMERA_ENABLED", false)? {
            return Ok(None);
        }
        let host: IpAddr = env_value("IMAGE_CAMERA_UPSTREAM_IP", "")
            .parse()
            .context("image camera requires a physical IP")?;
        let private = match host {
            IpAddr::V4(ip) => ip.is_private() || ip.is_loopback(),
            IpAddr::V6(ip) => ip.is_unique_local() || ip.is_loopback(),
        };
        if !private {
            bail!("image camera requires a private or loopback physical IP")
        }
        let source = camera::load_source(
            &PathBuf::from(env_value("IMAGE_CAMERA_REGISTRY", "/data/printers.json")),
            &env_value("FTPS_REMOTE_SERIAL", ""),
        )?;
        login(&source.access_code)?;
        let upstream = Upstream {
            host,
            port: env_value("IMAGE_CAMERA_UPSTREAM_PORT", "6000").parse()?,
            source,
            state: PathBuf::from(env_value("IMAGE_CAMERA_STATE_DIR", "/camera-state")),
            fingerprint: env_value("IMAGE_CAMERA_TLS_FINGERPRINT", ""),
        };
        Ok(Some(Self {
            upstream,
            pump: Arc::new(Mutex::new(None)),
        }))
    }

    async fn subscribe(&self) -> watch::Receiver<Frame> {
        let mut current = self.pump.lock().await;
        if let Some(pump) = current.as_ref()
            && !pump.task.is_finished()
            && pump.sender.receiver_count() > 0
        {
            return pump.sender.subscribe();
        }
        // Abort a closing worker before creating its replacement.
        if let Some(mut pump) = current.take() {
            pump.task.abort();
            let _ = (&mut pump.task).await;
        }
        let (sender, receiver) = watch::channel(None);
        let upstream = self.upstream.clone();
        let worker = sender.clone();
        let task = tokio::spawn(async move { upstream.pump(worker).await });
        *current = Some(Pump { sender, task });
        receiver
    }

    pub async fn serve(&self, mut stream: TlsStream<TcpStream>, access: &str) -> Result<()> {
        // Header already consumed by the shared listener. Authenticate before
        // starting any physical connection or subscribing to existing frames.
        let mut credentials = [0; 64];
        tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut credentials)).await??;
        if credentials.ct_eq(&login(access)?[16..]).unwrap_u8() != 1 {
            stream.shutdown().await?;
            bail!("image camera login rejected")
        }
        let mut frames = self.subscribe().await;
        let mut extra = [0];
        loop {
            let frame = frames.borrow_and_update().clone();
            if let Some(frame) = frame {
                tokio::time::timeout(IO_TIMEOUT, stream.write_all(&frame)).await??;
            }
            // Client EOF releases its subscription immediately, even if the
            // physical camera is unavailable. Extra client commands are refused.
            tokio::select! {
                result = stream.read(&mut extra) => { result?; return Ok(()); }
                result = tokio::time::timeout(Duration::from_secs(45), frames.changed()) => { result??; }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn tls_fixture() -> tempfile::TempDir {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-keyout",
            ])
            .arg(dir.path().join("key.pem"))
            .arg("-out")
            .arg(dir.path().join("cert.pem"))
            .output()
            .unwrap();
        assert!(output.status.success());
        dir
    }

    pub fn certificate_pin(dir: &std::path::Path) -> String {
        use sha2::Digest;
        let cert = std::fs::read(dir.join("cert.pem")).unwrap();
        let cert = rustls_pemfile::certs(&mut &cert[..])
            .next()
            .unwrap()
            .unwrap();
        hex::encode_upper(sha2::Sha256::digest(cert.as_ref()))
    }

    pub fn frame(sequence: u8) -> Vec<u8> {
        let mut frame = vec![0; 16];
        frame[..4].copy_from_slice(&6u32.to_le_bytes());
        frame[8] = sequence;
        frame.extend_from_slice(&[0xff, 0xd8, 1, 2, 0xff, 0xd9]);
        frame
    }

    #[tokio::test]
    async fn tls_pin_rejects_a_changed_certificate_before_authentication() {
        let dir = tls_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let acceptor =
            crate::tls::acceptor(&dir.path().join("cert.pem"), &dir.path().join("key.pem"))
                .unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            assert!(acceptor.accept(socket).await.is_err());
        });
        let camera = ImageCamera::fixture(
            address.ip(),
            address.port(),
            "AA".repeat(32),
            dir.path().to_owned(),
        );
        assert!(camera.upstream.connect().await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn viewers_share_one_upstream_and_last_viewer_releases_it() {
        let dir = tls_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let acceptor =
            crate::tls::acceptor(&dir.path().join("cert.pem"), &dir.path().join("key.pem"))
                .unwrap();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(socket).await.unwrap();
            let mut auth = [0; 80];
            stream.read_exact(&mut auth).await.unwrap();
            assert_eq!(auth, login("REAL1234").unwrap());
            stream.write_all(&frame(1)).await.unwrap();
            ready_tx.send(()).unwrap();
            let mut byte = [0];
            let result = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
                .await
                .unwrap();
            assert!(
                matches!(result, Ok(0))
                    || result.is_err_and(|error| error.kind() == std::io::ErrorKind::UnexpectedEof)
            );
        });
        let camera = ImageCamera::fixture(
            address.ip(),
            address.port(),
            certificate_pin(dir.path()),
            dir.path().to_owned(),
        );
        let mut first = camera.subscribe().await;
        let second = camera.subscribe().await;
        ready_rx.await.unwrap();
        first.changed().await.unwrap();
        assert_eq!(
            first.borrow_and_update().as_deref().unwrap().as_slice(),
            frame(1)
        );
        assert_eq!(second.borrow().as_deref().unwrap().as_slice(), frame(1));
        assert_eq!(
            camera
                .pump
                .lock()
                .await
                .as_ref()
                .unwrap()
                .sender
                .receiver_count(),
            2
        );
        drop(first);
        drop(second);
        server.await.unwrap();
        drop(camera);
    }

    #[tokio::test]
    async fn upstream_disconnect_reconnects_for_active_viewers() {
        let dir = tls_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let acceptor =
            crate::tls::acceptor(&dir.path().join("cert.pem"), &dir.path().join("key.pem"))
                .unwrap();
        let server = tokio::spawn(async move {
            for sequence in [1, 2] {
                let (socket, _) = listener.accept().await.unwrap();
                let mut stream = acceptor.accept(socket).await.unwrap();
                let mut auth = [0; 80];
                stream.read_exact(&mut auth).await.unwrap();
                stream.write_all(&frame(sequence)).await.unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        let camera = ImageCamera::fixture(
            address.ip(),
            address.port(),
            certificate_pin(dir.path()),
            dir.path().to_owned(),
        );
        let mut frames = camera.subscribe().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                frames.changed().await.unwrap();
                if frames
                    .borrow_and_update()
                    .as_ref()
                    .is_some_and(|packet| packet[8] == 2)
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        drop(frames);
        server.await.unwrap();
        drop(camera);
    }

    #[test]
    fn login_protocols_are_distinct_and_credentials_are_padded() {
        let packet = login("TEST1234").unwrap();
        assert!(is_image_login(packet[..16].try_into().unwrap()));
        assert_eq!(&packet[16..20], b"bblp");
        assert_eq!(&packet[48..56], b"TEST1234");
        let mut file = [0; 16];
        file[..4].copy_from_slice(&16u32.to_le_bytes());
        file[4..8].copy_from_slice(&0x0101013fu32.to_le_bytes());
        assert!(!is_image_login(&file));
        assert!(login("bad").is_err());
    }

    #[tokio::test]
    async fn fragmented_frames_preserve_headers_and_reject_invalid_sizes_and_jpegs() {
        let (mut tx, mut rx) = tokio::io::duplex(64);
        let mut expected = vec![0; 16];
        expected[..4].copy_from_slice(&6u32.to_le_bytes());
        expected[8] = 42;
        expected.extend_from_slice(&[0xff, 0xd8, 1, 2, 0xff, 0xd9]);
        let copy = expected.clone();
        tokio::spawn(async move {
            for chunk in copy.chunks(3) {
                tx.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        assert_eq!(read_frame(&mut rx).await.unwrap(), expected);
        let mut header = [0; 16];
        header[..4].copy_from_slice(&((MAX_IMAGE + 1) as u32).to_le_bytes());
        assert!(read_frame(&mut &header[..]).await.is_err());
        header[..4].copy_from_slice(&4u32.to_le_bytes());
        let mut invalid = header.to_vec();
        invalid.extend_from_slice(&[1, 2, 3, 4]);
        assert!(read_frame(&mut &invalid[..]).await.is_err());
    }
}
