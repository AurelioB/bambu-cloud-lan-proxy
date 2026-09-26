use std::{
    collections::BTreeMap,
    fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::{Child, Command};

use crate::config::{env_bool, env_value};

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Endpoint {
    serial: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    machine_model: String,
    #[serde(default)]
    ip: String,
    #[serde(default)]
    access_code: String,
    #[serde(default)]
    virtual_ip: String,
    #[serde(default = "enabled")]
    endpoint_enabled: bool,
}

fn enabled() -> bool {
    true
}

fn validate(endpoint: &mut Endpoint) -> Result<()> {
    endpoint.serial = endpoint.serial.trim().to_ascii_uppercase();
    if endpoint.serial.len() < 5
        || endpoint.serial.len() > 40
        || !endpoint.serial.bytes().all(|v| v.is_ascii_alphanumeric())
    {
        bail!("invalid endpoint serial")
    }
    let ip: Ipv4Addr = endpoint
        .virtual_ip
        .parse()
        .context("virtual_ip must be an IPv4 address")?;
    if !(ip.is_private() || ip.is_loopback()) {
        bail!("virtual_ip must be private or loopback")
    }
    if !endpoint.ip.is_empty() {
        let physical: IpAddr = endpoint
            .ip
            .parse()
            .context("physical printer ip must be an IP address")?;
        let safe = match physical {
            IpAddr::V4(value) => value.is_private() || value.is_loopback(),
            IpAddr::V6(value) => value.is_unique_local() || value.is_loopback(),
        };
        if !safe {
            bail!("physical printer ip must be private or loopback")
        }
    }
    if !endpoint.access_code.is_empty()
        && (endpoint.access_code.len() != 8
            || !endpoint
                .access_code
                .bytes()
                .all(|v| v.is_ascii_alphanumeric()))
    {
        bail!("physical access code must be eight letters or digits")
    }
    Ok(())
}

fn load(path: &Path) -> Result<BTreeMap<String, Endpoint>> {
    let mut result = BTreeMap::new();
    let entries: Vec<Endpoint> = serde_json::from_slice(&fs::read(path)?)?;
    for mut endpoint in entries {
        if !endpoint.endpoint_enabled || endpoint.virtual_ip.trim().is_empty() {
            continue;
        }
        validate(&mut endpoint)?;
        if result
            .values()
            .any(|value: &Endpoint| value.virtual_ip == endpoint.virtual_ip)
        {
            bail!(
                "virtual IP {} is assigned more than once",
                endpoint.virtual_ip
            )
        }
        result.insert(endpoint.serial.clone(), endpoint);
    }
    Ok(result)
}

fn child(command: &str, environment: &[(&str, String)]) -> Result<Child> {
    let mut process = Command::new(std::env::current_exe()?);
    process
        .arg(command)
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    Ok(process.spawn()?)
}

fn optional_child(command: &str, environment: &[(&str, String)]) -> Result<Child> {
    let mut values = environment.to_vec();
    values.push(("ENDPOINT_OPTIONAL_COMMAND", command.to_owned()));
    child("endpoint-optional", &values)
}

async fn stop(children: &mut Vec<Child>) {
    for child in children.iter_mut() {
        terminate(child).await;
    }
    children.clear();
}

async fn termination_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal?,
            _ = terminate.recv() => {}
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}

async fn terminate(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .await;
        if tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .is_ok()
        {
            return;
        }
    }
    let _ = child.kill().await;
}

