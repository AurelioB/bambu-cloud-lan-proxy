"""Configure an authenticated RTSPS facade backed by a printer LAN camera."""

from __future__ import annotations

import hashlib
import ipaddress
import json
import os
import socket
import ssl
import tempfile
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote


class CameraConfigError(RuntimeError):
    """Raised when a safe camera relay configuration cannot be assembled."""


@dataclass(frozen=True, slots=True)
class CameraSource:
    serial: str
    access_code: str


def load_camera_source(registry_path: Path, requested_serial: str = "") -> CameraSource:
    """Load one printer's camera credential from Gateway's private registry."""

    try:
        raw = json.loads(registry_path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise CameraConfigError(
            f"Gateway printer registry is not ready: {registry_path}"
        ) from exc
    except (OSError, json.JSONDecodeError) as exc:
        raise CameraConfigError(f"could not read Gateway printer registry: {exc}") from exc

    if not isinstance(raw, list):
        raise CameraConfigError("Gateway printer registry must contain a list")
    entries = [entry for entry in raw if isinstance(entry, dict)]
    serial = requested_serial.strip().upper()
    if serial:
        entries = [
            entry for entry in entries if str(entry.get("serial", "")).strip().upper() == serial
        ]
        if not entries:
            raise CameraConfigError(f"serial {serial} is not present in Gateway's registry")
    elif len(entries) != 1:
        raise CameraConfigError(
            "CAMERA_PRINTER_SERIAL is required when Gateway has zero or multiple printers"
        )

    if len(entries) != 1:
        raise CameraConfigError(f"serial {serial} is duplicated in Gateway's registry")
    entry = entries[0]
    resolved_serial = str(entry.get("serial", "")).strip().upper()
    access_code = str(entry.get("access_code", "")).strip()
    if not resolved_serial or not access_code:
        raise CameraConfigError("Gateway registry entry has no serial or LAN liveview access code")
    return CameraSource(serial=resolved_serial, access_code=access_code)


def normalize_fingerprint(value: str) -> str:
    normalized = value.replace(":", "").strip().upper()
    if len(normalized) != 64 or any(char not in "0123456789ABCDEF" for char in normalized):
        raise CameraConfigError("camera TLS fingerprint must be a SHA-256 hex digest")
    return normalized


def fetch_tls_fingerprint(host: str, port: int, timeout: float = 8.0) -> str:
    """Fetch a leaf fingerprint for first-use pinning without accepting application data."""

    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    try:
        with (
            socket.create_connection((host, port), timeout=timeout) as raw,
            context.wrap_socket(raw, server_hostname=host) as tls,
        ):
            certificate = tls.getpeercert(binary_form=True)
    except OSError as exc:
        raise CameraConfigError(f"cannot reach camera TLS endpoint {host}:{port}: {exc}") from exc
    if not certificate:
        raise CameraConfigError("camera TLS endpoint did not provide a certificate")
    return hashlib.sha256(certificate).hexdigest().upper()


def resolve_fingerprint(
    *,
    host: str,
    port: int,
    serial: str,
    state_dir: Path,
    configured: str = "",
) -> str:
    """Use an explicit pin or persist a trust-on-first-use leaf fingerprint."""

    if configured.strip():
        return normalize_fingerprint(configured)

    state_dir.mkdir(parents=True, exist_ok=True)
    pin_path = state_dir / f"{serial}.sha256"
    try:
        existing = pin_path.read_text(encoding="ascii").strip()
    except FileNotFoundError:
        existing = ""
    except OSError as exc:
        raise CameraConfigError(f"could not read camera TLS pin: {exc}") from exc
    if existing:
        return normalize_fingerprint(existing)

    fingerprint = fetch_tls_fingerprint(host, port)
    try:
        fd, temporary_name = tempfile.mkstemp(prefix=f".{serial}.", dir=state_dir)
        with os.fdopen(fd, "w", encoding="ascii") as output:
            output.write(fingerprint + "\n")
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary_name, 0o600)
        os.replace(temporary_name, pin_path)
    except OSError as exc:
        raise CameraConfigError(f"could not persist camera TLS pin: {exc}") from exc
    return fingerprint


