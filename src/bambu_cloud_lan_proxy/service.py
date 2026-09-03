"""Headless cloud-session service owned by Bambu Cloud LAN Proxy.

No upstream app imports, monkeypatches, slicer, notification system, or UI.
The historical /data layout is retained solely for credential migration.
"""

from __future__ import annotations

import asyncio
import hmac
import json
import logging
import os
import re
import time
import uuid
from contextlib import asynccontextmanager, suppress
from pathlib import Path

from fastapi import Depends, FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

from .bridge import CloudLanFacade
from .cloud_auth import login, signin_url
from .cloud_printer import CloudPrinter
from .commands import command
from .config import ProxyConfig
from .plugin_install import ensure_plugin, studio_headers
from .plugin_rpc import PluginError, PluginRPC
from .storage_api import install_storage_routes

logger = logging.getLogger(__name__)


class Callback(BaseModel):
    pasted_url: str = Field(max_length=20000)


class Session:
    def __init__(self, data: Path, region: str, config: ProxyConfig) -> None:
        self.data, self.region = data, region
        self.config = config
        self.facade = CloudLanFacade(config)
        self.printers: dict[str, CloudPrinter] = {}
        self.signed_in = False
        self.connected = False
        self.lock = asyncio.Lock()
        self.host: PluginRPC | None = None
        self.tasks: list[asyncio.Task] = []
        self.last_refresh = 0.0

    async def start(self) -> None:
        active = self.data / "bambu-plugin" / "active"
        await ensure_plugin(active, self.region)
        identity_dir = self.data / "bambu-plugin" / "state"
        identity_dir.mkdir(parents=True, exist_ok=True)
        identity = identity_dir / "device_id"
        if not identity.exists():
            identity.write_text(str(uuid.uuid4()))
            identity.chmod(0o600)
        self.host = PluginRPC(
            [os.getenv("BAMBU_PLUGIN_HOST", "/usr/local/bin/bambu-proxy-native")],
            {
                "BAMBU_NETWORK_SO": str(active / "libbambu_networking.so"),
                "BAMBU_SOURCE_SO": str(active / "libBambuSource.so"),
                "BAMBU_PLUGIN_STATE": str(active / "state"),
                "BAMBU_CLOUD_REGION": self.region,
                "SSL_CERT_FILE": "/etc/ssl/certs/ca-certificates.crt",
                "SSL_CERT_DIR": "/etc/ssl/certs",
                "CURL_CA_BUNDLE": "/etc/ssl/certs/ca-certificates.crt",
            },
        )
        await self.host.start()
        await self.host.call("init_plugin", {})
        headers = studio_headers()
        headers.pop("User-Agent")
        headers.update(
            {
                "X-BBL-OS-Version": "6.1.0",
                "X-BBL-Language": "en-US",
                "X-BBL-Device-ID": identity.read_text().strip(),
            }
        )
        await self.checked("set_extra_http_header", {"headers": headers})
        self.tasks = [asyncio.create_task(self.events()), asyncio.create_task(self.monitor())]

    async def checked(self, method: str, params: dict) -> dict:
        assert self.host
        result = await self.host.call(method, params)
        if result.get("rc", 0) != 0:
            raise PluginError("plugin rejected session operation")
        return result

    async def refresh_devices(self) -> None:
        result = await self.checked("get_user_print_info", {})
        if not isinstance(result.get("devices"), list):
            raise PluginError("invalid device discovery response")
        registry_path = self.data / "printers.json"
        previous = json.loads(registry_path.read_text()) if registry_path.exists() else []
        registry = {entry["serial"]: entry for entry in previous}
        serials = set()
        for device in result["devices"]:
            serial = str(device.get("dev_id", "")).upper()
            if not re.fullmatch(r"[A-Z0-9]{5,40}", serial) or not self.config.permits_printer(
                serial
            ):
                continue
            serials.add(serial)
            if serial not in self.printers:
                assert self.host
                printer = CloudPrinter(serial, self.host, self.data / "submissions")
                self.printers[serial] = printer
                self.facade.register(printer)
            registry.setdefault(serial, {"serial": serial, "ip": "", "access_code": ""})
            registry[serial]["name"] = device.get("dev_name", serial)
            registry[serial]["machine_model"] = device.get("dev_model_name", "")
        for serial in self.printers.keys() - serials:
            self.printers[serial].connected = False
            self.printers[serial].secure = False
            self.facade._clients.pop(serial, None)
            del self.printers[serial]
        # Preserve LAN credentials and the historical on-disk schema.
        temporary = registry_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(list(registry.values()), indent=2))
        temporary.chmod(0o600)
        temporary.replace(registry_path)
        self.last_refresh = time.monotonic()

    async def connect(self) -> None:
        async with self.lock:
            await self.refresh_devices()
            await self.checked("connect_server", {})
            for _ in range(40):
                if (await self.checked("is_server_connected", {})).get("connected"):
                    break
                await asyncio.sleep(0.5)
            else:
                raise PluginError("cloud connection timed out")
            await self.checked("start_subscribe", {"module": "app"})
            for serial in self.printers:
                await self.checked("set_user_selected_machine", {"dev_id": serial})
            await self.checked("add_subscribe", {"dev_ids": list(self.printers)})
            self.connected = True

    async def snapshot(self, printer: CloudPrinter) -> None:
        for namespace, name in (("info", "get_version"), ("pushing", "pushall")):
            await printer.send_command(envelope=command(namespace, name))

    async def events(self) -> None:
        assert self.host
        while True:
            event = await self.host.events.get()
            printer = self.printers.get(str(event.get("dev_id", "")).upper())
            if printer is None:
                continue
            try:
                kind = event.get("kind")
                if kind == "OnPrinterConnected":
                    printer.connected = True
                    await self.checked("install_device_cert", {"dev_id": printer.serial})
                    await self.snapshot(printer)
                elif kind == "OnMessage":
                    payload = event.get("payload")
                    if printer.receive(payload):
                        self.facade.publish_report(printer.serial, payload)
                elif kind in {"OnUpdateStatus", "PrintComplete"} and printer._progress is not None:
                    printer._progress.put_nowait(event)
            except Exception:
                # Payloads and native exception details can contain private data.
                logger.warning("cloud event handling failed (%s)", event.get("kind"))

    async def monitor(self) -> None:
        assert self.host
        last_snapshot = 0.0
        while True:
            try:
                self.signed_in = bool((await self.checked("is_user_login", {})).get("is_login"))
                broker = bool((await self.checked("is_server_connected", {})).get("connected"))
                if not self.signed_in or not broker:
                    self.connected = False
                    for printer in self.printers.values():
                        printer.connected = printer.secure = False
                    if self.signed_in:
                        await self.connect()
                elif not self.connected:
                    await self.connect()
                if self.connected and time.monotonic() - last_snapshot > 30:
                    for printer in self.printers.values():
                        if printer.connected:
                            if not printer.secure:
                                await self.checked(
                                    "install_device_cert", {"dev_id": printer.serial}
                                )
                            await self.snapshot(printer)
                    last_snapshot = time.monotonic()
                if self.connected and time.monotonic() - self.last_refresh > 300:
                    # Account changes need a new subscription, not cached credentials.
                    await self.connect()
            except Exception:
                self.connected = False
                logger.warning("cloud session unavailable; retrying session setup (not commands)")
            await asyncio.sleep(10)

    async def stop(self) -> None:
        for task in self.tasks:
            task.cancel()
        for task in self.tasks:
            with suppress(asyncio.CancelledError):
                await task
        await self.facade.stop()
        if self.host:
            await self.host.stop()


