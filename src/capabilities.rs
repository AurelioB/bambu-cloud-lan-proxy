use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const TELEMETRY_MAX_AGE: u64 = 90;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraKind {
    Rtsps,
    Image,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    ModelProfile,
    RuntimeTelemetry,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PrinterCapabilities {
    pub camera: CameraKind,
    pub local_camera_bridge: bool,
    pub chamber_temperature: bool,
    pub dual_nozzle: bool,
    pub source: CapabilitySource,
    pub model_known: bool,
    pub observed_fields: Vec<String>,
    pub runtime: Option<ObservedCapabilities>,
}

/// Only capability evidence is retained, never the full report, filament tags,
/// module serials, or credentials. Omitted delta fields preserve prior evidence.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ObservedCapabilities {
    pub updated_at: u64,
    pub firmware: Option<String>,
    pub ams_units: Option<usize>,
    pub filament_slots: Option<usize>,
    pub ams: BTreeMap<String, BTreeSet<String>>,
    pub sensors: BTreeSet<String>,
    pub firmware_controls: BTreeMap<String, bool>,
    pub dual_nozzle: Option<bool>,
    pub chamber_temperature: Option<bool>,
}

fn hex_bits(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_str()
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
    })
}

fn id(value: &Value) -> Option<String> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .filter(|value| *value <= 255)
        .map(|v| v.to_string())
}

impl ObservedCapabilities {
    pub fn prefer(&mut self, local: &Self) {
        self.updated_at = self.updated_at.max(local.updated_at);
        if local.firmware.is_some() {
            if self.firmware != local.firmware {
                self.firmware_controls.clear();
            }
            self.firmware.clone_from(&local.firmware);
        }
        if local.ams_units.is_some() {
            self.ams_units = local.ams_units;
            self.filament_slots = local.filament_slots;
            self.ams.clone_from(&local.ams);
        }
        if local.dual_nozzle.is_some() {
            self.dual_nozzle = local.dual_nozzle;
        }
        if local.chamber_temperature.is_some() {
            self.chamber_temperature = local.chamber_temperature;
        }
        self.sensors.extend(local.sensors.iter().cloned());
        self.firmware_controls.extend(
            local
                .firmware_controls
                .iter()
                .map(|(key, value)| (key.clone(), *value)),
        );
    }

