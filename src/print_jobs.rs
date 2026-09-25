use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use regex::Regex;
use serde_json::{Map, Value, json};
use url::Url;

#[derive(Clone, Debug)]
pub struct CloudPrintRequest {
    pub sequence_id: String,
    pub relative_path: PathBuf,
    pub filename: String,
    pub project_name: String,
    pub plate_index: u16,
    pub param: String,
    pub ams_mapping: Option<Vec<i16>>,
    pub ams_mapping2: Option<Vec<Map<String, Value>>>,
    pub nozzle_mapping: Option<Vec<i16>>,
    pub use_ams: bool,
    pub bed_leveling: bool,
    pub flow_cali: bool,
    pub vibration_cali: bool,
    pub layer_inspect: bool,
    pub timelapse: bool,
    pub bed_type: String,
    pub auto_bed_leveling: i64,
    pub auto_flow_cali: i64,
    pub auto_offset_cali: i64,
    pub extruder_cali_manual_mode: i64,
    pub ext_change_assist: bool,
}

fn bool_value(body: &Map<String, Value>, key: &str, default: bool) -> bool {
    body.get(key)
        .and_then(|v| {
            v.as_bool()
                .or_else(|| v.as_i64().filter(|n| matches!(n, 0 | 1)).map(|n| n == 1))
        })
        .unwrap_or(default)
}

fn small_int(body: &Map<String, Value>, key: &str, default: i64, minimum: i64) -> i64 {
    let parsed = body
        .get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(default);
    if (minimum..=2).contains(&parsed) {
        parsed
    } else {
        default
    }
}

fn int_list(value: Option<&Value>, name: &str) -> Result<Option<Vec<i16>>> {
    let Some(mut value) = value.cloned() else {
        return Ok(None);
    };
    if let Some(text) = value.as_str() {
        value = serde_json::from_str(text).with_context(|| format!("{name} is not valid JSON"))?;
    }
    let values = value
        .as_array()
        .with_context(|| format!("{name} must be an array"))?;
    values
        .iter()
        .map(|item| {
            let parsed = item
                .as_i64()
                .or_else(|| item.as_str()?.parse().ok())
                .with_context(|| format!("{name} contains a non-integer"))?;
            if !(-1..=255).contains(&parsed) {
                bail!("{name} contains an out-of-range tray")
            }
            Ok(parsed as i16)
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

fn mapping2(value: Option<&Value>) -> Result<Option<Vec<Map<String, Value>>>> {
    let Some(mut value) = value.cloned() else {
        return Ok(None);
    };
    if let Some(text) = value.as_str() {
        value = serde_json::from_str(text).context("ams_mapping2 is not valid JSON")?;
    }
    let entries = value.as_array().context("ams_mapping2 must be an array")?;
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let object = entry
            .as_object()
            .context("ams_mapping2 entries must be objects")?;
        let ams = object
            .get("ams_id")
            .and_then(Value::as_i64)
            .context("ams_mapping2 entry is invalid")?;
        let slot = object
            .get("slot_id")
            .and_then(Value::as_i64)
            .context("ams_mapping2 entry is invalid")?;
        if !(0..=255).contains(&ams) || !(0..=255).contains(&slot) {
            bail!("ams_mapping2 entry is out of range")
        }
        result.push(Map::from_iter([
            ("ams_id".into(), ams.into()),
            ("slot_id".into(), slot.into()),
        ]));
    }
    Ok(Some(result))
}

pub fn parse(body: &Map<String, Value>) -> Result<CloudPrintRequest> {
    let candidate = body
        .get("file")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            body.get("url")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .and_then(|raw| {
                    let url = Url::parse(raw).ok()?;
                    if url.scheme() != "ftp" {
                        return None;
                    }
                    let path = url.path().trim_start_matches('/');
                    Some(
                        if path.is_empty() {
                            url.host_str().unwrap_or_default()
                        } else {
                            path
                        }
                        .to_owned(),
                    )
                })
        })
        .context("project_file has no valid file or ftp URL")?;
    let decoded = percent_decode_str(candidate.trim())
        .decode_utf8()
        .context("project_file path is invalid")?
        .replace('\\', "/");
    if decoded.contains('\0') {
        bail!("project_file path is invalid")
    }
    let mut relative = PathBuf::new();
    for component in Path::new(decoded.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part)
                if !part.to_string_lossy().starts_with(".proxy-")
                    && !part.to_string_lossy().starts_with(".upload-") =>
            {
                relative.push(part)
            }
            Component::CurDir => {}
            _ => bail!("project_file path escapes staged storage"),
        }
    }
    let filename = relative
        .file_name()
        .and_then(|v| v.to_str())
        .context("project_file path is empty")?
        .to_owned();
    if !filename.to_ascii_lowercase().ends_with(".3mf") {
        bail!("cloud printing requires a sliced .3mf file")
    }
    let param = body
        .get("param")
        .and_then(Value::as_str)
        .unwrap_or("Metadata/plate_1.gcode")
        .to_owned();
    let plate_re = Regex::new(r"(?i)(?:^|/)plate_(\d+)\.gcode$").unwrap();
    let plate_index: u16 = plate_re
        .captures(&param)
        .and_then(|c| c[1].parse().ok())
        .unwrap_or(1);
    if !(1..=256).contains(&plate_index) {
        bail!("plate index is outside the supported range")
    }
    let mut project_name = filename.clone();
    for suffix in [".gcode.3mf", ".3mf"] {
        if project_name.to_ascii_lowercase().ends_with(suffix) {
            project_name.truncate(project_name.len() - suffix.len());
            break;
        }
    }
    if project_name.is_empty() {
        project_name = "Bambuddy print".into();
    }
    project_name = body
        .get("subtask_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&project_name)
        .to_owned();
    let ams_mapping = int_list(body.get("ams_mapping"), "ams_mapping")?;
    let ams_mapping2 = mapping2(body.get("ams_mapping2"))?;
    if ams_mapping
        .as_ref()
        .zip(ams_mapping2.as_ref())
        .is_some_and(|(a, b)| a.len() != b.len())
    {
        bail!("AMS mapping arrays have different lengths")
    }
    let cfg = body
        .get("cfg")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0);
    Ok(CloudPrintRequest {
        sequence_id: body
            .get("sequence_id")
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_else(|| "0".into()),
        relative_path: relative,
        filename,
        project_name,
        plate_index,
        param,
        ams_mapping,
        ams_mapping2,
        nozzle_mapping: int_list(body.get("nozzle_mapping"), "nozzle_mapping")?,
        use_ams: bool_value(body, "use_ams", true),
        bed_leveling: bool_value(body, "bed_leveling", true),
        flow_cali: bool_value(body, "flow_cali", true),
        vibration_cali: bool_value(body, "vibration_cali", false),
        layer_inspect: bool_value(body, "layer_inspect", false),
        timelapse: bool_value(body, "timelapse", false),
        bed_type: body
            .get("bed_type")
            .and_then(Value::as_str)
            .unwrap_or("auto")
            .to_owned(),
        auto_bed_leveling: small_int(body, "auto_bed_leveling", 2, 0),
        auto_flow_cali: small_int(body, "extrude_cali_flag", 2, 0),
        auto_offset_cali: small_int(body, "nozzle_offset_cali", 0, 0),
        extruder_cali_manual_mode: small_int(body, "extrude_cali_manual_mode", -1, -1),
        ext_change_assist: cfg & 1 != 0,
    })
}

