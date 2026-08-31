from __future__ import annotations

import json
from pathlib import Path

import pytest

from bambu_cloud_lan_proxy.camera_relay import (
    CameraConfigError,
    build_mediamtx_config,
    load_camera_source,
    normalize_fingerprint,
)


def test_load_camera_source_selects_serial_without_exposing_other_credentials(
    tmp_path: Path,
) -> None:
    registry = tmp_path / "printers.json"
    registry.write_text(
        json.dumps(
            [
                {"serial": "SERIAL-A", "access_code": "AAAAAAAA"},
                {"serial": "serial-b", "access_code": "BBBBBBBB"},
            ]
        )
    )

    source = load_camera_source(registry, "SERIAL-B")

    assert source.serial == "SERIAL-B"
    assert source.access_code == "BBBBBBBB"


def test_load_camera_source_requires_selection_for_multiple_printers(tmp_path: Path) -> None:
    registry = tmp_path / "printers.json"
    registry.write_text(
        json.dumps(
            [
                {"serial": "A", "access_code": "AAAAAAAA"},
                {"serial": "B", "access_code": "BBBBBBBB"},
            ]
        )
    )

    with pytest.raises(CameraConfigError, match="CAMERA_PRINTER_SERIAL"):
        load_camera_source(registry)


def test_build_mediamtx_config_is_authenticated_tls_and_on_demand(tmp_path: Path) -> None:
    cert = tmp_path / "server.crt"
    key = tmp_path / "server.key"
    cert.write_text("certificate")
    key.write_text("key")

    config = build_mediamtx_config(
        upstream_ip="192.0.2.25",
        upstream_port=322,
        upstream_access_code="PHYSICAL",
        upstream_fingerprint="AA:" * 31 + "AA",
        downstream_access_code="LOCAL123",
        server_cert=cert,
        server_key=key,
        listen_ip="127.0.0.2",
        listen_port=322,
    )

    assert config["rtspEncryption"] == "strict"
    assert config["rtspsAddress"] == "127.0.0.2:322"
    assert config["rtspTransports"] == ["tcp"]
    assert config["authInternalUsers"] == [
        {
            "user": "bblp",
            "pass": "LOCAL123",
            "ips": [],
            "permissions": [{"action": "read", "path": "streaming/live/1"}],
        }
    ]
    path = config["paths"]["streaming/live/1"]
    assert path["source"] == "rtsps://bblp:PHYSICAL@192.0.2.25:322/streaming/live/1"
    assert path["sourceOnDemand"] is True
    assert path["sourceFingerprint"] == "AA" * 32


@pytest.mark.parametrize("value", ["", "AA", "not-a-digest", "GG" * 32])
def test_normalize_fingerprint_rejects_invalid_values(value: str) -> None:
    with pytest.raises(CameraConfigError):
        normalize_fingerprint(value)
