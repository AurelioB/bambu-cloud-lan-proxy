use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Result, bail};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};

use crate::{
    cloud_printer::CloudPrinter,
    commands::translate_control,
    config::ProxyConfig,
    policy::{Action, Decision, decide, rejection},
    print_jobs::{self, CloudPrintRequest},
    reports::prepare_report,
};

#[derive(Clone)]
pub struct CloudLanFacade {
    inner: Arc<Inner>,
}
struct Inner {
    config: RwLock<ProxyConfig>,
    mqtt: AsyncClient,
    printers: RwLock<HashMap<String, CloudPrinter>>,
    recent: Mutex<HashMap<String, Instant>>,
    print_jobs: Mutex<HashMap<String, PrintJobRecord>>,
    active_prints: Mutex<HashSet<String>>,
    local_state_dir: PathBuf,
    local_fresh: Duration,
}
struct PrintJobRecord {
    seen: Instant,
    request: CloudPrintRequest,
    error: Option<String>,
}

impl CloudLanFacade {
    pub async fn start(config: ProxyConfig) -> Result<Self> {
        let mut options = MqttOptions::new(
            "bambu-cloud-lan-proxy",
            &config.broker_host,
            config.broker_port,
        );
        options.set_keep_alive(Duration::from_secs(30));
        options.set_max_packet_size(16 * 1024 * 1024, 16 * 1024 * 1024);
        let (mqtt, mut event_loop) = AsyncClient::new(options, 1000);
        mqtt.subscribe("device/+/request", QoS::AtLeastOnce).await?;
        let facade = Self {
            inner: Arc::new(Inner {
                config: RwLock::new(config),
                mqtt,
                printers: RwLock::new(HashMap::new()),
                recent: Mutex::new(HashMap::new()),
                print_jobs: Mutex::new(HashMap::new()),
                active_prints: Mutex::new(HashSet::new()),
                local_state_dir: std::env::var("CLOUD_LAN_PROXY_LOCAL_STATUS_STATE_DIR")
                    .unwrap_or_else(|_| "/local-status".into())
                    .into(),
                local_fresh: Duration::from_secs_f64(
                    std::env::var("CLOUD_LAN_PROXY_LOCAL_STATUS_FRESH_SECONDS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(20.0),
                ),
            }),
        };
        let receiver = facade.clone();
        tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(message))) => {
                        if let Err(error) = receiver
                            .handle_request(&message.topic, &message.payload)
                            .await
                        {
                            tracing::warn!(error = %error, "local MQTT request rejected");
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "local MQTT unavailable");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        });
        Ok(facade)
    }

    pub async fn register(&self, printer: CloudPrinter) {
        let config = self.inner.config.read().await;
        if !config.enabled || !config.permits_printer(printer.serial()) {
            return;
        }
        self.inner
            .printers
            .write()
            .await
            .insert(printer.serial().to_owned(), printer);
    }

    pub async fn config(&self) -> ProxyConfig {
        self.inner.config.read().await.clone()
    }

    pub async fn update_policy(
        &self,
        mode: crate::config::ProxyMode,
        allow_cancel: bool,
        cloud_print_enabled: bool,
        remote_reprint_enabled: bool,
        advertise_controls: bool,
    ) {
        let mut config = self.inner.config.write().await;
        config.mode = mode;
        config.allow_cancel = allow_cancel;
        config.cloud_print_enabled = cloud_print_enabled;
        config.remote_reprint_enabled = remote_reprint_enabled;
        config.advertise_controls = advertise_controls;
    }

    pub async fn remove(&self, serial: &str) {
        self.inner.printers.write().await.remove(serial);
    }

    pub async fn publish_report(&self, serial: &str, raw: &Value) -> Result<()> {
        let serial = serial.to_ascii_uppercase();
        if !self.inner.printers.read().await.contains_key(&serial) {
            return Ok(());
        }
        let heartbeat = self.inner.local_state_dir.join(format!("{serial}.local"));
        if let Ok(modified) = heartbeat
            .metadata()
            .and_then(|metadata| metadata.modified())
            && SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default()
                < self.inner.local_fresh
        {
            return Ok(());
        }
        let raw = if let Some(text) = raw.as_str() {
            text.as_bytes().to_vec()
        } else {
            serde_json::to_vec(raw)?
        };
        let advertise_controls = self.inner.config.read().await.advertise_controls;
        let payload = prepare_report(&raw, advertise_controls)?;
        self.inner
            .mqtt
            .publish(
                format!("device/{serial}/report"),
                QoS::AtMostOnce,
                false,
                payload,
            )
            .await?;
        Ok(())
    }

    async fn handle_request(&self, topic: &str, payload: &[u8]) -> Result<()> {
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() != 3 || parts[0] != "device" || parts[2] != "request" {
            return Ok(());
        }
        let serial = parts[1].to_ascii_uppercase();
        let printer = self
            .inner
            .printers
            .read()
            .await
            .get(&serial)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("request for unregistered printer"))?;
        let envelope: Value = serde_json::from_slice(payload)?;
        let config = self.inner.config.read().await.clone();
        let decision = decide(
            &envelope,
            config.mode,
            config.allow_cancel,
            config.cloud_print_enabled,
            config.remote_reprint_enabled,
        );
        if decision.action == Action::Block {
            self.publish_rejection(&serial, &decision).await?;
            bail!(decision.reason)
        }
        if decision.action == Action::CloudPrint {
            return self
                .schedule_cloud_print(serial, printer, decision, payload)
                .await;
        }
        if matches!(
            decision.action,
            Action::ForwardControl | Action::ForwardSigned
        ) && self.duplicate(&serial, &decision, payload).await
        {
            return Ok(());
        }
        let request = decision
            .request
            .as_ref()
            .expect("allowed decisions have a request");
        let outgoing = if decision.action == Action::ForwardControl {
            translate_control(&request.namespace, &request.command, &request.body)?
        } else {
            request.envelope.clone()
        };
        if printer.send(outgoing, 1).await? != 0 {
            self.publish_rejection(
                &serial,
                &Decision {
                    action: Action::Block,
                    request: decision.request,
                    reason: "Bambu cloud plugin rejected command",
                },
            )
            .await?;
            bail!("Bambu cloud plugin rejected command")
        }
        Ok(())
    }

    async fn duplicate(&self, serial: &str, decision: &Decision, raw: &[u8]) -> bool {
        let now = Instant::now();
        let mut recent = self.inner.recent.lock().await;
        recent.retain(|_, seen| now.duration_since(*seen) < Duration::from_secs(60));
        let request = decision
            .request
            .as_ref()
            .expect("mutating decision has request");
        let digest = hex::encode(Sha256::digest(raw));
        let key = format!(
            "{serial}:{}:{}:{}:{}",
            request.namespace,
            request.command,
            request.sequence_id().unwrap_or_default(),
            &digest[..16]
        );
        let duplicate = recent.get(&key).is_some_and(|seen| {
            now.duration_since(*seen)
                < if request.sequence_id().is_some() {
                    Duration::from_secs(30)
                } else {
                    Duration::from_secs(1)
                }
        });
        recent.insert(key, now);
        duplicate
    }

    async fn publish_rejection(&self, serial: &str, decision: &Decision) -> Result<()> {
        if let Some(payload) = rejection(decision) {
            self.inner
                .mqtt
                .publish(
                    format!("device/{serial}/report"),
                    QoS::AtMostOnce,
                    false,
                    serde_json::to_vec(&payload)?,
                )
                .await?;
        }
        Ok(())
    }

    async fn publish_json(&self, serial: &str, payload: Value) -> Result<()> {
        self.inner
            .mqtt
            .publish(
                format!("device/{serial}/report"),
                QoS::AtMostOnce,
                false,
                serde_json::to_vec(&payload)?,
            )
            .await?;
        Ok(())
    }

    async fn schedule_cloud_print(
        &self,
        serial: String,
        printer: CloudPrinter,
        decision: Decision,
        raw: &[u8],
    ) -> Result<()> {
        let parsed = decision
            .request
            .as_ref()
            .expect("cloud print has parsed request");
        let request = match print_jobs::parse(&parsed.body) {
            Ok(value) => value,
            Err(error) => {
                let failure = print_jobs::validation_failure(
                    parsed.sequence_id().as_deref().unwrap_or("0"),
                    &error.to_string(),
                );
                return self.publish_json(&serial, failure).await;
            }
        };
        let key = hex::encode(Sha256::digest([serial.as_bytes(), &[0], raw].concat()));
        {
            let mut jobs = self.inner.print_jobs.lock().await;
            jobs.retain(|_, job| {
                job.error.is_none() || job.seen.elapsed() < Duration::from_secs(3600)
            });
            if let Some(job) = jobs.get(&key) {
                let response = job
                    .error
                    .as_ref()
                    .map(|reason| print_jobs::failure(&job.request, reason))
                    .unwrap_or_else(|| print_jobs::ack(&job.request));
                return self.publish_json(&serial, response).await;
            }
        }
        let status = printer.status().await;
        let state_error = if !status.online {
            Some("printer is offline".to_owned())
        } else if !status.secure_control {
            Some("printer certificate is not ready".to_owned())
        } else if !matches!(status.state.as_str(), "idle" | "finished" | "cancelled") {
            Some(format!("printer is busy ({})", status.state))
        } else if self.inner.active_prints.lock().await.contains(&serial) {
            Some("another cloud print submission is pending".to_owned())
        } else {
            None
        };
        if let Some(reason) = state_error {
            self.inner.print_jobs.lock().await.insert(
                key,
                PrintJobRecord {
                    seen: Instant::now(),
                    request: request.clone(),
                    error: Some(reason.clone()),
                },
            );
            return self
                .publish_json(&serial, print_jobs::failure(&request, &reason))
                .await;
        }
        self.inner.print_jobs.lock().await.insert(
            key.clone(),
            PrintJobRecord {
                seen: Instant::now(),
                request: request.clone(),
                error: None,
            },
        );
        self.inner.active_prints.lock().await.insert(serial.clone());
        self.publish_json(&serial, print_jobs::ack(&request))
            .await?;
        let facade = self.clone();
        tokio::spawn(async move { facade.run_cloud_print(serial, printer, key, request).await });
        Ok(())
    }

    async fn run_cloud_print(
        &self,
        serial: String,
        printer: CloudPrinter,
        key: String,
        request: CloudPrintRequest,
    ) {
        let config = self.inner.config.read().await.clone();
        let mut snapshot: Option<PathBuf> = None;
        let result = async {
            let per_printer = config.staging_dir.join(&serial);
            let staging = if per_printer.is_dir() {
                &per_printer
            } else {
                &config.staging_dir
            };
            let source = wait_for_staged(
                staging,
                &request.relative_path,
                config.cloud_print_wait,
                config.cloud_print_max_bytes,
            )
            .await?;
            let target = snapshot_file(
                &source,
                &config.cloud_print_job_dir,
                &request.filename,
                config.cloud_print_max_bytes,
            )
            .await?;
            snapshot = Some(target.clone());
            let mut progress = printer
                .submit_print(
                    print_jobs::params(&request, &serial, &target),
                    config.cloud_print_progress_timeout,
                )
                .await?;
            while let Some(frame) = progress.recv().await {
                let kind = frame
                    .get("kind")
                    .and_then(Value::as_str)
                    .or_else(|| frame.get("event").and_then(Value::as_str));
                if kind == Some("error")
                    || frame
                        .get("rc")
                        .and_then(Value::as_i64)
                        .is_some_and(|v| v != 0)
                {
                    bail!(
                        "{}",
                        frame
                            .get("msg")
                            .and_then(Value::as_str)
                            .unwrap_or("cloud submission failed")
                    )
                }
                if matches!(kind, Some("done" | "PrintComplete")) {
                    printer.complete_submission(true).await;
                    return Ok::<_, anyhow::Error>(());
                }
            }
            bail!("cloud submission ended without a terminal frame")
        }
        .await;
        let error = result
            .as_ref()
            .err()
            .map(|e| e.to_string().chars().take(300).collect::<String>());
        if let Some(reason) = &error {
            printer.complete_submission(false).await;
            let _ = self
                .publish_json(&serial, print_jobs::failure(&request, reason))
                .await;
        } else if let Some(path) = snapshot {
            let _ = fs::remove_file(path);
        }
        self.inner.print_jobs.lock().await.insert(
            key,
            PrintJobRecord {
                seen: Instant::now(),
                request,
                error,
            },
        );
        self.inner.active_prints.lock().await.remove(&serial);
    }
}

