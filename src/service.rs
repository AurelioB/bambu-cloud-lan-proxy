use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    bridge::CloudLanFacade,
    cloud_printer::CloudPrinter,
    commands::command,
    config::{ProxyConfig, env_value},
    plugin_install::{ensure, studio_headers},
    plugin_rpc::PluginRpc,
};

pub struct Session {
    data: PathBuf,
    pub region: String,
    pub config: ProxyConfig,
    pub facade: CloudLanFacade,
    pub host: PluginRpc,
    pub printers: RwLock<HashMap<String, CloudPrinter>>,
    pub signed_in: AtomicBool,
    pub connected: AtomicBool,
    connect_lock: Mutex<()>,
    registry_lock: Mutex<()>,
    last_refresh: Mutex<Instant>,
}

impl Session {
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub async fn start(data: PathBuf, region: String, config: ProxyConfig) -> Result<Arc<Self>> {
        let active = data.join("bambu-plugin/active");
        ensure(&active, &region).await?;
        let identity_dir = data.join("bambu-plugin/state");
        fs::create_dir_all(&identity_dir)?;
        let identity = identity_dir.join("device_id");
        if !identity.exists() {
            write_private_atomic(&identity, Uuid::new_v4().to_string().as_bytes())?;
        }
        let executable = env_value("BAMBU_PLUGIN_HOST", "/usr/local/bin/bambu-proxy-native");
        let host = PluginRpc::start(
            &executable,
            &[
                (
                    "BAMBU_NETWORK_SO",
                    active.join("libbambu_networking.so").display().to_string(),
                ),
                (
                    "BAMBU_SOURCE_SO",
                    active.join("libBambuSource.so").display().to_string(),
                ),
                (
                    "BAMBU_PLUGIN_STATE",
                    active.join("state").display().to_string(),
                ),
                ("BAMBU_CLOUD_REGION", region.clone()),
                ("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt".into()),
                ("SSL_CERT_DIR", "/etc/ssl/certs".into()),
                (
                    "CURL_CA_BUNDLE",
                    "/etc/ssl/certs/ca-certificates.crt".into(),
                ),
            ],
        )
        .await?;
        host.call("init_plugin", json!({}), Duration::from_secs(60))
            .await?;
        let mut headers = Map::new();
        for (name, value) in studio_headers()?.iter() {
            if name.as_str() != "user-agent" {
                headers.insert(
                    name.as_str().to_owned(),
                    Value::String(value.to_str()?.to_owned()),
                );
            }
        }
        headers.insert("X-BBL-OS-Version".into(), "6.1.0".into());
        headers.insert("X-BBL-Language".into(), "en-US".into());
        headers.insert(
            "X-BBL-Device-ID".into(),
            fs::read_to_string(identity)?.trim().into(),
        );
        checked(&host, "set_extra_http_header", json!({"headers":headers})).await?;
        let facade = CloudLanFacade::start(config.clone()).await?;
        let session = Arc::new(Self {
            data,
            region,
            config,
            facade,
            host,
            printers: RwLock::new(HashMap::new()),
            signed_in: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            connect_lock: Mutex::new(()),
            registry_lock: Mutex::new(()),
            last_refresh: Mutex::new(Instant::now() - Duration::from_secs(600)),
        });
        let events = session.clone();
        tokio::spawn(async move { events.event_loop().await });
        let monitor = session.clone();
        tokio::spawn(async move { monitor.monitor().await });
        Ok(session)
    }

