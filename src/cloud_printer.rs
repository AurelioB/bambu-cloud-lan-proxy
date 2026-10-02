use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::capabilities::ObservedCapabilities;
use crate::plugin_rpc::PluginRpc;

#[derive(Clone, Debug, Serialize)]
pub struct PrinterStatus {
    pub online: bool,
    pub state: String,
    pub secure_control: bool,
}

#[derive(Clone)]
pub struct CloudPrinter {
    inner: Arc<Inner>,
}
struct Inner {
    serial: String,
    host: PluginRpc,
    state: RwLock<(String, Option<Instant>)>,
    connected: AtomicBool,
    secure: AtomicBool,
    uncertain: AtomicBool,
    marker: PathBuf,
    secure_marker: PathBuf,
    progress: Mutex<Option<mpsc::Sender<Value>>>,
    capabilities: RwLock<ObservedCapabilities>,
}

impl CloudPrinter {
    pub fn new(serial: String, host: PluginRpc, safety_dir: PathBuf) -> Self {
        let marker = safety_dir.join(format!("{serial}.pending"));
        let uncertain = marker.exists();
        let secure_marker = safety_dir.join(format!("{serial}.secure"));
        let secure = secure_marker.exists();
        Self {
            inner: Arc::new(Inner {
                serial,
                host,
                state: RwLock::new(("unknown".into(), None)),
                connected: AtomicBool::new(false),
                secure: AtomicBool::new(secure),
                uncertain: AtomicBool::new(uncertain),
                marker,
                secure_marker,
                progress: Mutex::new(None),
                capabilities: RwLock::new(ObservedCapabilities::default()),
            }),
        }
    }
    pub fn serial(&self) -> &str {
        &self.inner.serial
    }
    pub fn set_connected(&self, connected: bool) {
        self.inner.connected.store(connected, Ordering::Release);
    }
    pub fn set_secure(&self, secure: bool) {
        self.inner.secure.store(secure, Ordering::Release);
    }
    pub fn restore_secure(&self) {
        self.set_secure(self.inner.secure_marker.exists());
    }
    fn confirm_secure(&self) {
        self.set_secure(true);
        if let Some(parent) = self.inner.secure_marker.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(
            &self.inner.secure_marker,
            b"Secure cloud control previously confirmed.\n",
        );
    }
    fn invalidate_secure(&self) {
        self.set_secure(false);
        let _ = fs::remove_file(&self.inner.secure_marker);
    }
    pub async fn receive(&self, payload: &Value) -> bool {
        if let Some(signal) = payload.as_str() {
            match signal {
                "device_cert_installed" => {
                    self.confirm_secure();
                    return false;
                }
                "device_cert_uninstalled" | "cert_expired" | "cert_revoked" => {
                    self.invalidate_secure();
                    return false;
                }
                "wait_info" => {
                    self.set_secure(false);
                    return false;
                }
                _ => {}
            }
        }
        let envelope = if let Some(raw) = payload.as_str() {
            serde_json::from_str(raw).ok()
        } else {
            Some(payload.clone())
        };
        let Some(envelope): Option<Value> = envelope else {
            return false;
        };
        self.inner.capabilities.write().await.observe(
            &envelope,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        let Some(state) = envelope
            .pointer("/print/gcode_state")
            .and_then(Value::as_str)
        else {
            return envelope.is_object();
        };
        let state = match state {
            "IDLE" => "idle",
            "FINISH" => "finished",
            "RUNNING" => "running",
            "PAUSE" => "paused",
            "PREPARE" => "preparing",
            _ => "unknown",
        };
        *self.inner.state.write().await = (state.into(), Some(Instant::now()));
        true
    }
    pub async fn status(&self) -> PrinterStatus {
        let (state, seen) = self.inner.state.read().await.clone();
        let fresh = seen.is_some_and(|seen| seen.elapsed() < Duration::from_secs(90));
        PrinterStatus {
            online: self.inner.connected.load(Ordering::Acquire)
                && fresh
                && self.inner.host.failure().await.is_none(),
            state: if self.inner.uncertain.load(Ordering::Acquire) {
                "uncertain".into()
            } else {
                state
            },
            secure_control: self.inner.secure.load(Ordering::Acquire),
        }
    }
    pub async fn observed_capabilities(&self) -> ObservedCapabilities {
        self.inner.capabilities.read().await.clone()
    }
    pub async fn send(&self, envelope: Value, qos: u8) -> Result<i64> {
        let result = self
            .inner
            .host
            .call(
                "send_message",
                json!({"dev_id":self.serial(),"payload":envelope.to_string(),"qos":qos}),
                Duration::from_secs(60),
            )
            .await?;
        Ok(result.get("rc").and_then(Value::as_i64).unwrap_or(-1))
    }
    pub async fn send_control(&self, envelope: Value, qos: u8) -> Result<i64> {
        // Bambu's cloud publish channel is only writable for a few milliseconds
        // after a query. Keep both native calls in one RPC dispatch so an async
        // round trip cannot miss that window.
        let wake = json!({"pushing":{"command":"pushall","sequence_id":"0"}});
        let result = self
            .inner
            .host
            .call(
                "send_burst",
                json!({
                    "dev_id": self.serial(),
                    "payloads": [wake.to_string(), envelope.to_string()],
                    "qos": qos
                }),
                Duration::from_secs(60),
            )
            .await?;
        let results = result
            .get("rcs")
            .and_then(Value::as_array)
            .context("plugin omitted burst results")?;
        if results.len() != 2 {
            bail!("plugin returned an incomplete burst result")
        }
        let rc = results
            .iter()
            .map(|value| value.as_i64().unwrap_or(-1))
            .find(|value| *value != 0)
            .unwrap_or(0);
        if rc == 0 {
            self.confirm_secure();
        }
        Ok(rc)
    }
    pub async fn deliver_progress(&self, event: Value) {
        if let Some(sender) = self.inner.progress.lock().await.as_ref() {
            let _ = sender.try_send(event);
        }
    }
    pub async fn submit_print(
        &self,
        params: Value,
        progress_timeout: Duration,
    ) -> Result<mpsc::Receiver<Value>> {
        let status = self.status().await;
        let mut progress = self.inner.progress.lock().await;
        if progress.is_some() || self.inner.uncertain.load(Ordering::Acquire) {
            bail!("previous print submission active or outcome unknown")
        }
        if !status.online || !status.secure_control {
            bail!("printer control/state is not ready")
        }
        fs::create_dir_all(
            self.inner
                .marker
                .parent()
                .context("invalid safety marker")?,
        )?;
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.inner.marker)?;
        marker.write_all(b"Cloud submission outcome pending; do not automatically retry.\n")?;
        marker.sync_all()?;
        self.inner.uncertain.store(true, Ordering::Release);
        let (tx, rx) = mpsc::channel(256);
        *progress = Some(tx.clone());
        let printer = self.clone();
        tokio::spawn(async move {
            let result = printer
                .inner
                .host
                .call("start_print", params, progress_timeout)
                .await;
            if result
                .as_ref()
                .ok()
                .and_then(|value| value.get("rc"))
                .and_then(Value::as_i64)
                .unwrap_or(-1)
                != 0
            {
                let _ = tx.send(json!({"kind":"PrintComplete","rc":-1})).await;
            }
        });
        Ok(rx)
    }
    pub async fn complete_submission(&self, success: bool) {
        *self.inner.progress.lock().await = None;
        if success {
            self.inner.uncertain.store(false, Ordering::Release);
            let _ = fs::remove_file(&self.inner.marker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, time::SystemTime};

    #[tokio::test]
    async fn timed_out_submission_stays_latched_across_restart() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloud-printer-latch-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let host_script = root.join("hanging-host.sh");
        fs::write(
            &host_script,
            b"#!/bin/sh\nIFS= read -r first\nIFS= read -r second\n",
        )
        .unwrap();
        fs::set_permissions(&host_script, fs::Permissions::from_mode(0o700)).unwrap();

        let host = PluginRpc::start(host_script.to_str().unwrap(), &[])
            .await
            .unwrap();
        let printer = CloudPrinter::new("TESTSERIAL".into(), host.clone(), root.clone());
        printer.set_connected(true);
        printer.set_secure(true);
        printer
            .receive(&json!({"print":{"gcode_state":"IDLE"}}))
            .await;

        let mut events = printer
            .submit_print(json!({}), Duration::from_millis(20))
            .await
            .unwrap();
        let terminal = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(terminal["kind"], "PrintComplete");
        assert!(root.join("TESTSERIAL.pending").is_file());
        assert_eq!(printer.status().await.state, "uncertain");

        printer.complete_submission(false).await;
        let restarted = CloudPrinter::new("TESTSERIAL".into(), host.clone(), root.clone());
        assert_eq!(restarted.status().await.state, "uncertain");
        assert!(
            restarted
                .submit_print(json!({}), Duration::from_millis(20))
                .await
                .is_err()
        );

        restarted.complete_submission(true).await;
        assert!(!root.join("TESTSERIAL.pending").exists());
        host.stop().await;
        let _ = fs::remove_dir_all(root);
    }
}