    pub fn observe(&mut self, report: &Value, now: u64) {
        if let Some(modules) = report.pointer("/info/module").and_then(Value::as_array)
            && let Some(firmware) = modules
                .iter()
                .find(|m| m["name"] == "ota")
                .and_then(|m| m["sw_ver"].as_str())
                .filter(|v| !v.is_empty() && v.len() <= 64)
        {
            // A firmware change invalidates previously reported function flags.
            if self.firmware.as_deref() != Some(firmware) {
                self.firmware_controls.clear();
            }
            self.firmware = Some(firmware.to_owned());
        }
        if let Some(print) = report.get("print").and_then(Value::as_object) {
            if let Some(ams) = print.get("ams").and_then(Value::as_object) {
                let bits = ams.get("ams_exist_bits").and_then(hex_bits);
                if let Some(bits) = bits {
                    self.ams.retain(|key, _| {
                        key.parse::<u32>()
                            .ok()
                            .is_some_and(|n| n >= 64 || bits & (1u64 << n) != 0)
                    });
                    if bits == 0 {
                        self.ams.clear();
                    }
                    self.ams_units = Some(bits.count_ones() as usize);
                }
                if let Some(units) = ams.get("ams").and_then(Value::as_array) {
                    for unit in units.iter().take(32) {
                        let Some(unit_id) = unit.get("id").and_then(id) else {
                            continue;
                        };
                        if bits.is_some_and(|bits| {
                            unit_id
                                .parse::<u32>()
                                .ok()
                                .is_some_and(|n| n < 64 && bits & (1u64 << n) == 0)
                        }) {
                            continue;
                        }
                        if !self.ams.contains_key(&unit_id) && self.ams.len() >= 32 {
                            continue;
                        }
                        let slots = self.ams.entry(unit_id).or_default();
                        if let Some(trays) = unit.get("tray").and_then(Value::as_array) {
                            slots.extend(
                                trays
                                    .iter()
                                    .take(16)
                                    .filter_map(|tray| tray.get("id").and_then(id))
                                    .filter(|slot| slot.parse::<u32>().is_ok_and(|v| v < 16)),
                            );
                        }
                        for (key, sensor) in
                            [("temp", "ams_temperature"), ("humidity", "ams_humidity")]
                        {
                            if unit.get(key).is_some_and(|v| {
                                v.as_f64().is_some()
                                    || v.as_str().is_some_and(|s| s.parse::<f64>().is_ok())
                            }) {
                                self.sensors.insert(sensor.into());
                            }
                        }
                    }
                    if bits.is_none() && units.is_empty() {
                        self.ams.clear();
                        self.ams_units = Some(0);
                    } else {
                        self.ams_units = Some(self.ams_units.unwrap_or(0).max(self.ams.len()));
                    }
                }
                self.filament_slots = self.ams_units.and_then(|count| {
                    (count == self.ams.len() && self.ams.values().all(|slots| !slots.is_empty()))
                        .then(|| self.ams.values().map(BTreeSet::len).sum())
                });
            }
            for (key, sensor) in [
                ("nozzle_temper", "nozzle_temperature"),
                ("bed_temper", "bed_temperature"),
                ("mc_percent", "print_progress"),
                ("wifi_signal", "wifi_signal"),
                ("door_open", "door"),
                ("heatbreak_fan_speed", "heatbreak_fan"),
                ("chamber_fan_speed", "chamber_fan"),
            ] {
                if print
                    .get(key)
                    .is_some_and(|v| v.is_number() || v.is_boolean() || v.is_string())
                {
                    self.sensors.insert(sensor.into());
                }
            }
            let ctc = report
                .pointer("/print/device/ctc/info/temp")
                .and_then(Value::as_u64);
            // Legacy firmware emits a dummy 0/5 chamber temperature on models
            // without a chamber sensor. Those values alone are not evidence.
            let chamber = print
                .get("chamber_temper")
                .and_then(Value::as_f64)
                .is_some_and(|v| v > 5.0 && v <= 150.0);
            if ctc.is_some() || chamber {
                self.chamber_temperature = Some(true);
                self.sensors.insert("chamber_temperature".into());
            }
            for (path, sensor) in [
                ("/print/device/bed/info/temp", "bed_temperature"),
                ("/print/device/extruder/info", "nozzle_temperature"),
            ] {
                if report
                    .pointer(path)
                    .is_some_and(|v| v.is_number() || v.is_array())
                {
                    self.sensors.insert(sensor.into());
                }
            }
            if let Some(extruders) = report
                .pointer("/print/device/extruder/info")
                .and_then(Value::as_array)
            {
                let ids: BTreeSet<_> = extruders
                    .iter()
                    .filter_map(|v| v.get("id").and_then(id))
                    .collect();
                if ids.len() >= 2 {
                    self.dual_nozzle = Some(true);
                }
            }
            if let Some(flag) = print
                .get("home_flag")
                .and_then(|v| v.as_i64().map(|n| n as u32))
            {
                // These firmware-reported support bits are decoded by Studio's
                // MachineObject::parse_home_flag; they are not proxy permissions.
                self.firmware_controls
                    .insert("motor_noise_calibration".into(), flag & (1 << 21) != 0);
                self.firmware_controls
                    .insert("user_presets".into(), flag & (1 << 22) != 0);
            }
            if let Some(fun) = print.get("fun").and_then(hex_bits) {
                self.firmware_controls
                    .insert("local_control".into(), fun & 0x2000_0000 == 0);
            }
        }
        if report.get("print").is_some_and(Value::is_object)
            || report.get("info").is_some_and(Value::is_object)
        {
            self.updated_at = now;
        }
    }

    pub fn fresh(&self, now: u64, max_age: u64) -> bool {
        self.updated_at > 0 && self.updated_at <= now && now - self.updated_at <= max_age
    }

    fn fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        for (observed, name) in [
            (self.firmware.is_some(), "firmware"),
            (self.ams_units.is_some(), "ams_units"),
            (self.filament_slots.is_some(), "filament_slots"),
            (self.dual_nozzle.is_some(), "dual_nozzle"),
            (self.chamber_temperature.is_some(), "chamber_temperature"),
            (!self.sensors.is_empty(), "sensors"),
            (!self.firmware_controls.is_empty(), "firmware_controls"),
        ] {
            if observed {
                fields.push(name.into());
            }
        }
        fields
    }
}