pub async fn run_endpoint() -> Result<()> {
    let serial = env_value("ENDPOINT_SERIAL", "").to_ascii_uppercase();
    let virtual_ip = env_value("ENDPOINT_VIRTUAL_IP", "");
    let physical_ip = env_value("ENDPOINT_PHYSICAL_IP", "");
    let access = env_value("PROXY_ACCESS_CODE", "");
    if serial.is_empty() || virtual_ip.is_empty() || access.len() != 8 {
        bail!("ENDPOINT_SERIAL, ENDPOINT_VIRTUAL_IP, and PROXY_ACCESS_CODE are required")
    }
    let staging = PathBuf::from(env_value("ENDPOINT_STAGING_ROOT", "/staging")).join(&serial);
    fs::create_dir_all(&staging)?;
    let common = vec![
        ("FTPS_ACCESS_CODE", access.clone()),
        (
            "FTPS_CERT_FILE",
            env_value("ENDPOINT_CERT_FILE", "/secrets/certs/server.crt"),
        ),
        (
            "FTPS_KEY_FILE",
            env_value("ENDPOINT_KEY_FILE", "/secrets/certs/server.key"),
        ),
        ("FTPS_ROOT", staging.display().to_string()),
        ("FTPS_REMOTE_SERIAL", serial.clone()),
        (
            "FTPS_REMOTE_API_URL",
            env_value(
                "ENDPOINT_STORAGE_API_URL",
                "http://127.0.0.1:4844/internal/cloud-lan-proxy/storage",
            ),
        ),
        (
            "FTPS_REMOTE_CACHE_DIR",
            env_value("ENDPOINT_REMOTE_CACHE_DIR", "/remote-cache"),
        ),
        ("PROXY_ACCESS_CODE", access),
    ];
    let mut children = Vec::new();
    let mut environment = common.clone();
    environment.extend([
        ("TCP_RELAY_LISTEN_HOST", virtual_ip.clone()),
        ("TCP_RELAY_LISTEN_PORT", "8883".into()),
        (
            "TCP_RELAY_UPSTREAM_HOST",
            env_value("ENDPOINT_BROKER_HOST", "127.0.0.1"),
        ),
        (
            "TCP_RELAY_UPSTREAM_PORT",
            env_value("ENDPOINT_TLS_BROKER_PORT", "18883"),
        ),
    ]);
    children.push(child("tcp-relay", &environment)?);

    let mut environment = common.clone();
    environment.extend([
        ("FTPS_BIND_HOST", virtual_ip.clone()),
        ("FTPS_ADVERTISE_IP", virtual_ip.clone()),
        ("FTPS_PORT", "990".into()),
    ]);
    children.push(child("ftps", &environment)?);

    let mut environment = common.clone();
    environment.extend([
        ("NATIVE_FILES_BIND_HOST", virtual_ip.clone()),
        ("NATIVE_FILES_PORT", "6000".into()),
    ]);
    children.push(child("native-files", &environment)?);

    let mut environment = common.clone();
    environment.extend([
        ("DISCOVERY_PRINTER_IP", virtual_ip.clone()),
        ("DISCOVERY_PRINTER_SERIAL", serial.clone()),
        (
            "DISCOVERY_PRINTER_MODEL",
            env_value("ENDPOINT_MODEL", "Bambu Lab Printer"),
        ),
        (
            "DISCOVERY_PRINTER_NAME",
            env_value("ENDPOINT_NAME", "Bambu Cloud LAN Proxy"),
        ),
    ]);
    children.push(child("discovery", &environment)?);

    if !physical_ip.is_empty() && env_bool("ENDPOINT_PHYSICAL_ACCESS_CONFIGURED", false)? {
        let mut environment = common.clone();
        environment.extend([
            (
                "CAMERA_GATEWAY_REGISTRY",
                env_value("ENDPOINT_REGISTRY", "/data/printers.json"),
            ),
            ("CAMERA_PRINTER_SERIAL", serial.clone()),
            ("CAMERA_UPSTREAM_IP", physical_ip.clone()),
            ("CAMERA_LISTEN_IP", virtual_ip.clone()),
            ("CAMERA_LISTEN_PORT", "322".into()),
            ("CAMERA_CONFIG_PATH", format!("/tmp/mediamtx-{serial}.json")),
            (
                "CAMERA_STATE_DIR",
                env_value("ENDPOINT_CAMERA_STATE_DIR", "/camera-state"),
            ),
        ]);
        children.push(optional_child("camera", &environment)?);

        if env_bool("ENDPOINT_LOCAL_STATUS", true)? {
            let mut environment = common;
            environment.extend([
                (
                    "LOCAL_STATUS_GATEWAY_REGISTRY",
                    env_value("ENDPOINT_REGISTRY", "/data/printers.json"),
                ),
                ("LOCAL_STATUS_PRINTER_SERIAL", serial.clone()),
                ("LOCAL_STATUS_UPSTREAM_IP", physical_ip),
                (
                    "LOCAL_STATUS_BROKER_HOST",
                    env_value("ENDPOINT_BROKER_HOST", "127.0.0.1"),
                ),
                (
                    "LOCAL_STATUS_BROKER_PORT",
                    env_value("ENDPOINT_BROKER_PORT", "1883"),
                ),
                (
                    "LOCAL_STATUS_STATE_DIR",
                    env_value("ENDPOINT_LOCAL_STATUS_DIR", "/local-status"),
                ),
            ]);
            children.push(optional_child("local-status", &environment)?);
        }
    }

    tracing::info!(%serial, %virtual_ip, services=children.len(), "printer endpoint started");
    loop {
        tokio::select! {
            signal = termination_signal() => {
                signal?;
                stop(&mut children).await;
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                for process in children.iter_mut() {
                    if let Some(status) = process.try_wait()? {
                        stop(&mut children).await;
                        bail!("endpoint child exited with {status}")
                    }
                }
            }
        }
    }
}

