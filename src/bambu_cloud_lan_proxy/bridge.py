"""MQTT broker adapter embedded into Bambu Gateway's cloud process."""

from __future__ import annotations

import asyncio
import hashlib
import json
import logging
import threading
import time
from collections import OrderedDict
from typing import Any

import paho.mqtt.client as mqtt

from .commands import translate_control
from .config import ProxyConfig
from .policy import Action, Decision, decide, rejection_envelope
from .reports import prepare_report

logger = logging.getLogger("bambu.cloud_lan_proxy")


class CloudLanFacade:
    """Connects Gateway cloud clients to a local Bambu-shaped MQTT broker."""

    def __init__(self, config: ProxyConfig) -> None:
        self.config = config
        self._clients: dict[str, Any] = {}
        self._loop: asyncio.AbstractEventLoop | None = None
        self._mqtt: mqtt.Client | None = None
        self._started = False
        self._start_lock = threading.Lock()
        self._recent: OrderedDict[str, float] = OrderedDict()

    def register(self, cloud_client: Any) -> None:
        serial = str(cloud_client.serial).upper()
        if not self.config.permits_printer(serial):
            logger.info("serial=%s omitted by CLOUD_LAN_PROXY_PRINTERS", serial)
            return
        self._clients[serial] = cloud_client
        try:
            self._loop = asyncio.get_running_loop()
        except RuntimeError:
            logger.warning("serial=%s registered before an asyncio loop was available", serial)
        self._ensure_started()
        logger.info("serial=%s registered with cloud LAN facade", serial)

    def _ensure_started(self) -> None:
        if self._started:
            return
        with self._start_lock:
            if self._started:
                return
            client = mqtt.Client(
                callback_api_version=mqtt.CallbackAPIVersion.VERSION2,
                client_id="bambu-cloud-lan-proxy-gateway",
                protocol=mqtt.MQTTv311,
            )
            client.on_connect = self._on_connect
            client.on_disconnect = self._on_disconnect
            client.on_message = self._on_message
            client.reconnect_delay_set(min_delay=1, max_delay=30)
            client.max_queued_messages_set(1000)
            client.connect_async(
                self.config.broker_host,
                self.config.broker_port,
                keepalive=30,
            )
            client.loop_start()
            self._mqtt = client
            self._started = True
            logger.info(
                "starting cloud LAN facade broker=%s:%d mode=%s cancel=%s controls-advertised=%s",
                self.config.broker_host,
                self.config.broker_port,
                self.config.mode.value,
                self.config.allow_cancel,
                self.config.advertise_controls,
            )

    def publish_report(self, serial: str, raw: object) -> None:
        serial = serial.upper()
        if serial not in self._clients or self._mqtt is None:
            return
        try:
            payload = prepare_report(raw, controls=self.config.advertise_controls)
        except (UnicodeDecodeError, ValueError, TypeError, json.JSONDecodeError):
            logger.warning("serial=%s ignored invalid cloud report", serial)
            return
        result = self._mqtt.publish(
            f"device/{serial}/report",
            payload,
            qos=self.config.report_qos,
            retain=False,
        )
        if result.rc not in {mqtt.MQTT_ERR_SUCCESS, mqtt.MQTT_ERR_NO_CONN}:
            logger.warning("serial=%s report publish rc=%s", serial, result.rc)

    def _on_connect(self, client, _userdata, _flags, reason_code, _properties) -> None:
        if reason_code.is_failure:
            logger.error("local MQTT connection failed: %s", reason_code)
            return
        client.subscribe("device/+/request", qos=self.config.request_qos)
        logger.info("connected to local MQTT broker and subscribed to request topics")

    def _on_disconnect(
        self, _client, _userdata, _flags, reason_code, _properties
    ) -> None:
        if reason_code.is_failure:
            logger.warning("local MQTT disconnected unexpectedly: %s", reason_code)

    def _on_message(self, _client, _userdata, message) -> None:
        parts = message.topic.split("/")
        if len(parts) != 3 or parts[0] != "device" or parts[2] != "request":
            return
        serial = parts[1].upper()
        cloud_client = self._clients.get(serial)
        if cloud_client is None:
            logger.warning("ignored request for unregistered serial=%s", serial)
            return
        try:
            envelope = json.loads(message.payload)
        except (UnicodeDecodeError, json.JSONDecodeError):
            logger.warning("serial=%s ignored non-JSON request", serial)
            return

        decision = decide(
            envelope,
            mode=self.config.mode,
            allow_cancel=self.config.allow_cancel,
        )
        if not decision.allowed:
            self._publish_rejection(serial, decision)
            command = (
                f"{decision.request.namespace}.{decision.request.command}"
                if decision.request
                else "unknown"
            )
            logger.warning(
                "serial=%s blocked command=%s reason=%s",
                serial,
                command,
                decision.reason,
            )
            return

        if decision.action is Action.FORWARD_CONTROL and self._is_duplicate(
            serial, decision, message.payload
        ):
            logger.info("serial=%s ignored duplicate command", serial)
            return
        loop = self._loop
        if loop is None or loop.is_closed():
            self._publish_rejection(
                serial,
                Decision(Action.BLOCK, decision.request, "Gateway event loop unavailable"),
            )
            return
        loop.call_soon_threadsafe(
            lambda: asyncio.create_task(self._dispatch(serial, cloud_client, decision))
        )

    def _is_duplicate(self, serial: str, decision: Decision, raw: bytes) -> bool:
        now = time.monotonic()
        while self._recent and next(iter(self._recent.values())) < now - 60:
            self._recent.popitem(last=False)
        request = decision.request
        assert request is not None
        digest = hashlib.sha256(raw).hexdigest()[:16]
        key = f"{serial}:{request.namespace}:{request.command}:{request.sequence_id}:{digest}"
        previous = self._recent.get(key)
        self._recent[key] = now
        self._recent.move_to_end(key)
        ttl = 1.0 if not request.sequence_id else 30.0
        return previous is not None and now - previous < ttl

    async def _dispatch(self, serial: str, cloud_client: Any, decision: Decision) -> None:
        request = decision.request
        assert request is not None
        try:
            if decision.action is Action.FORWARD_CONTROL:
                envelope = translate_control(request)
            else:
                envelope = request.envelope
            rc = await cloud_client.send_command(envelope=envelope, qos=1)
            if rc != 0:
                raise RuntimeError(f"Bambu cloud plugin returned rc={rc}")
            logger.info(
                "serial=%s forwarded command=%s.%s policy=%s",
                serial,
                request.namespace,
                request.command,
                decision.action.value,
            )
        except Exception as exc:
            logger.error(
                "serial=%s command=%s.%s failed: %s",
                serial,
                request.namespace,
                request.command,
                exc,
            )
            self._publish_rejection(
                serial,
                Decision(Action.BLOCK, request, str(exc)),
            )

    def _publish_rejection(self, serial: str, decision: Decision) -> None:
        if self._mqtt is None:
            return
        payload = rejection_envelope(decision)
        if payload is None:
            return
        self._mqtt.publish(
            f"device/{serial}/report",
            json.dumps(payload, separators=(",", ":")),
            qos=0,
            retain=False,
        )