impl PrinterCapabilities {
    pub fn from_model(model: &str) -> Self {
        let normalized: String = model
            .chars()
            .filter(|value| value.is_ascii_alphanumeric())
            .flat_map(char::to_uppercase)
            .collect();
        let profile = [
            "H2DPRO", "X1CARBON", "A1MINI", "H2C", "H2D", "H2S", "P1P", "P1S", "P2S", "X1C", "X1E",
            "X2D", "A2L", "A1", "X1",
        ]
        .into_iter()
        .find(|profile| normalized.ends_with(profile));
        // The account/discovery APIs may return Bambu's internal model code
        // rather than the marketing name. O1C2 is emitted by current H2C
        // firmware and has the same local RTSPS/dual-nozzle capability set.
        let profile = match normalized.as_str() {
            "O1C2" => Some("H2C"),
            "C11" => Some("P1P"),
            "C12" => Some("P1S"),
            "N1" => Some("A1MINI"),
            "N2S" => Some("A1"),
            "N7" => Some("P2S"),
            _ => profile,
        };
        let image_camera = matches!(profile, Some("A1" | "A1MINI" | "A2L" | "P1P" | "P1S"));
        let rtsps_camera = matches!(
            profile,
            Some(
                "P2S"
                    | "X1"
                    | "X1C"
                    | "X1CARBON"
                    | "X1E"
                    | "X2D"
                    | "H2C"
                    | "H2D"
                    | "H2DPRO"
                    | "H2S"
            )
        );
        let chamber_temperature = matches!(
            profile,
            Some(
                "P2S"
                    | "X1"
                    | "X1C"
                    | "X1CARBON"
                    | "X1E"
                    | "X2D"
                    | "H2C"
                    | "H2D"
                    | "H2DPRO"
                    | "H2S"
            )
        );
        let dual_nozzle = matches!(profile, Some("X2D" | "H2C" | "H2D" | "H2DPRO"));
        Self {
            camera: if rtsps_camera {
                CameraKind::Rtsps
            } else if image_camera {
                CameraKind::Image
            } else {
                CameraKind::Unknown
            },
            local_camera_bridge: rtsps_camera || image_camera,
            chamber_temperature,
            dual_nozzle,
            source: if profile.is_some() {
                CapabilitySource::ModelProfile
            } else {
                CapabilitySource::Unknown
            },
            model_known: profile.is_some(),
            observed_fields: Vec::new(),
            runtime: None,
        }
    }

