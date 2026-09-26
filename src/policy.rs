use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::config::ProxyMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    ForwardQuery,
    ForwardControl,
    ForwardSigned,
    CloudPrint,
    Block,
}

#[derive(Clone, Debug)]
pub struct ParsedRequest {
    pub namespace: String,
    pub command: String,
    pub body: Map<String, Value>,
    pub envelope: Value,
}

impl ParsedRequest {
    pub fn sequence_id(&self) -> Option<String> {
        self.body.get("sequence_id").map(|value| match value {
            Value::String(value) => value.clone(),
            value => value.to_string(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub action: Action,
    pub request: Option<ParsedRequest>,
    pub reason: &'static str,
}

fn parsed(envelope: &Value) -> Option<ParsedRequest> {
    let object = envelope.as_object()?;
    let mut candidates = object.iter().filter_map(|(namespace, body)| {
        let body = body.as_object()?;
        let command = body.get("command")?.as_str()?.trim();
        (!namespace.trim().is_empty() && !command.is_empty()).then(|| ParsedRequest {
            namespace: namespace.clone(),
            command: command.to_owned(),
            body: body.clone(),
            envelope: envelope.clone(),
        })
    });
    let result = candidates.next()?;
    candidates.next().is_none().then_some(result)
}

pub fn decide(
    envelope: &Value,
    mode: ProxyMode,
    allow_cancel: bool,
    allow_cloud_print: bool,
    allow_remote_reprint: bool,
) -> Decision {
    let Some(request) = parsed(envelope) else {
        return Decision {
            action: Action::Block,
            request: None,
            reason: "malformed or ambiguous command envelope",
        };
    };
    let key = (request.namespace.as_str(), request.command.as_str());
    let (action, reason) = match key {
        ("pushing", "pushall")
        | ("info", "get_version")
        | ("system", "get_accessories")
        | ("print", "extrusion_cali_get") => (Action::ForwardQuery, "read-only query"),
        ("print", "project_file") => {
            let url = request
                .body
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("");
            if url.starts_with("file:///")
                && allow_remote_reprint
                && matches!(mode, ProxyMode::FullControl | ProxyMode::Unsafe)
            {
                (
                    Action::ForwardSigned,
                    "printer-resident reprint explicitly enabled",
                )
            } else if allow_cloud_print {
                (Action::CloudPrint, "cloud print explicitly enabled")
            } else {
                (Action::Block, "cloud print is disabled")
            }
        }
        _ if mode == ProxyMode::ReadOnly => (Action::Block, "proxy is in read-only mode"),
        ("print", "pause" | "resume" | "print_speed") | ("system", "ledctrl") => {
            (Action::ForwardControl, "allowlisted control")
        }
        ("print", "stop") if allow_cancel => (Action::ForwardControl, "cancel explicitly enabled"),
        ("print", "stop") => (Action::Block, "cancel is disabled"),
        _ if mode == ProxyMode::FullControl => {
            (Action::ForwardSigned, "full-control signed passthrough")
        }
        _ if mode == ProxyMode::Unsafe => (Action::ForwardSigned, "unsafe passthrough mode"),
        _ => (Action::Block, "command is not on the controls allowlist"),
    };
    Decision {
        action,
        request: Some(request),
        reason,
    }
}

pub fn rejection(decision: &Decision) -> Option<Value> {
    let request = decision.request.as_ref()?;
    let mut body = json!({
        "command": request.command,
        "result": "failed",
        "reason": format!("blocked by cloud LAN proxy: {}", decision.reason),
    });
    if let Some(sequence) = request.sequence_id() {
        body["sequence_id"] = Value::String(sequence);
    }
    Some(json!({ request.namespace.clone(): body }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_fails_closed_and_preserves_queries() {
        let query = json!({"pushing":{"command":"pushall","sequence_id":"7"}});
        assert_eq!(
            decide(&query, ProxyMode::ReadOnly, false, false, false).action,
            Action::ForwardQuery
        );
        let pause = json!({"print":{"command":"pause","sequence_id":"8"}});
        let blocked = decide(&pause, ProxyMode::ReadOnly, false, false, false);
        assert_eq!(blocked.action, Action::Block);
        assert_eq!(rejection(&blocked).unwrap()["print"]["sequence_id"], "8");
        assert_eq!(
            decide(&pause, ProxyMode::Controls, false, false, false).action,
            Action::ForwardControl
        );
        assert_eq!(
            decide(
                &json!({"a":{"command":"x"},"b":{"command":"y"}}),
                ProxyMode::Unsafe,
                true,
                true,
                true
            )
            .action,
            Action::Block
        );
    }
}
