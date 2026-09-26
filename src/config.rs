use std::{collections::BTreeSet, env, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    ReadOnly,
    Controls,
    FullControl,
    Unsafe,
}

impl FromStr for ProxyMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "read_only" => Ok(Self::ReadOnly),
            "controls" => Ok(Self::Controls),
            "full_control" => Ok(Self::FullControl),
            "unsafe" => Ok(Self::Unsafe),
            _ => bail!("CLOUD_LAN_PROXY_MODE must be read_only, controls, full_control, or unsafe"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub broker_host: String,
    pub broker_port: u16,
    pub mode: ProxyMode,
    pub allow_cancel: bool,
    pub cloud_print_enabled: bool,
    pub remote_reprint_enabled: bool,
    pub cloud_print_wait: Duration,
    pub cloud_print_progress_timeout: Duration,
    pub cloud_print_max_bytes: u64,
    pub staging_dir: PathBuf,
    pub cloud_print_job_dir: PathBuf,
    pub printers: BTreeSet<String>,
    pub advertise_controls: bool,
}

impl ProxyConfig {
    pub fn from_env() -> Result<Self> {
        let mode = env_value("CLOUD_LAN_PROXY_MODE", "read_only").parse()?;
        let cloud_print_enabled = env_bool("CLOUD_LAN_PROXY_CLOUD_PRINT_ENABLED", false)?;
        let advertise_controls = match env::var("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS") {
            Ok(value) if value.trim().eq_ignore_ascii_case("auto") => {
                mode != ProxyMode::ReadOnly || cloud_print_enabled
            }
            Ok(_) => env_bool("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", false)?,
            Err(_) => mode != ProxyMode::ReadOnly || cloud_print_enabled,
        };
        let wait = positive_f64("CLOUD_LAN_PROXY_CLOUD_PRINT_WAIT_SECONDS", 900.0)?;
        let progress = positive_f64("CLOUD_LAN_PROXY_CLOUD_PRINT_PROGRESS_TIMEOUT", 900.0)?;
        let max_mb = positive_u64("CLOUD_LAN_PROXY_CLOUD_PRINT_MAX_MB", 1024)?;
        let printers = env_value("CLOUD_LAN_PROXY_PRINTERS", "")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_uppercase)
            .collect();
        Ok(Self {
            enabled: env_bool("CLOUD_LAN_PROXY_ENABLED", true)?,
            broker_host: env_value("CLOUD_LAN_PROXY_BROKER_HOST", "mqtt"),
            broker_port: env_value("CLOUD_LAN_PROXY_BROKER_PORT", "1883")
                .parse()
                .context("CLOUD_LAN_PROXY_BROKER_PORT must be between 1 and 65535")?,
            mode,
            allow_cancel: env_bool("CLOUD_LAN_PROXY_ALLOW_CANCEL", false)?,
            cloud_print_enabled,
            remote_reprint_enabled: env_bool("CLOUD_LAN_PROXY_REMOTE_REPRINT_ENABLED", false)?,
            cloud_print_wait: Duration::from_secs_f64(wait),
            cloud_print_progress_timeout: Duration::from_secs_f64(progress),
            cloud_print_max_bytes: max_mb * 1024 * 1024,
            staging_dir: env_value("CLOUD_LAN_PROXY_STAGING_DIR", "/staging").into(),
            cloud_print_job_dir: env_value(
                "CLOUD_LAN_PROXY_CLOUD_PRINT_JOB_DIR",
                "/data/cloud-lan-proxy-jobs",
            )
            .into(),
            printers,
            advertise_controls,
        })
    }

    pub fn permits_printer(&self, serial: &str) -> bool {
        self.printers.is_empty() || self.printers.contains(&serial.to_ascii_uppercase())
    }
}

pub fn env_value(name: &str, default: &str) -> String {
    env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .trim()
        .to_owned()
}

pub fn env_bool(name: &str, default: bool) -> Result<bool> {
    let Ok(value) = env::var(name) else {
        return Ok(default);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{name} must be true or false"),
    }
}

fn positive_f64(name: &str, default: f64) -> Result<f64> {
    let value = env_value(name, &default.to_string()).parse::<f64>()?;
    if !value.is_finite() || value <= 0.0 {
        bail!("{name} must be positive")
    }
    Ok(value)
}

fn positive_u64(name: &str, default: u64) -> Result<u64> {
    let value = env_value(name, &default.to_string()).parse::<u64>()?;
    if value == 0 {
        bail!("{name} must be positive")
    }
    Ok(value)
}
