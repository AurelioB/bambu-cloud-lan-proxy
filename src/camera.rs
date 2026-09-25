use crate::config::env_value;
use anyhow::{Context, Result, bail};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rustls::pki_types::ServerName;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct CameraSource {
    pub serial: String,
    pub access_code: String,
}

pub fn load_source(path: &Path, requested: &str) -> Result<CameraSource> {
    let raw: Value = serde_json::from_slice(
        &fs::read(path)
            .with_context(|| format!("printer registry is not ready: {}", path.display()))?,
    )?;
    let values = raw
        .as_array()
        .context("printer registry must contain a list")?;
    let requested = requested.trim().to_ascii_uppercase();
    let mut matches = values.iter().filter(|v| v.is_object()).filter(|v| {
        requested.is_empty()
            || v.get("serial")
                .and_then(Value::as_str)
                .unwrap_or("")
                .eq_ignore_ascii_case(&requested)
    });
    let entry = matches.next().context(if requested.is_empty() {
        "CAMERA_PRINTER_SERIAL is required when there is not exactly one printer"
    } else {
        "requested camera printer is absent"
    })?;
    if matches.next().is_some()
        || requested.is_empty() && values.iter().filter(|v| v.is_object()).count() != 1
    {
        bail!("CAMERA_PRINTER_SERIAL is required when there is not exactly one printer")
    }
    let serial = entry
        .get("serial")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let access_code = entry
        .get("access_code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if serial.is_empty() || access_code.is_empty() {
        bail!("printer registry has no serial or LAN liveview access code")
    }
    Ok(CameraSource {
        serial,
        access_code,
    })
}
pub fn normalize_fingerprint(value: &str) -> Result<String> {
    let value = value.replace(':', "").trim().to_ascii_uppercase();
    if value.len() != 64 || !value.bytes().all(|v| v.is_ascii_hexdigit()) {
        bail!("camera TLS fingerprint must be a SHA-256 hex digest")
    }
    Ok(value)
}
async fn fetch_fingerprint(host: &str, port: u16) -> Result<String> {
    let socket = TcpStream::connect((host, port)).await?;
    let name = host
        .parse::<IpAddr>()
        .map(|value| ServerName::IpAddress(value.into()))
        .or_else(|_| ServerName::try_from(host.to_owned()))?;
    let stream = crate::tls::insecure_connector()
        .connect(name, socket)
        .await?;
    let cert = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|v| v.first())
        .context("camera endpoint supplied no certificate")?;
    Ok(hex::encode_upper(Sha256::digest(cert.as_ref())))
}
pub async fn resolve_fingerprint(
    host: &str,
    port: u16,
    serial: &str,
    state: &Path,
    configured: &str,
) -> Result<String> {
    if !configured.trim().is_empty() {
        return normalize_fingerprint(configured);
    }
    fs::create_dir_all(state)?;
    let path = state.join(format!("{serial}.sha256"));
    if let Ok(value) = fs::read_to_string(&path) {
        return normalize_fingerprint(value.trim());
    }
    let value = fetch_fingerprint(host, port).await?;
    let temporary = state.join(format!(".{serial}.{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, format!("{value}\n"))?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(temporary, path)?;
    Ok(value)
}

pub async fn run() -> Result<()> {
    let registry = PathBuf::from(env_value("CAMERA_GATEWAY_REGISTRY", "/data/printers.json"));
    let source = load_source(&registry, &env_value("CAMERA_PRINTER_SERIAL", ""))?;
    let upstream = env_value("CAMERA_UPSTREAM_IP", "");
    let upstream_ip: IpAddr = upstream
        .parse()
        .context("CAMERA_UPSTREAM_IP must be a literal IP address")?;
    let upstream_port = env_value("CAMERA_UPSTREAM_PORT", "322").parse::<u16>()?;
    let state = PathBuf::from(env_value("CAMERA_STATE_DIR", "/state"));
    let fingerprint = resolve_fingerprint(
        &upstream,
        upstream_port,
        &source.serial,
        &state,
        &env_value("CAMERA_UPSTREAM_TLS_FINGERPRINT", ""),
    )
    .await?;
    let listen = env_value("CAMERA_LISTEN_IP", "127.0.0.1");
    let resolved = tokio::net::lookup_host((listen.as_str(), 0))
        .await?
        .next()
        .context("CAMERA_LISTEN_IP could not be resolved")?
        .ip();
    let safe = match resolved {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v == Ipv4Addr::UNSPECIFIED,
        IpAddr::V6(v) => v.is_loopback() || v.is_unique_local() || v.is_unspecified(),
    };
    if !safe {
        bail!("CAMERA_LISTEN_IP must resolve to a private or loopback address")
    }
    let downstream = env_value("PROXY_ACCESS_CODE", "");
    if downstream.is_empty() {
        bail!("PROXY_ACCESS_CODE is empty")
    }
    let cert = PathBuf::from(env_value("CAMERA_SERVER_CERT", "/secrets/certs/server.crt"));
    let key = PathBuf::from(env_value("CAMERA_SERVER_KEY", "/secrets/certs/server.key"));
    if !cert.is_file() || !key.is_file() {
        bail!("proxy TLS certificate or key is missing")
    }
    let listen_port = env_value("CAMERA_LISTEN_PORT", "8322").parse::<u16>()?;
    let source_url = format!(
        "rtsps://{}:{}@{}:{}/streaming/live/1",
        utf8_percent_encode("bblp", NON_ALPHANUMERIC),
        utf8_percent_encode(&source.access_code, NON_ALPHANUMERIC),
        upstream_ip,
        upstream_port
    );
    let config = json!({"logLevel":env_value("CAMERA_LOG_LEVEL","info").to_ascii_lowercase(),"readTimeout":"20s","writeTimeout":"20s","authMethod":"internal","authInternalUsers":[{"user":"bblp","pass":downstream,"ips":[],"permissions":[{"action":"read","path":"streaming/live/1"}]}],"api":false,"metrics":false,"pprof":false,"playback":false,"rtsp":true,"rtspTransports":["tcp"],"rtspEncryption":"strict","rtspsAddress":format!("{resolved}:{listen_port}"),"rtspServerCert":cert,"rtspServerKey":key,"rtspAuthMethods":["basic"],"rtmp":false,"hls":false,"webrtc":false,"srt":false,"moq":false,"paths":{"streaming/live/1":{"source":source_url,"sourceFingerprint":fingerprint,"sourceOnDemand":true,"sourceOnDemandStartTimeout":"20s","sourceOnDemandCloseAfter":"10s","rtspTransport":"tcp"}}});
    let path = PathBuf::from(env_value("CAMERA_CONFIG_PATH", "/tmp/mediamtx.json"));
    fs::write(&path, serde_json::to_vec(&config)?)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let status = tokio::process::Command::new("/usr/local/bin/mediamtx")
        .arg(&path)
        .status()
        .await?;
    if !status.success() {
        bail!("MediaMTX exited with {status}")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_fingerprint() {
        assert_eq!(
            normalize_fingerprint(&("AA:".repeat(31) + "AA")).unwrap(),
            "AA".repeat(32)
        );
        assert!(normalize_fingerprint("GG").is_err())
    }
}
