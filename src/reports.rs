use anyhow::{Result, bail};
use serde_json::{Map, Value};

const DEVELOPER_MODE_DISABLED_BIT: u64 = 0x2000_0000;

fn set_control_capability(object: &mut Map<String, Value>, enabled: bool) {
    let original = object.get("fun");
    let was_number = original.is_some_and(Value::is_number);
    let current = original
        .and_then(|value| match value {
            Value::Number(value) => value.as_u64(),
            Value::String(value) => u64::from_str_radix(value, 16).ok(),
            _ => None,
        })
        .unwrap_or(0);
    let updated = if enabled {
        current & !DEVELOPER_MODE_DISABLED_BIT
    } else {
        current | DEVELOPER_MODE_DISABLED_BIT
    };
    object.insert(
        "fun".into(),
        if was_number {
            updated.into()
        } else {
            Value::String(format!("{updated:X}"))
        },
    );
}

pub fn prepare_report(raw: &[u8], controls: bool) -> Result<Vec<u8>> {
    let mut payload: Value = serde_json::from_slice(raw)?;
    let Some(object) = payload.as_object_mut() else {
        bail!("cloud report must be a JSON object")
    };
    if object.contains_key("fun") {
        set_control_capability(object, controls);
    }
    if let Some(print) = object.get_mut("print").and_then(Value::as_object_mut) {
        set_control_capability(print, controls);
    }
    Ok(serde_json::to_vec(&payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_are_proxy_policy_not_physical_mode() {
        let enabled: Value = serde_json::from_slice(
            &prepare_report(br#"{"print":{"fun":"20000001"}}"#, true).unwrap(),
        )
        .unwrap();
        assert_eq!(enabled["print"]["fun"], "1");
        let disabled: Value =
            serde_json::from_slice(&prepare_report(br#"{"fun":1}"#, false).unwrap()).unwrap();
        assert_eq!(disabled["fun"], 0x20000001_u64);
    }
}