    pub fn with_observations(
        model: &str,
        observed: &ObservedCapabilities,
        now: u64,
        max_age: u64,
    ) -> Self {
        let mut profile = Self::from_model(model);
        if observed.fresh(now, max_age) {
            profile.observed_fields = observed.fields();
            if !profile.observed_fields.is_empty() {
                if let Some(value) = observed.dual_nozzle {
                    profile.dual_nozzle = value;
                }
                if let Some(value) = observed.chamber_temperature {
                    profile.chamber_temperature = value;
                }
                profile.source = CapabilitySource::RuntimeTelemetry;
                profile.runtime = Some(observed.clone());
            }
        }
        profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_camera_and_toolhead_profiles() {
        let compact = PrinterCapabilities::from_model("Bambu Lab A1 mini");
        assert_eq!(compact.camera, CameraKind::Image);
        assert!(compact.local_camera_bridge);
        assert!(!compact.chamber_temperature);

        let dual = PrinterCapabilities::from_model("3DPrinter-H2D-Pro");
        assert_eq!(dual.camera, CameraKind::Rtsps);
        assert!(dual.local_camera_bridge);
        assert!(dual.chamber_temperature);
        assert!(dual.dual_nozzle);

        let internal = PrinterCapabilities::from_model("O1C2");
        assert_eq!(internal.camera, CameraKind::Rtsps);
        assert!(internal.local_camera_bridge);
        assert!(internal.dual_nozzle);
    }

    #[test]
    fn leaves_unknown_models_conservative() {
        let unknown = PrinterCapabilities::from_model("Future Printer 9000");
        assert_eq!(unknown.camera, CameraKind::Unknown);
        assert!(!unknown.local_camera_bridge);
        assert_eq!(unknown.source, CapabilitySource::Unknown);
    }

    #[test]
    fn local_observations_override_cloud_counts_without_losing_version_evidence() {
        let mut cloud = ObservedCapabilities::default();
        cloud.observe(&serde_json::json!({"info":{"module":[{"name":"ota","sw_ver":"01.00.00.00"}]},"print":{"ams":{"ams_exist_bits":"1","ams":[{"id":0,"tray":[{"id":0}]}]},"fun":"0"}}), 100);
        let mut local = ObservedCapabilities::default();
        local.observe(
            &serde_json::json!({"print":{"ams":{"ams_exist_bits":"0"},"fun":"20000000"}}),
            101,
        );
        cloud.prefer(&local);
        assert_eq!(cloud.ams_units, Some(0));
        assert_eq!(cloud.filament_slots, Some(0));
        assert_eq!(cloud.firmware.as_deref(), Some("01.00.00.00"));
        assert!(!cloud.firmware_controls["local_control"]);
        let restored: ObservedCapabilities =
            serde_json::from_slice(&serde_json::to_vec(&cloud).unwrap()).unwrap();
        assert_eq!(restored, cloud);
        let capabilities = PrinterCapabilities::with_observations(
            "Future Printer",
            &cloud,
            102,
            TELEMETRY_MAX_AGE,
        );
        assert!(!capabilities.model_known);
        assert!(
            !capabilities
                .observed_fields
                .iter()
                .any(|field| field == "dual_nozzle")
        );
        local.observe(
            &serde_json::json!({"info":{"module":[{"name":"ota","sw_ver":"02.00.00.00"}]}}),
            103,
        );
        cloud.prefer(&local);
        assert!(cloud.firmware_controls.is_empty());
    }

    #[test]
    fn observations_merge_deltas_and_remove_detached_ams() {
        let mut observed = ObservedCapabilities::default();
        observed.observe(&serde_json::json!({"print":{"ams":{"ams_exist_bits":"3","ams":[{"id":"0","tray":[{"id":"0"},{"id":"1"},{"id":"2"},{"id":"3"}]},{"id":"1","tray":[{"id":"0"}]}]},"device":{"extruder":{"info":[{"id":0},{"id":1}]}},"chamber_temper":32,"fun":"20000000","home_flag":1 << 21}}), 100);
        observed.observe(&serde_json::json!({"print":{"ams":{"ams":[{"id":"0","tray":[{"id":"1","tray_type":"PLA"}]}]}}}), 101);
        assert_eq!(observed.ams_units, Some(2));
        assert_eq!(observed.filament_slots, Some(5));
        assert_eq!(observed.firmware_controls["local_control"], false);
        assert_eq!(observed.firmware_controls["motor_noise_calibration"], true);
        let capabilities = PrinterCapabilities::with_observations(
            "Future Printer",
            &observed,
            102,
            TELEMETRY_MAX_AGE,
        );
        assert_eq!(capabilities.source, CapabilitySource::RuntimeTelemetry);
        assert!(capabilities.dual_nozzle);
        assert!(capabilities.chamber_temperature);
        assert!(!capabilities.local_camera_bridge);
        observed.observe(
            &serde_json::json!({"print":{"ams":{"ams_exist_bits":"0","ams":[]}}}),
            103,
        );
        assert_eq!(observed.ams_units, Some(0));
        assert_eq!(observed.filament_slots, Some(0));
        assert!(observed.ams.is_empty());
        assert_eq!(
            PrinterCapabilities::with_observations("A1", &observed, 200, TELEMETRY_MAX_AGE).source,
            CapabilitySource::ModelProfile
        );
    }

    #[test]
    fn firmware_changes_clear_flags_and_placeholder_temperatures_do_not_add_sensors() {
        let mut observed = ObservedCapabilities::default();
        observed.observe(&serde_json::json!({"info":{"module":[{"name":"ota","sw_ver":"01.00.00.00"} ]},"print":{"fun":"0","chamber_temper":5}}), 100);
        assert_eq!(observed.chamber_temperature, None);
        observed.observe(
            &serde_json::json!({"info":{"module":[{"name":"ota","sw_ver":"02.00.00.00"} ]}}),
            101,
        );
        assert!(observed.firmware_controls.is_empty());
        observed.observe(
            &serde_json::json!({"print":{"ams":{"ams_exist_bits":"1"}}}),
            102,
        );
        assert_eq!(observed.ams_units, Some(1));
        assert_eq!(observed.filament_slots, None);
        assert!(!observed.fresh(99, TELEMETRY_MAX_AGE));
        assert_eq!(
            PrinterCapabilities::from_model("P1S").camera,
            CameraKind::Image
        );
    }
}