pub fn params(request: &CloudPrintRequest, serial: &str, file: &Path) -> Value {
    let mapping2 = request.ams_mapping2.clone().or_else(|| {
        request.ams_mapping.as_ref().map(|items| {
            items
                .iter()
                .map(|tray| {
                    let (ams, slot) = if *tray < 0 {
                        (255, 255)
                    } else if *tray >= 128 {
                        (*tray as i64, 0)
                    } else {
                        ((*tray / 4) as i64, (*tray % 4) as i64)
                    };
                    Map::from_iter([
                        ("ams_id".into(), ams.into()),
                        ("slot_id".into(), slot.into()),
                    ])
                })
                .collect()
        })
    });
    json!({"dev_id":serial,"task_name":format!("{}_plate_{}",request.project_name,request.plate_index),"project_name":request.project_name,
        "preset_name":"","filename":file,"config_filename":file,"plate_index":request.plate_index,
        "nozzle_mapping":request.nozzle_mapping.as_ref().map(|v| serde_json::to_string(v).unwrap()).unwrap_or_default(),
        "ams_mapping":request.ams_mapping.as_ref().map(|v| serde_json::to_string(v).unwrap()).unwrap_or_default(),
        "ams_mapping2":mapping2.as_ref().map(|v| serde_json::to_string(v).unwrap()).unwrap_or_default(),
        "ams_mapping_info":"","nozzles_info":"","connection_type":"","task_bed_leveling":request.bed_leveling,
        "task_flow_cali":request.flow_cali,"task_vibration_cali":request.vibration_cali,"task_layer_inspect":request.layer_inspect,
        "task_record_timelapse":request.timelapse,"task_use_ams":request.use_ams,"task_bed_type":request.bed_type,
        "auto_bed_leveling":request.auto_bed_leveling,"auto_flow_cali":request.auto_flow_cali,"auto_offset_cali":request.auto_offset_cali,
        "extruder_cali_manual_mode":request.extruder_cali_manual_mode,"task_ext_change_assist":request.ext_change_assist,"dev_ip":"","username":"","password":""})
}

pub fn ack(r: &CloudPrintRequest) -> Value {
    json!({"print":{"command":"project_file","sequence_id":r.sequence_id,"param":r.param,"subtask_name":r.project_name,"gcode_state":"PREPARE","gcode_file":r.filename,"gcode_file_prepare_percent":"0","result":"SUCCESS","msg":0}})
}
pub fn failure(r: &CloudPrintRequest, reason: &str) -> Value {
    json!({"print":{"command":"project_file","sequence_id":r.sequence_id,"gcode_file":r.filename,"result":"FAILED","msg":-1,"reason":format!("cloud LAN proxy: {}",reason)}})
}
pub fn validation_failure(sequence_id: &str, reason: &str) -> Value {
    json!({"print":{"command":"project_file","sequence_id":sequence_id,"result":"FAILED","msg":-1,"reason":format!("cloud LAN proxy: {}",reason)}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escape_and_builds_h2c_params() {
        assert!(parse(json!({"file":"../x.3mf"}).as_object().unwrap()).is_err());
        let r = parse(json!({"file":"rack.gcode.3mf","param":"Metadata/plate_3.gcode","ams_mapping":[0,-1],"cfg":"1"}).as_object().unwrap()).unwrap();
        assert_eq!(r.plate_index, 3);
        assert_eq!(
            params(&r, "SERIAL", Path::new("/tmp/x"))["task_ext_change_assist"],
            true
        );
    }
}