async fn wait_for_staged(
    root: &Path,
    relative: &Path,
    wait: Duration,
    max_bytes: u64,
) -> Result<PathBuf> {
    let root = tokio::fs::canonicalize(root).await?;
    let source = root.join(relative);
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        match tokio::fs::canonicalize(&source).await {
            Ok(resolved) => {
                if !resolved.starts_with(&root) {
                    bail!("staged file path is unsafe")
                }
                let metadata = tokio::fs::metadata(&resolved).await?;
                if !metadata.is_file() || metadata.len() == 0 {
                    bail!("staged file is empty or not a regular file")
                }
                if metadata.len() > max_bytes {
                    bail!("staged file exceeds the cloud print size limit")
                }
                return Ok(resolved);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(250)).await
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn snapshot_file(
    source: &Path,
    directory: &Path,
    filename: &str,
    max_bytes: u64,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(directory).await?;
    let suffix = Path::new(filename)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("3mf");
    let target = directory.join(format!(
        "cloud-lan-proxy-{}.{}",
        uuid::Uuid::new_v4(),
        suffix
    ));
    tokio::fs::copy(source, &target).await?;
    if tokio::fs::metadata(&target).await?.len() > max_bytes {
        let _ = tokio::fs::remove_file(&target).await;
        bail!("staged file exceeds the cloud print size limit")
    }
    let checked = target.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || -> Result<()> {
        let file = fs::File::open(checked)?;
        let mut zip = zip::ZipArchive::new(file)?;
        for i in 0..zip.len() {
            let _ = zip.by_index(i)?;
        }
        Ok(())
    })
    .await?
    {
        let _ = tokio::fs::remove_file(&target).await;
        return Err(error);
    }
    Ok(target)
}
