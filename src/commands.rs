use std::sync::atomic::{AtomicU16, Ordering};

use anyhow::{Result, bail};
use serde_json::{Value, json};

static SEQUENCE: AtomicU16 = AtomicU16::new(20_000);

fn sequence() -> String {
    let old = SEQUENCE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(if value >= 29_999 { 20_000 } else { value + 1 })
        })
        .unwrap_or(20_000);
    old.to_string()
}

pub fn command(namespace: &str, name: &str) -> Value {
    json!({ namespace: { "command": name, "sequence_id": sequence() } })
}

pub fn translate_control(
    namespace: &str,
    name: &str,
    body: &serde_json::Map<String, Value>,
) -> Result<Value> {
    match (namespace, name) {
        ("print", "pause" | "resume" | "stop") => Ok(command("print", name)),
        ("print", "print_speed") => {
            let level = body
                .get("param")
                .and_then(Value::as_str)
                .or_else(|| body.get("print_speed").and_then(Value::as_str))
                .ok_or_else(|| anyhow::anyhow!("missing print speed"))?;
            Ok(json!({"print":{"command":"print_speed","param":level,"sequence_id":sequence()}}))
        }
        ("system", "ledctrl") => {
            let mode = body
                .get("led_mode")
                .and_then(Value::as_str)
                .unwrap_or("off");
            if !matches!(mode, "on" | "off") {
                bail!("invalid chamber light mode")
            }
            Ok(
                json!({"system":{"command":"ledctrl","led_node":"chamber_light","led_mode":mode,"sequence_id":sequence()}}),
            )
        }
        _ => bail!("unsupported allowlisted control"),
    }
}