pub async fn run_optional() -> Result<()> {
    let command = env_value("ENDPOINT_OPTIONAL_COMMAND", "");
    if !matches!(command.as_str(), "camera" | "local-status") {
        bail!("unsupported optional endpoint service")
    }
    loop {
        let mut process = Command::new(std::env::current_exe()?)
            .arg(&command)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        tokio::select! {
            status = process.wait() => {
                tracing::warn!(service=%command, status=%status?, "optional endpoint service stopped; retrying");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            signal = termination_signal() => {
                signal?;
                let _ = process.kill().await;
                return Ok(());
            }
        }
    }
}

pub async fn run_manager() -> Result<()> {
    let registry = PathBuf::from(env_value("ENDPOINT_REGISTRY", "/data/printers.json"));
    let executable = std::env::current_exe()?;
    let mut active: BTreeMap<String, (Endpoint, Child)> = BTreeMap::new();
    loop {
        let desired = match load(&registry) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, path=%registry.display(), "endpoint registry unavailable");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let stale: Vec<String> = active
            .iter_mut()
            .filter_map(|(serial, (current, child))| {
                let exited = child.try_wait().ok().flatten().is_some();
                (exited || desired.get(serial) != Some(current)).then(|| serial.clone())
            })
            .collect();
        for serial in stale {
            if let Some((_, mut child)) = active.remove(&serial) {
                terminate(&mut child).await;
            }
        }
        for (serial, endpoint) in desired {
            if active.contains_key(&serial) {
                continue;
            }
            let child = Command::new(&executable)
                .arg("endpoint")
                .env("ENDPOINT_SERIAL", &endpoint.serial)
                .env("ENDPOINT_VIRTUAL_IP", &endpoint.virtual_ip)
                .env("ENDPOINT_PHYSICAL_IP", &endpoint.ip)
                .env(
                    "ENDPOINT_PHYSICAL_ACCESS_CONFIGURED",
                    (!endpoint.access_code.is_empty()).to_string(),
                )
                .env("ENDPOINT_MODEL", &endpoint.machine_model)
                .env("ENDPOINT_NAME", &endpoint.name)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            active.insert(serial, (endpoint, child));
        }
        tokio::select! {
            signal = termination_signal() => {
                signal?;
                for (_, (_, mut child)) in active { terminate(&mut child).await; }
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

pub async fn configure_addresses() -> Result<()> {
    let registry = PathBuf::from(env_value("ENDPOINT_REGISTRY", "/data/printers.json"));
    let interface = env_value("ENDPOINT_INTERFACE", "");
    let endpoints = load(&registry)?;
    for endpoint in endpoints.values() {
        let address: Ipv4Addr = endpoint.virtual_ip.parse()?;
        if address.is_loopback() {
            continue;
        }
        if interface.is_empty() {
            bail!("ENDPOINT_INTERFACE is required for non-loopback virtual IPs")
        }
        let present = Command::new("ip")
            .args(["address", "show", "to", &format!("{address}/32")])
            .output()
            .await?;
        if !present.status.success() {
            bail!("could not inspect host addresses")
        }
        if !present.stdout.is_empty() {
            tracing::info!(%address, "virtual IP is already configured");
            continue;
        }
        let status = Command::new("ip")
            .args([
                "address",
                "add",
                &format!("{address}/32"),
                "dev",
                &interface,
            ])
            .status()
            .await?;
        if !status.success() {
            bail!("failed to add {address} to {interface}")
        }
        tracing::info!(%address, %interface, "configured virtual printer IP");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_accepts_distinct_private_endpoints() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("printers.json");
        fs::write(
            &path,
            br#"[{"serial":"SERIALA","virtual_ip":"192.168.10.201"},{"serial":"SERIALB","virtual_ip":"192.168.10.202"}]"#,
        )
        .unwrap();
        let endpoints = load(&path).unwrap();
        assert_eq!(endpoints.len(), 2);
    }

    #[test]
    fn registry_rejects_shared_endpoint_ip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("printers.json");
        fs::write(
            &path,
            br#"[{"serial":"SERIALA","virtual_ip":"192.168.10.201"},{"serial":"SERIALB","virtual_ip":"192.168.10.201"}]"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
    }
}
