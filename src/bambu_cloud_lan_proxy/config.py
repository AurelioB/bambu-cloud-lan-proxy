"""Environment configuration for the embedded MQTT facade."""

from __future__ import annotations

import os
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path


class ProxyMode(StrEnum):
    """How much downstream MQTT traffic may reach Bambu Cloud."""

    READ_ONLY = "read_only"
    CONTROLS = "controls"
    UNSAFE = "unsafe"


def _env_bool(name: str, default: bool) -> bool:
    raw = os.getenv(name)
    if raw is None:
        return default
    normalized = raw.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"{name} must be true or false, got {raw!r}")


@dataclass(frozen=True, slots=True)
class ProxyConfig:
    enabled: bool
    broker_host: str
    broker_port: int
    mode: ProxyMode
    allow_cancel: bool
    cloud_print_enabled: bool
    cloud_print_wait_seconds: float
    cloud_print_progress_timeout: float
    cloud_print_max_bytes: int
    staging_dir: Path
    cloud_print_job_dir: Path
    printers: frozenset[str]
    advertise_controls: bool
    request_qos: int = 1
    report_qos: int = 0

    @classmethod
    def from_env(cls) -> ProxyConfig:
        mode_raw = os.getenv("CLOUD_LAN_PROXY_MODE", ProxyMode.READ_ONLY.value)
        try:
            mode = ProxyMode(mode_raw.strip().lower())
        except ValueError as exc:
            choices = ", ".join(item.value for item in ProxyMode)
            raise ValueError(
                f"CLOUD_LAN_PROXY_MODE must be one of {choices}, got {mode_raw!r}"
            ) from exc

        printer_raw = os.getenv("CLOUD_LAN_PROXY_PRINTERS", "")
        printers = frozenset(
            item.strip().upper() for item in printer_raw.split(",") if item.strip()
        )

        cloud_print_enabled = _env_bool("CLOUD_LAN_PROXY_CLOUD_PRINT_ENABLED", False)

        advertise_raw = os.getenv("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", "auto")
        if advertise_raw.strip().lower() == "auto":
            advertise_controls = mode is not ProxyMode.READ_ONLY or cloud_print_enabled
        else:
            advertise_controls = _env_bool("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", False)

        broker_port = int(os.getenv("CLOUD_LAN_PROXY_BROKER_PORT", "1883"))
        if not 1 <= broker_port <= 65535:
            raise ValueError("CLOUD_LAN_PROXY_BROKER_PORT must be between 1 and 65535")

        cloud_print_wait_seconds = float(
            os.getenv("CLOUD_LAN_PROXY_CLOUD_PRINT_WAIT_SECONDS", "900")
        )
        if cloud_print_wait_seconds <= 0:
            raise ValueError("CLOUD_LAN_PROXY_CLOUD_PRINT_WAIT_SECONDS must be positive")

        cloud_print_progress_timeout = float(
            os.getenv("CLOUD_LAN_PROXY_CLOUD_PRINT_PROGRESS_TIMEOUT", "900")
        )
        if cloud_print_progress_timeout <= 0:
            raise ValueError("CLOUD_LAN_PROXY_CLOUD_PRINT_PROGRESS_TIMEOUT must be positive")

        cloud_print_max_mb = int(os.getenv("CLOUD_LAN_PROXY_CLOUD_PRINT_MAX_MB", "1024"))
        if cloud_print_max_mb <= 0:
            raise ValueError("CLOUD_LAN_PROXY_CLOUD_PRINT_MAX_MB must be positive")

        return cls(
            enabled=_env_bool("CLOUD_LAN_PROXY_ENABLED", True),
            broker_host=os.getenv("CLOUD_LAN_PROXY_BROKER_HOST", "mqtt").strip(),
            broker_port=broker_port,
            mode=mode,
            allow_cancel=_env_bool("CLOUD_LAN_PROXY_ALLOW_CANCEL", False),
            cloud_print_enabled=cloud_print_enabled,
            cloud_print_wait_seconds=cloud_print_wait_seconds,
            cloud_print_progress_timeout=cloud_print_progress_timeout,
            cloud_print_max_bytes=cloud_print_max_mb * 1024 * 1024,
            staging_dir=Path(os.getenv("CLOUD_LAN_PROXY_STAGING_DIR", "/staging")),
            cloud_print_job_dir=Path(
                os.getenv(
                    "CLOUD_LAN_PROXY_CLOUD_PRINT_JOB_DIR",
                    "/data/cloud-lan-proxy-jobs",
                )
            ),
            printers=printers,
            advertise_controls=advertise_controls,
        )

    def permits_printer(self, serial: str) -> bool:
        return not self.printers or serial.upper() in self.printers