def build_mediamtx_config(
    *,
    upstream_ip: str,
    upstream_port: int,
    upstream_access_code: str,
    upstream_fingerprint: str,
    downstream_access_code: str,
    server_cert: Path,
    server_key: Path,
    listen_ip: str = "127.0.0.2",
    listen_port: int = 8322,
) -> dict[str, object]:
    """Build a minimal MediaMTX config with one on-demand H.264 path."""

    try:
        ipaddress.ip_address(upstream_ip)
    except ValueError as exc:
        raise CameraConfigError("CAMERA_UPSTREAM_IP must be a literal IP address") from exc
    try:
        resolved_listen_ip = socket.gethostbyname(listen_ip)
        listen_address = ipaddress.ip_address(resolved_listen_ip)
    except (OSError, ValueError) as exc:
        raise CameraConfigError("CAMERA_LISTEN_IP could not be resolved") from exc
    if not (listen_address.is_loopback or listen_address.is_private):
        raise CameraConfigError("CAMERA_LISTEN_IP must resolve to a private or loopback address")
    if not 1 <= upstream_port <= 65535 or not 1 <= listen_port <= 65535:
        raise CameraConfigError("camera ports must be between 1 and 65535")
    if not upstream_access_code:
        raise CameraConfigError("physical camera access code is empty")
    if not downstream_access_code:
        raise CameraConfigError("PROXY_ACCESS_CODE is empty")
    if not server_cert.is_file() or not server_key.is_file():
        raise CameraConfigError("proxy TLS certificate or key is missing")

    encoded_user = quote("bblp", safe="")
    encoded_password = quote(upstream_access_code, safe="")
    source = (
        f"rtsps://{encoded_user}:{encoded_password}@{upstream_ip}:{upstream_port}"
        "/streaming/live/1"
    )
    return {
        "logLevel": os.getenv("CAMERA_LOG_LEVEL", "info").strip().lower(),
        "readTimeout": "20s",
        "writeTimeout": "20s",
        "authMethod": "internal",
        "authInternalUsers": [
            {
                "user": "bblp",
                "pass": downstream_access_code,
                "ips": [],
                "permissions": [
                    {"action": "read", "path": "streaming/live/1"},
                ],
            }
        ],
        "api": False,
        "metrics": False,
        "pprof": False,
        "playback": False,
        "rtsp": True,
        "rtspTransports": ["tcp"],
        "rtspEncryption": "strict",
        "rtspsAddress": f"{resolved_listen_ip}:{listen_port}",
        "rtspServerCert": str(server_cert),
        "rtspServerKey": str(server_key),
        "rtspAuthMethods": ["basic"],
        "rtmp": False,
        "hls": False,
        "webrtc": False,
        "srt": False,
        "moq": False,
        "paths": {
            "streaming/live/1": {
                "source": source,
                "sourceFingerprint": normalize_fingerprint(upstream_fingerprint),
                "sourceOnDemand": True,
                "sourceOnDemandStartTimeout": "20s",
                "sourceOnDemandCloseAfter": "10s",
                "rtspTransport": "tcp",
            }
        },
    }


def main() -> None:
    registry = Path(os.getenv("CAMERA_GATEWAY_REGISTRY", "/gateway/printers.json"))
    source = load_camera_source(registry, os.getenv("CAMERA_PRINTER_SERIAL", ""))
    upstream_ip = os.getenv("CAMERA_UPSTREAM_IP", "").strip()
    if not upstream_ip:
        raise CameraConfigError(
            "CAMERA_UPSTREAM_IP is required (LAN-only liveview does not advertise through Gateway)"
        )
    upstream_port = int(os.getenv("CAMERA_UPSTREAM_PORT", "322"))
    state_dir = Path(os.getenv("CAMERA_STATE_DIR", "/state"))
    fingerprint = resolve_fingerprint(
        host=upstream_ip,
        port=upstream_port,
        serial=source.serial,
        state_dir=state_dir,
        configured=os.getenv("CAMERA_UPSTREAM_TLS_FINGERPRINT", ""),
    )
    config = build_mediamtx_config(
        upstream_ip=upstream_ip,
        upstream_port=upstream_port,
        upstream_access_code=source.access_code,
        upstream_fingerprint=fingerprint,
        downstream_access_code=os.environ.get("PROXY_ACCESS_CODE", ""),
        server_cert=Path(os.getenv("CAMERA_SERVER_CERT", "/secrets/certs/server.crt")),
        server_key=Path(os.getenv("CAMERA_SERVER_KEY", "/secrets/certs/server.key")),
        listen_ip=os.getenv("CAMERA_LISTEN_IP", "host.docker.internal"),
        listen_port=int(os.getenv("CAMERA_LISTEN_PORT", "8322")),
    )

    config_path = Path(os.getenv("CAMERA_CONFIG_PATH", "/tmp/mediamtx.json"))
    config_path.write_text(json.dumps(config, separators=(",", ":")), encoding="utf-8")
    config_path.chmod(0o600)
    os.execv("/usr/local/bin/mediamtx", ["mediamtx", str(config_path)])


if __name__ == "__main__":
    main()
