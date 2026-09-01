"""Authenticated internal API for Studio-compatible cloud storage tunnels."""

from __future__ import annotations

import hmac
import os
import time
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


class AbilityRequest(BaseModel):
    serial: str
    api_version: int = 2


class SubfileRequest(BaseModel):
    serial: str
    request: dict[str, Any]


class DeleteRequest(BaseModel):
    serial: str
    paths: list[str]


class UploadRequest(BaseModel):
    serial: str
    storage: str = ""
    path: str
    source_path: str


def install_storage_routes(app: FastAPI) -> None:
    access_code = os.environ.get("CLOUD_LAN_PROXY_ACCESS_CODE", "")
    cache_root = Path(
        os.environ.get("CLOUD_LAN_PROXY_REMOTE_CACHE_DIR", "/remote-cache")
    )
    max_bytes = int(
        os.environ.get("CLOUD_LAN_PROXY_REMOTE_MAX_MB", "4096")
    ) * 1024 * 1024
    max_subfile_bytes = int(
        os.environ.get("CLOUD_LAN_PROXY_REMOTE_SUBFILE_MAX_MB", "256")
    ) * 1024 * 1024
    allow_delete = os.environ.get(
        "CLOUD_LAN_PROXY_STORAGE_ALLOW_DELETE", "false"
    ).lower() in {"1", "true", "yes", "on"}
    allow_upload = os.environ.get(
        "CLOUD_LAN_PROXY_STORAGE_ALLOW_UPLOAD", "false"
    ).lower() in {"1", "true", "yes", "on"}
    staging_root = Path(
        os.environ.get("CLOUD_LAN_PROXY_STAGING_DIR", "/staging")
    ).resolve()
    revision: dict[str, int] = {}
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

    def cache_target() -> tuple[str, Path]:
        cache_root.mkdir(parents=True, exist_ok=True)
        cache_key = uuid.uuid4().hex
        return cache_key, cache_root / cache_key

    def mark_changed(serial: str) -> int:
        key = serial.upper()
        revision[key] = revision.get(key, 0) + 1
        return revision[key]

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
        cache_key, target = cache_target()
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

    @app.post("/internal/cloud-lan-proxy/storage/ability", include_in_schema=False)
    async def storage_ability(
        request: AbilityRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize(request.serial, x_cloud_lan_proxy_key)
        try:
            result = await host().call(
                "storage.ability",
                {"dev_id": request.serial, "api_version": request.api_version},
                timeout=60,
            )
        except Exception as exc:
            raise HTTPException(status_code=502, detail=str(exc)) from exc
        result["revision"] = revision.get(request.serial.upper(), 0)
        return result

    @app.post("/internal/cloud-lan-proxy/storage/subfile", include_in_schema=False)
    async def storage_subfile(
        request: SubfileRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize(request.serial, x_cloud_lan_proxy_key)
        cache_key, target = cache_target()
        try:
            result = await host().call(
                "storage.subfile",
                {
                    "dev_id": request.serial,
                    "request": request.request,
                    "target_path": str(target),
                    "max_bytes": max_subfile_bytes,
                },
                timeout=6 * 60,
            )
        except Exception as exc:
            target.unlink(missing_ok=True)
            raise HTTPException(status_code=502, detail=str(exc)) from exc
        return {"cache_key": cache_key, "size": result.get("size", 0)}

    @app.post("/internal/cloud-lan-proxy/storage/delete", include_in_schema=False)
    async def storage_delete(
        request: DeleteRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize(request.serial, x_cloud_lan_proxy_key)
        if not allow_delete:
            raise HTTPException(status_code=403, detail="remote delete is disabled")
        if not request.paths or len(request.paths) > 100:
            raise HTTPException(status_code=400, detail="invalid delete request")
        try:
            result = await host().call(
                "storage.delete",
                {"dev_id": request.serial, "paths": request.paths},
                timeout=90,
            )
        except Exception as exc:
            raise HTTPException(status_code=502, detail=str(exc)) from exc
        result["revision"] = mark_changed(request.serial)
        return result

    @app.post("/internal/cloud-lan-proxy/storage/upload", include_in_schema=False)
    async def storage_upload(
        request: UploadRequest,
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize(request.serial, x_cloud_lan_proxy_key)
        if not allow_upload:
            raise HTTPException(status_code=403, detail="remote upload is disabled")
        try:
            source = Path(request.source_path).resolve(strict=True)
        except OSError as exc:
            raise HTTPException(status_code=400, detail="upload source is unavailable") from exc
        if not source.is_relative_to(staging_root) or not source.is_file():
            raise HTTPException(status_code=400, detail="upload source is outside staging")
        try:
            result = await host().call(
                "storage.upload",
                {
                    "dev_id": request.serial,
                    "storage": request.storage,
                    "path": request.path,
                    "source_path": str(source),
                    "max_bytes": max_bytes,
                },
                timeout=21 * 60,
            )
        except Exception as exc:
            raise HTTPException(status_code=502, detail=str(exc)) from exc
        result["revision"] = mark_changed(request.serial)
        return result

    @app.get("/internal/cloud-lan-proxy/storage/capabilities", include_in_schema=False)
    async def storage_capabilities(
        x_cloud_lan_proxy_key: str | None = Header(default=None),
    ) -> dict[str, Any]:
        authorize_key(x_cloud_lan_proxy_key)
        return {
            "ability": True,
            "list": True,
            "download": True,
            "subfile": True,
            "upload": allow_upload,
            "delete": allow_delete,
            "notifications": "revision",
            "time": int(time.time()),
        }

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
