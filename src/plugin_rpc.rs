use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, RwLock, broadcast, oneshot},
    time,
};

#[derive(Clone)]
pub struct PluginRpc {
    inner: Arc<Inner>,
}

struct Inner {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Option<Child>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>,
    events: broadcast::Sender<Value>,
    sequence: AtomicU64,
    failure: RwLock<Option<&'static str>>,
}

impl PluginRpc {
    pub async fn start(executable: &str, environment: &[(&str, String)]) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in environment {
            command.env(name, value);
        }
        let mut child = command.spawn().context("start native plugin adapter")?;
        let stdin = child
            .stdin
            .take()
            .context("native adapter stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("native adapter stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("native adapter stderr unavailable")?;
        let (events, _) = broadcast::channel(2048);
        let rpc = Self {
            inner: Arc::new(Inner {
                stdin: Mutex::new(stdin),
                child: Mutex::new(Some(child)),
                pending: Mutex::new(HashMap::new()),
                events,
                sequence: AtomicU64::new(0),
                failure: RwLock::new(None),
            }),
        };
        let receiver = rpc.clone();
        tokio::spawn(async move { receiver.receive(BufReader::new(stdout)).await });
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // The proprietary plugin also writes to this stream. Only
                // surface diagnostics produced by our adapter so vendor logs
                // cannot accidentally disclose session material.
                if line.starts_with("native operation ") {
                    tracing::warn!("{line}");
                }
            }
        });
        Ok(rpc)
    }

    async fn receive(&self, mut stdout: BufReader<tokio::process::ChildStdout>) {
        let mut line = String::new();
        loop {
            line.clear();
            let read = stdout.read_line(&mut line).await;
            let Ok(count) = read else { break };
            if count == 0 {
                break;
            }
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                break;
            };
            let Some(object) = frame.as_object() else {
                break;
            };
            if let Some(event) = object.get("event") {
                if self.inner.events.send(event.clone()).is_err() {
                    // No receiver during initial startup is valid; later calls remain fail closed.
                }
                continue;
            }
            let Some(id) = object.get("id").and_then(Value::as_u64) else {
                break;
            };
            if let Some(sender) = self.inner.pending.lock().await.remove(&id) {
                let response = if object.contains_key("error") {
                    Err(anyhow!("native operation failed"))
                } else {
                    Ok(object.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = sender.send(response);
            }
        }
        *self.inner.failure.write().await = Some("native transport closed");
        let mut pending = self.inner.pending.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(anyhow!("native transport closed")));
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.inner.events.subscribe()
    }

    pub async fn failure(&self) -> Option<&'static str> {
        *self.inner.failure.read().await
    }

    pub async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        if let Some(failure) = self.failure().await {
            bail!(failure)
        }
        let (tx, rx) = oneshot::channel();
        let id = self.inner.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut pending = self.inner.pending.lock().await;
            if pending.len() >= 64 {
                bail!("native request limit reached")
            }
            pending.insert(id, tx);
        }
        let frame = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))?;
        let write_result = async {
            let mut stdin = self.inner.stdin.lock().await;
            stdin.write_all(&frame).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        }
        .await;
        if write_result.is_err() {
            self.inner.pending.lock().await.remove(&id);
            bail!("native transport lost; outcome unknown")
        }
        match time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => bail!("native transport closed; outcome unknown"),
            Err(_) => {
                self.inner.pending.lock().await.remove(&id);
                bail!("native operation timed out; outcome unknown")
            }
        }
    }

    pub async fn stop(&self) {
        let mut child = self.inner.child.lock().await;
        if let Some(mut child) = child.take() {
            let _ = child.start_kill();
            let _ = time::timeout(Duration::from_secs(5), child.wait()).await;
        }
    }
}
