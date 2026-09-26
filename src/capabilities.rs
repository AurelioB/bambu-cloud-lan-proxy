use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PrinterCapabilities {
    pub camera: CameraKind,
    pub local_camera_bridge: bool,
    pub chamber_temperature: bool,
    pub dual_nozzle: bool,
    pub source: CapabilitySource,
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
        let profile = if normalized == "O1C2" {
            Some("H2C")
        } else {
            profile
        };
        let image_camera = matches!(profile, Some("A1" | "A1MINI" | "A2L" | "P1P"));
        let rtsps_camera = matches!(
            profile,
            Some(
                "P1S"
                    | "P2S"
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
            local_camera_bridge: rtsps_camera,
            chamber_temperature,
            dual_nozzle,
            source: if profile.is_some() {
                CapabilitySource::ModelProfile
            } else {
                CapabilitySource::Unknown
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_camera_and_toolhead_profiles() {
        let compact = PrinterCapabilities::from_model("Bambu Lab A1 mini");
        assert_eq!(compact.camera, CameraKind::Image);
        assert!(!compact.local_camera_bridge);
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
}