    pub async fn refresh_devices(&self) -> Result<()> {
        let _registry_guard = self.registry_lock.lock().await;
        let result = checked(&self.host, "get_user_print_info", json!({})).await?;
        let devices = result
            .get("devices")
            .and_then(Value::as_array)
            .context("invalid device discovery response")?;
        let registry_path = self.data.join("printers.json");
        let previous: Vec<Value> = fs::read(&registry_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut registry: BTreeMap<String, Value> = previous
            .into_iter()
            .filter_map(|entry| {
                let serial = entry.get("serial")?.as_str()?.to_ascii_uppercase();
                Some((serial, entry))
            })
            .collect();
        let valid_serial = Regex::new(r"^[A-Z0-9]{5,40}$")?;
        let mut discovered = HashSet::new();
        for device in devices {
            let serial = device
                .get("dev_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_uppercase();
            if !valid_serial.is_match(&serial) || !self.config.permits_printer(&serial) {
                continue;
            }
            discovered.insert(serial.clone());
            if !self.printers.read().await.contains_key(&serial) {
                let printer = CloudPrinter::new(
                    serial.clone(),
                    self.host.clone(),
                    self.data.join("submissions"),
                );
                self.facade.register(printer.clone()).await;
                self.printers.write().await.insert(serial.clone(), printer);
            }
            let entry = registry
                .entry(serial.clone())
                .or_insert_with(|| json!({"serial":serial,"ip":"","access_code":""}));
            entry["name"] = device
                .get("dev_name")
                .cloned()
                .unwrap_or_else(|| Value::String(serial.clone()));
            entry["machine_model"] = device
                .get("dev_model_name")
                .cloned()
                .unwrap_or(Value::String(String::new()));
        }
        let removed: Vec<String> = self
            .printers
            .read()
            .await
            .keys()
            .filter(|serial| !discovered.contains(*serial))
            .cloned()
            .collect();
        for serial in removed {
            if let Some(printer) = self.printers.write().await.remove(&serial) {
                printer.set_connected(false);
                printer.set_secure(false);
            }
            self.facade.remove(&serial).await;
        }
        write_private_atomic(
            &registry_path,
            &serde_json::to_vec_pretty(&registry.into_values().collect::<Vec<_>>())?,
        )?;
        *self.last_refresh.lock().await = Instant::now();
        Ok(())
    }

    pub async fn configure_endpoint(
        &self,
        serial: &str,
        virtual_ip: &str,
        physical_ip: &str,
        physical_access_code: &str,
        enabled: bool,
    ) -> Result<()> {
        use std::net::{IpAddr, Ipv4Addr};

        let serial = serial.trim().to_ascii_uppercase();
        if !self.printers.read().await.contains_key(&serial) {
            bail!("printer is not present in the connected account")
        }
        let virtual_ip = virtual_ip.trim();
        if enabled {
            let address: Ipv4Addr = virtual_ip
                .parse()
                .context("virtual IP must be an IPv4 address")?;
            if !(address.is_private() || address.is_loopback()) {
                bail!("virtual IP must be private or loopback")
            }
        } else if !virtual_ip.is_empty() {
            let _: Ipv4Addr = virtual_ip
                .parse()
                .context("virtual IP must be an IPv4 address")?;
        }
        let physical_ip = physical_ip.trim();
        if !physical_ip.is_empty() {
            let address: IpAddr = physical_ip
                .parse()
                .context("physical IP must be an IP address")?;
            let safe = match address {
                IpAddr::V4(value) => value.is_private() || value.is_loopback(),
                IpAddr::V6(value) => value.is_unique_local() || value.is_loopback(),
            };
            if !safe {
                bail!("physical IP must be private or loopback")
            }
        }
        if !physical_access_code.is_empty()
            && (physical_access_code.len() != 8
                || !physical_access_code
                    .bytes()
                    .all(|v| v.is_ascii_alphanumeric()))
        {
            bail!("physical access code must be eight letters or digits")
        }
        let _guard = self.registry_lock.lock().await;
        let path = self.data.join("printers.json");
        let mut registry: Vec<Value> = serde_json::from_slice(&fs::read(&path)?)?;
        if enabled
            && registry.iter().any(|entry| {
                !entry
                    .get("serial")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .eq_ignore_ascii_case(&serial)
                    && entry.get("endpoint_enabled").and_then(Value::as_bool) != Some(false)
                    && entry.get("virtual_ip").and_then(Value::as_str) == Some(virtual_ip)
            })
        {
            bail!("virtual IP is already assigned to another printer")
        }
        let entry = registry
            .iter_mut()
            .find(|entry| {
                entry
                    .get("serial")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .eq_ignore_ascii_case(&serial)
            })
            .context("printer registry entry is missing")?;
        entry["virtual_ip"] = virtual_ip.into();
        entry["ip"] = physical_ip.into();
        entry["endpoint_enabled"] = enabled.into();
        if !physical_access_code.is_empty() {
            entry["access_code"] = physical_access_code.into();
        }
        write_private_atomic(&path, &serde_json::to_vec_pretty(&registry)?)?;
        Ok(())
    }

    pub async fn connect(&self) -> Result<()> {
        let _guard = self.connect_lock.lock().await;
        self.refresh_devices().await?;
        checked(&self.host, "connect_server", json!({})).await?;
        let mut ready = false;
        for _ in 0..40 {
            ready = checked(&self.host, "is_server_connected", json!({}))
                .await?
                .get("connected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if !ready {
            bail!("cloud connection timed out")
        }
        checked(&self.host, "start_subscribe", json!({"module":"app"})).await?;
        let serials: Vec<String> = self.printers.read().await.keys().cloned().collect();
        for serial in &serials {
            checked(
                &self.host,
                "set_user_selected_machine",
                json!({"dev_id":serial}),
            )
            .await?;
        }
        checked(&self.host, "add_subscribe", json!({"dev_ids":serials})).await?;
        self.connected.store(true, Ordering::Release);
        Ok(())
    }

    async fn snapshot(&self, printer: &CloudPrinter) -> Result<()> {
        for (namespace, name) in [("info", "get_version"), ("pushing", "pushall")] {
            if printer.send(command(namespace, name), 0).await? != 0 {
                bail!("snapshot request rejected")
            }
        }
        Ok(())
    }

    async fn event_loop(self: Arc<Self>) {
        let mut events = self.host.subscribe();
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "cloud event receiver lagged; requesting fresh snapshots"
                    );
                    let printers: Vec<CloudPrinter> =
                        self.printers.read().await.values().cloned().collect();
                    for printer in printers {
                        let _ = self.snapshot(&printer).await;
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    self.connected.store(false, Ordering::Release);
                    break;
                }
            };
            let serial = event
                .get("dev_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_uppercase();
            let Some(printer) = self.printers.read().await.get(&serial).cloned() else {
                continue;
            };
            let result = match event.get("kind").and_then(Value::as_str) {
                Some("OnPrinterConnected") => {
                    async {
                        printer.set_connected(true);
                        checked(&self.host, "install_device_cert", json!({"dev_id":serial}))
                            .await?;
                        self.snapshot(&printer).await
                    }
                    .await
                }
                Some("OnMessage") => {
                    async {
                        let payload = event.get("payload").cloned().unwrap_or(Value::Null);
                        if printer.receive(&payload).await {
                            self.facade.publish_report(&serial, &payload).await?;
                        }
                        Ok(())
                    }
                    .await
                }
                Some("OnUpdateStatus" | "PrintComplete") => {
                    printer.deliver_progress(event).await;
                    Ok(())
                }
                _ => Ok(()),
            };
            if result.is_err() {
                tracing::warn!("cloud event handling failed");
            }
        }
    }

    async fn monitor(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        let mut last_snapshot = Instant::now() - Duration::from_secs(60);
        loop {
            interval.tick().await;
            let result = async {
                let signed_in = checked(&self.host, "is_user_login", json!({}))
                    .await?
                    .get("is_login")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.signed_in.store(signed_in, Ordering::Release);
                let broker = checked(&self.host, "is_server_connected", json!({}))
                    .await?
                    .get("connected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !signed_in || !broker {
                    self.connected.store(false, Ordering::Release);
                    for printer in self.printers.read().await.values() {
                        printer.set_connected(false);
                        printer.set_secure(false);
                    }
                    if signed_in {
                        self.connect().await?;
                    }
                } else if !self.connected.load(Ordering::Acquire) {
                    self.connect().await?;
                }
                if self.connected.load(Ordering::Acquire)
                    && last_snapshot.elapsed() > Duration::from_secs(30)
                {
                    for printer in self.printers.read().await.values() {
                        if printer.status().await.online {
                            self.snapshot(printer).await?;
                        }
                    }
                    last_snapshot = Instant::now();
                }
                if self.connected.load(Ordering::Acquire)
                    && self.last_refresh.lock().await.elapsed() > Duration::from_secs(300)
                {
                    self.connect().await?;
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if result.is_err() {
                self.connected.store(false, Ordering::Release);
                tracing::warn!(
                    "cloud session unavailable; retrying setup without replaying commands"
                );
            }
        }
    }
}

async fn checked(host: &PluginRpc, method: &str, params: Value) -> Result<Value> {
    let result = host.call(method, params, Duration::from_secs(60)).await?;
    if result.get("rc").and_then(Value::as_i64).unwrap_or(0) != 0 {
        bail!("plugin rejected session operation")
    }
    Ok(result)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("invalid state path")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|p| p.to_str()).unwrap_or("state"),
        Uuid::new_v4()
    ));
    fs::write(&temporary, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

pub async fn run() -> Result<()> {
    let data = PathBuf::from(env_value("PROXY_DATA_DIR", "/data"));
    let region = env_value("BAMBU_CLOUD_REGION", "US");
    let mut config = ProxyConfig::from_env()?;
    crate::web::apply_saved_settings(&data, &mut config)?;
    let session = Session::start(data, region, config).await?;
    let result = crate::web::serve(session.clone()).await;
    session.host.stop().await;
    result
}
