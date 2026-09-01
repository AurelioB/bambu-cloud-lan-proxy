"""Read-only local MQTT telemetry source with cloud-report fallback signaling."""

from __future__ import annotations

import hashlib
import hmac
import json
import logging
import os
import ssl
import time
from pathlib import Path

import paho.mqtt.client as mqtt

from .camera_relay import load_camera_source, resolve_fingerprint

logger = logging.getLogger("bambu.cloud_lan_proxy.local_status")


def main() -> None:
    logging.basicConfig(
        level=os.getenv("LOG_LEVEL", "INFO").upper(),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    source = load_camera_source(
        Path(os.getenv("LOCAL_STATUS_GATEWAY_REGISTRY", "/gateway/printers.json")),
        os.getenv("LOCAL_STATUS_PRINTER_SERIAL", ""),
    )
    upstream_ip = os.getenv("LOCAL_STATUS_UPSTREAM_IP", "").strip()
    if not upstream_ip:
        raise SystemExit("LOCAL_STATUS_UPSTREAM_IP is required")
    state_dir = Path(os.getenv("LOCAL_STATUS_STATE_DIR", "/state"))
    state_dir.mkdir(parents=True, exist_ok=True)
    heartbeat = state_dir / f"{source.serial}.local"
    fingerprint = resolve_fingerprint(
        host=upstream_ip,
        port=int(os.getenv("LOCAL_STATUS_UPSTREAM_PORT", "8883")),
        serial=f"{source.serial}-mqtt",
        state_dir=state_dir,
        configured=os.getenv("LOCAL_STATUS_TLS_FINGERPRINT", ""),
    )

    downstream = mqtt.Client(
        callback_api_version=mqtt.CallbackAPIVersion.VERSION2,
        client_id=f"bambu-local-status-out-{source.serial}",
    )
    downstream.connect(
        os.getenv("LOCAL_STATUS_BROKER_HOST", "mqtt"),
        int(os.getenv("LOCAL_STATUS_BROKER_PORT", "1883")),
        30,
    )
    downstream.loop_start()

    upstream = mqtt.Client(
        callback_api_version=mqtt.CallbackAPIVersion.VERSION2,
        client_id=f"bambu-local-status-in-{source.serial}",
    )
    upstream.username_pw_set("bblp", source.access_code)
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    upstream.tls_set_context(context)
    upstream.reconnect_delay_set(2, 30)

    def on_connect(client, _userdata, _flags, reason_code, _properties) -> None:
        if reason_code.is_failure:
            logger.warning("local status connection failed: %s", reason_code)
            return
        sock = client.socket()
        certificate = sock.getpeercert(binary_form=True) if sock else None
        actual = hashlib.sha256(certificate or b"").hexdigest().upper()
        if not hmac.compare_digest(actual, fingerprint):
            logger.error("local status TLS certificate changed; refusing telemetry")
            client.disconnect()
            return
        client.subscribe(f"device/{source.serial}/report", qos=0)
        logger.info("using local read-only telemetry for serial=%s", source.serial)

    def on_message(_client, _userdata, message) -> None:
        try:
            json.loads(message.payload)
        except (UnicodeDecodeError, json.JSONDecodeError):
            return
        downstream.publish(message.topic, message.payload, qos=0, retain=False)
        temporary = heartbeat.with_suffix(".tmp")
        temporary.write_text(str(time.time()), encoding="ascii")
        os.replace(temporary, heartbeat)

    upstream.on_connect = on_connect
    upstream.on_message = on_message
    upstream.connect_async(upstream_ip, int(os.getenv("LOCAL_STATUS_UPSTREAM_PORT", "8883")), 30)
    try:
        upstream.loop_forever(retry_first_connection=True)
    finally:
        heartbeat.unlink(missing_ok=True)
        downstream.loop_stop()


if __name__ == "__main__":
    main()
