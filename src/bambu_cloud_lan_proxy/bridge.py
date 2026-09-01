"""MQTT broker adapter embedded into Bambu Gateway's cloud process."""

from __future__ import annotations

import asyncio
import hashlib
import json
import logging
import os
import shutil
import tempfile
import threading
import time
import zipfile
from collections import OrderedDict
from contextlib import aclosing
from pathlib import Path
from typing import Any

import paho.mqtt.client as mqtt

from .commands import translate_control
from .config import ProxyConfig
from .policy import Action, Decision, decide, rejection_envelope
from .print_jobs import (
    CloudPrintRequest,
    CloudPrintValidationError,
    build_gateway_print_params,
    cloud_print_ack,
    cloud_print_failure,
    parse_cloud_print_request,
)
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
        self._print_jobs: OrderedDict[str, tuple[float, str, CloudPrintRequest, str]] = (
            OrderedDict()
        )
        self._print_locks: dict[str, asyncio.Lock] = {}
        self._active_print_serials: set[str] = set()
        self._background_tasks: set[asyncio.Task[Any]] = set()

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
            logger.info(
                "cloud print enabled=%s staging=%s",
                self.config.cloud_print_enabled,
                self.config.staging_dir,
            )

    def publish_report(self, serial: str, raw: object) -> None:
        serial = serial.upper()
        if serial not in self._clients or self._mqtt is None:
            return
        local_state = Path(
            os.getenv("CLOUD_LAN_PROXY_LOCAL_STATUS_STATE_DIR", "/local-status")
        ) / f"{serial}.local"
        try:
            fresh_for = float(
                os.getenv("CLOUD_LAN_PROXY_LOCAL_STATUS_FRESH_SECONDS", "20")
            )
            if time.time() - local_state.stat().st_mtime < fresh_for:
                return
        except OSError:
            pass
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

    def _on_disconnect(self, _client, _userdata, _flags, reason_code, _properties) -> None:
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
            allow_cloud_print=self.config.cloud_print_enabled,
            allow_remote_reprint=self.config.remote_reprint_enabled,
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

        loop = self._loop
        if loop is None or loop.is_closed():
            self._publish_rejection(
                serial,
                Decision(Action.BLOCK, decision.request, "Gateway event loop unavailable"),
            )
            return

        if decision.action is Action.CLOUD_PRINT:
            raw = bytes(message.payload)
            loop.call_soon_threadsafe(
                self._schedule_cloud_print,
                serial,
                cloud_client,
                decision,
                raw,
            )
            return

        mutating_actions = {Action.FORWARD_CONTROL, Action.FORWARD_SIGNED}
        if decision.action in mutating_actions and self._is_duplicate(
            serial, decision, message.payload
        ):
            logger.info("serial=%s ignored duplicate command", serial)
            return
        loop.call_soon_threadsafe(
            lambda: asyncio.create_task(self._dispatch(serial, cloud_client, decision))
        )

    def _schedule_cloud_print(
        self,
        serial: str,
        cloud_client: Any,
        decision: Decision,
        raw: bytes,
    ) -> None:
        request = decision.request
        assert request is not None
        try:
            print_request = parse_cloud_print_request(request.body)
        except CloudPrintValidationError as exc:
            self._publish_json(
                serial,
                cloud_print_failure(
                    sequence_id=request.sequence_id,
                    reason=str(exc),
                ),
            )
            logger.warning("serial=%s rejected cloud print: %s", serial, exc)
            return

        now = time.monotonic()
        for key, (seen_at, state, _request, _reason) in list(self._print_jobs.items()):
            if state != "active" and seen_at < now - 3600:
                self._print_jobs.pop(key, None)

        job_key = hashlib.sha256(serial.encode() + b"\0" + raw).hexdigest()
        previous = self._print_jobs.get(job_key)
        if previous is not None:
            _seen_at, state, old_request, reason = previous
            if state == "failed":
                self._publish_json(
                    serial,
                    cloud_print_failure(
                        sequence_id=old_request.sequence_id,
                        filename=old_request.filename,
                        reason=reason,
                    ),
                )
            else:
                self._publish_json(serial, cloud_print_ack(old_request))
            logger.info(
                "serial=%s replayed cloud print response job=%s state=%s",
                serial,
                job_key[:12],
                state,
            )
            return

        state_error = self._printer_start_error(cloud_client)
        if state_error is None and serial in self._active_print_serials:
            state_error = "another cloud print submission is already pending"
        if state_error is not None:
            self._print_jobs[job_key] = (
                now,
                "failed",
                print_request,
                state_error,
            )
            self._publish_json(
                serial,
                cloud_print_failure(
                    sequence_id=print_request.sequence_id,
                    filename=print_request.filename,
                    reason=state_error,
                ),
            )
            logger.warning("serial=%s rejected cloud print: %s", serial, state_error)
            return

        self._print_jobs[job_key] = (now, "active", print_request, "")
        self._print_jobs.move_to_end(job_key)
        self._active_print_serials.add(serial)
        self._publish_json(serial, cloud_print_ack(print_request))
        task = asyncio.create_task(
            self._run_cloud_print(serial, cloud_client, job_key, print_request),
            name=f"cloud-print-{serial}-{job_key[:12]}",
        )
        self._background_tasks.add(task)
        task.add_done_callback(self._background_tasks.discard)
        logger.info(
            "serial=%s accepted staged cloud print job=%s file=%s plate=%d",
            serial,
            job_key[:12],
            print_request.filename,
            print_request.plate_index,
        )

    @staticmethod
    def _printer_start_error(cloud_client: Any) -> str | None:
        try:
            status = cloud_client.get_status()
        except Exception as exc:
            return f"printer status unavailable ({exc})"
        if not bool(getattr(status, "online", False)):
            return "printer is offline"
        raw_state = getattr(status, "state", "offline")
        state = getattr(raw_state, "value", str(raw_state)).lower()
        if state not in {"idle", "finished", "cancelled"}:
            return f"printer is busy ({state})"
        if getattr(cloud_client, "_progress", None) is not None:
            return "another cloud print submission is already in flight"
        return None

    async def _run_cloud_print(
        self,
        serial: str,
        cloud_client: Any,
        job_key: str,
        request: CloudPrintRequest,
    ) -> None:
        snapshot: Path | None = None
        try:
            lock = self._print_locks.setdefault(serial, asyncio.Lock())
            async with lock:
                source = await self._wait_for_staged_file(request)
                state_error = self._printer_start_error(cloud_client)
                if state_error is not None:
                    raise RuntimeError(state_error)
                snapshot = await self._snapshot_staged_file(source, request.filename)
                params = build_gateway_print_params(
                    request,
                    serial=serial,
                    file_path=snapshot,
                )

                terminal = False
                async with aclosing(
                    cloud_client.submit_print(
                        print_params=params,
                        progress_timeout=self.config.cloud_print_progress_timeout,
                    )
                ) as frames:
                    async for frame in frames:
                        event = frame.get("event")
                        if event == "error":
                            raise RuntimeError(
                                str(frame.get("msg") or frame.get("code") or "cloud error")
                            )
                        if event == "done":
                            terminal = True
                            break
                        logger.info(
                            "serial=%s cloud print job=%s stage=%s code=%s",
                            serial,
                            job_key[:12],
                            frame.get("stage"),
                            frame.get("code"),
                        )
                if not terminal:
                    raise RuntimeError("cloud submission ended without a terminal frame")

            self._print_jobs[job_key] = (
                time.monotonic(),
                "succeeded",
                request,
                "",
            )
            logger.info(
                "serial=%s cloud print job=%s submitted successfully",
                serial,
                job_key[:12],
            )
        except Exception as exc:
            reason = str(exc)[:300] or type(exc).__name__
            self._print_jobs[job_key] = (
                time.monotonic(),
                "failed",
                request,
                reason,
            )
            self._publish_json(
                serial,
                cloud_print_failure(
                    sequence_id=request.sequence_id,
                    filename=request.filename,
                    reason=reason,
                ),
            )
            logger.error(
                "serial=%s cloud print job=%s failed: %s",
                serial,
                job_key[:12],
                reason,
            )
        finally:
            self._active_print_serials.discard(serial)
            if snapshot is not None:
                try:
                    snapshot.unlink(missing_ok=True)
                except OSError as exc:
                    logger.warning("could not remove cloud print snapshot: %s", exc)

    async def _wait_for_staged_file(self, request: CloudPrintRequest) -> Path:
        root = self.config.staging_dir.resolve()
        source = root.joinpath(*request.relative_path.parts)
        deadline = asyncio.get_running_loop().time() + self.config.cloud_print_wait_seconds
        while True:
            try:
                if source.is_symlink():
                    raise RuntimeError("staged file path is unsafe")
                resolved = source.resolve(strict=True)
                if not resolved.is_relative_to(root):
                    raise RuntimeError("staged file path is unsafe")
                stat = resolved.stat()
                if not resolved.is_file() or stat.st_size <= 0:
                    raise RuntimeError("staged file is empty or not a regular file")
                if stat.st_size > self.config.cloud_print_max_bytes:
                    raise RuntimeError("staged file exceeds the cloud print size limit")
                return resolved
            except FileNotFoundError:
                if asyncio.get_running_loop().time() >= deadline:
                    raise RuntimeError(
                        f"timed out waiting for FTPS upload of {request.filename}"
                    ) from None
                await asyncio.sleep(0.25)

    async def _snapshot_staged_file(self, source: Path, filename: str) -> Path:
        job_dir = self.config.cloud_print_job_dir
        job_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        fd, raw_path = tempfile.mkstemp(
            prefix="cloud-lan-proxy-",
            suffix=Path(filename).suffix or ".3mf",
            dir=job_dir,
        )
        os.close(fd)
        snapshot = Path(raw_path)
        try:
            await asyncio.to_thread(shutil.copyfile, source, snapshot)
            if snapshot.stat().st_size > self.config.cloud_print_max_bytes:
                raise RuntimeError("staged file exceeds the cloud print size limit")

            def _validate() -> None:
                with zipfile.ZipFile(snapshot) as archive:
                    archive.infolist()

            await asyncio.to_thread(_validate)
            return snapshot
        except Exception:
            snapshot.unlink(missing_ok=True)
            raise

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
        self._publish_json(serial, payload)

    def _publish_json(self, serial: str, payload: dict[str, Any]) -> None:
        if self._mqtt is None:
            return
        self._mqtt.publish(
            f"device/{serial}/report",
            json.dumps(payload, separators=(",", ":")),
            qos=0,
            retain=False,
        )
