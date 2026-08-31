"""Environment configuration for the embedded MQTT facade."""

from __future__ import annotations

import os
from dataclasses import dataclass
from enum import StrEnum


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

        advertise_raw = os.getenv("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", "auto")
        if advertise_raw.strip().lower() == "auto":
            advertise_controls = mode is not ProxyMode.READ_ONLY
        else:
            advertise_controls = _env_bool(
                "CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", False
            )

        broker_port = int(os.getenv("CLOUD_LAN_PROXY_BROKER_PORT", "1883"))
        if not 1 <= broker_port <= 65535:
            raise ValueError("CLOUD_LAN_PROXY_BROKER_PORT must be between 1 and 65535")

        return cls(
            enabled=_env_bool("CLOUD_LAN_PROXY_ENABLED", True),
            broker_host=os.getenv("CLOUD_LAN_PROXY_BROKER_HOST", "mqtt").strip(),
            broker_port=broker_port,
            mode=mode,
            allow_cancel=_env_bool("CLOUD_LAN_PROXY_ALLOW_CANCEL", False),
            printers=printers,
            advertise_controls=advertise_controls,
        )

    def permits_printer(self, serial: str) -> bool:
        return not self.printers or serial.upper() in self.printers
