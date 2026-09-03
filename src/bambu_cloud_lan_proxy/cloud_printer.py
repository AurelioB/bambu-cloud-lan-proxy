"""Minimal printer state and transport; clients receive original MQTT reports.

Only job-dispatch safety state is interpreted here. AMS, temperature, nozzle,
and other model-specific schemas remain the downstream client's responsibility.
"""

from __future__ import annotations

import asyncio
import json
import os
import time
from pathlib import Path
from types import SimpleNamespace

from .plugin_rpc import PluginError, PluginRPC


class CloudPrinter:
    def __init__(self, serial: str, host: PluginRPC, safety_dir: Path | None = None) -> None:
        self.serial = serial
        self.host = host
        self.state = "unknown"
        self.state_seen = 0.0
        self.secure = False
        self.connected = False
        self._progress: asyncio.Queue | None = None
        self.marker = safety_dir / f"{serial}.pending" if safety_dir else None
        self.uncertain_submission = bool(self.marker and self.marker.exists())

    def receive(self, payload: object) -> bool:
        if payload == "device_cert_installed":
            self.secure = True
            return False
        if payload in ("device_cert_uninstalled", "cert_expired", "cert_revoked", "wait_info"):
            self.secure = False
            return False
        try:
            envelope = json.loads(payload) if isinstance(payload, str | bytes) else payload
        except (ValueError, UnicodeError):
            return False
        if not isinstance(envelope, dict):
            return False
        report = envelope.get("print")
        if isinstance(report, dict) and isinstance(report.get("gcode_state"), str):
            self.state = {
                "IDLE": "idle",
                "FINISH": "finished",
                "RUNNING": "running",
                "PAUSE": "paused",
                "PREPARE": "preparing",
            }.get(report["gcode_state"], "unknown")
            self.state_seen = time.monotonic()
        return True

    def get_status(self) -> SimpleNamespace:
        return SimpleNamespace(
            online=self.connected
            and not self.host.failure
            and time.monotonic() - self.state_seen < 90,
            state="uncertain" if self.uncertain_submission else self.state,
        )

    async def send_command(self, *, envelope: dict, qos: int = 0) -> int:
        result = await self.host.call(
            "send_message",
            {
                "dev_id": self.serial,
                "payload": json.dumps(envelope),
                "qos": qos,
            },
        )
        return int(result.get("rc", -1))

    async def submit_print(self, *, print_params: dict, progress_timeout: float = 900):
        if self._progress is not None or self.uncertain_submission:
            raise PluginError("previous print submission active or outcome unknown")
        if not self.secure or not self.get_status().online:
            raise PluginError("printer control/state is not ready")
        self._progress = asyncio.Queue(maxsize=256)
        self.uncertain_submission = True
        try:
            if self.marker:
                self.marker.parent.mkdir(parents=True, exist_ok=True)
                # Exclusive creation refuses a second daemon sharing this printer's state.
                with self.marker.open("x") as pending:
                    pending.write("Cloud submission outcome pending; do not automatically retry.\n")
                    pending.flush()
                    os.fsync(pending.fileno())
            result = await self.host.call("start_print", print_params)
            if result.get("rc") != 0:
                self.uncertain_submission = False
                if self.marker:
                    self.marker.unlink(missing_ok=True)
                yield {"event": "error", "msg": "plugin refused submission"}
                return
            failed = False
            while True:
                event = await asyncio.wait_for(self._progress.get(), progress_timeout)
                if event.get("kind") == "PrintComplete":
                    self.uncertain_submission = False
                    if self.marker:
                        self.marker.unlink(missing_ok=True)
                    yield (
                        {"event": "done"}
                        if event["rc"] == 0 and not failed
                        else {
                            "event": "error",
                            "msg": "cloud submission failed",
                            "code": event["rc"],
                        }
                    )
                    return
                failed = failed or event.get("stage") == 7
                yield {"event": "progress", "stage": event.get("stage"), "code": event.get("code")}
        except TimeoutError:
            raise PluginError("print submission outcome unknown; automatic retry blocked") from None
        finally:
            self._progress = None
