use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;
use zip::ZipArchive;

pub const VERSION: &str = "02.05.02.58";
const LIMIT: usize = 128 * 1024 * 1024;
const PINS: [(&str, &str); 2] = [
    (
        "libbambu_networking.so",
        "c9bdb8e6868c461d5c456dc7ba913744e449e2cc4ce2f29e175b381a1ff95d05",
    ),
    (
        "libBambuSource.so",
        "506a202d225e9efb7e082b7920983bcadb93e85f0927d7b5b43c4dd7bf7d2f21",
    ),
];

pub fn api_base(region: &str) -> Result<&'static str> {
    match region {
        "US" => Ok("https://api.bambulab.com"),
        "CN" => Ok("https://api.bambulab.cn"),
        _ => bail!("BAMBU_CLOUD_REGION must be US or CN"),
    }
}

pub fn studio_headers() -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&format!("BambuStudio/{VERSION}"))?,
    );
    for (name, value) in [
        ("x-bbl-client-type", "slicer"),
        ("x-bbl-client-name", "BambuStudio"),
        ("x-bbl-client-version", VERSION),
        ("x-bbl-os-type", "linux"),
    ] {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes())?,
            HeaderValue::from_static(value),
        );
    }
    Ok(headers)
}

pub fn validate_binary(name: &str, payload: &[u8]) -> Result<()> {
    if payload.get(..6) != Some(b"\x7fELF\x02\x01") || payload.get(18..20) != Some(b"\x3e\x00") {
        bail!("plugin must be a little-endian x86_64 ELF binary")
    }
    let expected = PINS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .context("unexpected plugin file")?
        .1;
    if hex::encode(Sha256::digest(payload)) != expected {
        bail!("plugin does not match reviewed SHA-256 pin")
    }
    Ok(())
}

pub fn unpack(payload: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let mut result = BTreeMap::new();
    for index in 0..archive.len() {
        let member = archive.by_index(index)?;
        let Some(path) = member.enclosed_name() else {
            continue;
        };
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if !PINS.iter().any(|(required, _)| *required == name) {
            continue;
        }
        if result.contains_key(&name) || member.size() as usize > LIMIT {
            bail!("duplicate or oversized plugin binary")
        }
        let mut data = Vec::with_capacity(member.size() as usize);
        member.take(LIMIT as u64 + 1).read_to_end(&mut data)?;
        if data.len() > LIMIT {
            bail!("oversized plugin binary")
        }
        validate_binary(&name, &data)?;
        result.insert(name, data);
    }
    if result.len() != PINS.len() {
        bail!("plugin archive is incomplete")
    }
    Ok(result)
}

pub async fn ensure(directory: &Path, region: &str) -> Result<()> {
    if std::env::consts::ARCH != "x86_64" {
        bail!("the pinned Bambu plugin requires x86_64 Linux")
    }
    let present = PINS.iter().all(|(name, _)| directory.join(name).is_file());
    if present {
        for (name, _) in PINS {
            validate_binary(name, &fs::read(directory.join(name))?)?;
        }
        return Ok(());
    }
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let manifest: Value = client
        .get(format!(
            "{}/v1/iot-service/api/slicer/resource?slicer/plugins/cloud=02.05.02.00",
            api_base(region)?
        ))
        .headers(studio_headers()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let matches: Vec<&Value> = manifest
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("slicer/plugins/cloud")
                && entry.get("version").and_then(Value::as_str) == Some(VERSION)
        })
        .collect();
    if matches.len() != 1 {
        bail!("Bambu no longer advertises the reviewed plugin version")
    }
    let url = Url::parse(
        matches[0]
            .get("url")
            .and_then(Value::as_str)
            .context("plugin URL missing")?,
    )?;
    if url.scheme() != "https" {
        bail!("plugin download requires HTTPS")
    }
    let response = client.get(url).send().await?.error_for_status()?;
    let mut stream = response.bytes_stream();
    let mut blob = Vec::new();
    while let Some(chunk) = stream.next().await {
        blob.extend_from_slice(&chunk?);
        if blob.len() > LIMIT {
            bail!("plugin download exceeds size limit")
        }
    }
    let files = tokio::task::spawn_blocking(move || unpack(&blob)).await??;
    fs::create_dir_all(directory)?;
    for (name, payload) in files {
        let temporary = directory.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&payload)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, directory.join(name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unreviewed_payloads() {
        assert!(validate_binary("libbambu_networking.so", b"not ELF").is_err());
    }
}