def create_app() -> FastAPI:
    session = Session(
        Path(os.getenv("PROXY_DATA_DIR", "/data")),
        os.getenv("BAMBU_CLOUD_REGION", "US"),
        ProxyConfig.from_env(),
    )

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        try:
            await session.start()
            app.state.cloud_host = session.host
            yield
        finally:
            app.state.cloud_host = None
            await session.stop()

    app = FastAPI(title="Bambu Cloud LAN Proxy", lifespan=lifespan)

    def admin(x_cloud_lan_proxy_key: str | None = Header(default=None)) -> None:
        expected = os.getenv("CLOUD_LAN_PROXY_ACCESS_CODE", "")
        if (
            not expected
            or not x_cloud_lan_proxy_key
            or not hmac.compare_digest(expected, x_cloud_lan_proxy_key)
        ):
            raise HTTPException(401, "unauthorized")

    @app.get("/api/health")
    async def health():
        if not session.host or session.host.failure or any(t.done() for t in session.tasks):
            raise HTTPException(503, "native service unavailable")
        return {"service": "bambu-cloud-lan-proxy", "cloud_connected": session.connected}

    @app.get("/api/cloud/auth/status")
    async def auth_status():
        # Deliberately contains no account identity, access codes, or tokens.
        return {"signed_in": session.signed_in, "region": session.region}

    @app.get("/api/cloud/auth/url", dependencies=[Depends(admin)])
    async def auth_url():
        return {"url": signin_url(session.region)}

    @app.post("/api/cloud/auth/paste", dependencies=[Depends(admin)])
    async def auth_paste(body: Callback):
        if not session.host:
            raise HTTPException(503, "plugin unavailable")
        try:
            async with session.lock:
                await login(session.host, session.region, body.pasted_url)
            return {"accepted": True}
        except ValueError:
            raise HTTPException(400, "invalid callback URL") from None
        except Exception:
            raise HTTPException(502, "Bambu sign-in failed") from None

    @app.get("/api/printers", dependencies=[Depends(admin)])
    async def printers():
        return [
            {
                "serial": p.serial,
                "state": p.state,
                "online": p.get_status().online,
                "secure_control": p.secure,
            }
            for p in session.printers.values()
        ]

    install_storage_routes(app)
    return app


def main() -> None:
    import uvicorn

    logging.basicConfig(level=logging.INFO)
    uvicorn.run(create_app(), host="0.0.0.0", port=4844, access_log=False)


if __name__ == "__main__":
    main()
