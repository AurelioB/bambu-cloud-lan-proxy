"""Authenticated internal API for Studio-compatible cloud storage tunnels."""

from __future__ import annotations

import hmac
import os
import uuid
from pathlib import Path
from typing import Any

from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel


class ListRequest(BaseModel):
    serial: str
    type: str
    storage: str = ""


class DownloadRequest(BaseModel):
    serial: str
    path: str = ""
    name: str = ""


class DiscardRequest(BaseModel):
    cache_key: str


def install_storage_routes(app: FastAPI) -> None:
    access_code = os.environ.get("CLOUD_LAN_PROXY_ACCESS_CODE", "")
    cache_root = Path(
        os.environ.get("CLOUD_LAN_PROXY_REMOTE_CACHE_DIR", "/remote-cache")
    )
    max_bytes = int(
        os.environ.get("CLOUD_LAN_PROXY_REMOTE_MAX_MB", "4096")
    ) * 1024 * 1024
    allowed = {
        value.strip().upper()
        for value in os.environ.get("CLOUD_LAN_PROXY_PRINTERS", "").split(",")
        if value.strip()
    }

    def authorize_key(supplied: str | None) -> None:
        if not access_code or supplied is None or not hmac.compare_digest(
            supplied, access_code
        ):
            raise HTTPException(status_code=401, detail="unauthorized")

    def authorize(serial: str, supplied: str | None) -> None:
        authorize_key(supplied)
        if allowed and serial.upper() not in allowed:
            raise HTTPException(status_code=403, detail="printer is not allowed")

    def host() -> Any:
        value = getattr(app.state, "cloud_host", None)
        if value is None:
            raise HTTPException(status_code=503, detail="cloud plugin is unavailable")
        return value

    @app.post("/internal/cloud-lan-proxy/storage/list", include_in_schema=False)
    async def storage_list(
        request: ListRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize(request.serial, x_cloud_lan_proxy_key)
        try:
            return await host().call(
                "storage.list",
                {
                    "dev_id": request.serial,
                    "type": request.type,
                    "storage": request.storage,
                },
                timeout=60,
            )
        except Exception as exc:
            raise HTTPException(status_code=502, detail=str(exc)) from exc

    @app.post("/internal/cloud-lan-proxy/storage/download", include_in_schema=False)
    async def storage_download(
        request: DownloadRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize(request.serial, x_cloud_lan_proxy_key)
        if not request.path and not request.name:
            raise HTTPException(status_code=400, detail="path or name is required")
        cache_root.mkdir(parents=True, exist_ok=True)
        cache_key = uuid.uuid4().hex
        target = cache_root / cache_key
        try:
            result = await host().call(
                "storage.download",
                {
                    "dev_id": request.serial,
                    "path": request.path,
                    "name": request.name,
                    "target_path": str(target),
                    "max_bytes": max_bytes,
                },
                timeout=21 * 60,
            )
        except Exception as exc:
            target.unlink(missing_ok=True)
            raise HTTPException(status_code=502, detail=str(exc)) from exc
        return {"cache_key": cache_key, "size": result.get("size", 0)}

    @app.post("/internal/cloud-lan-proxy/storage/discard", include_in_schema=False)
    async def storage_discard(
        request: DiscardRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, bool]:
        # Cleanup is authenticated like every other internal storage operation.
        authorize_key(x_cloud_lan_proxy_key)
        if len(request.cache_key) != 32 or not request.cache_key.isalnum():
            raise HTTPException(status_code=400, detail="invalid cache key")
        (cache_root / request.cache_key).unlink(missing_ok=True)
        return {"discarded": True}
