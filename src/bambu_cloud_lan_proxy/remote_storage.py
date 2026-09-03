"""Read-only virtual filesystem backed by the printer's cloud media tunnel."""

from __future__ import annotations

import asyncio
import json
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any


@dataclass(frozen=True, slots=True)
class RemoteFile:
    name: str
    path: str
    size: int
    mtime: float
    kind: str
    storage: str
    serial: str = ""


class RemoteStorageClient:
    """Calls the proxy's authenticated internal storage RPC facade."""

    _MOUNTS = {
        PurePosixPath("/cache"): "model",
        PurePosixPath("/timelapse"): "timelapse",
        PurePosixPath("/video"): "video",
    }

    def __init__(
        self,
        *,
        api_url: str,
        access_code: str,
        serial: str,
        cache_dir: Path,
        ttl: float = 30,
    ) -> None:
        self.api_url = api_url.rstrip("/")
        self.access_code = access_code
        self.serial = serial
        self.cache_dir = cache_dir
        self.ttl = ttl
        self._catalog: dict[PurePosixPath, tuple[float, tuple[RemoteFile, ...]]] = {}
        self._storages: tuple[str, ...] = ("", "internal")
        self._lock = asyncio.Lock()

    @property
    def enabled(self) -> bool:
        return bool(self.api_url and self.access_code and self.serial)

    def _post(self, operation: str, payload: dict[str, Any]) -> dict[str, Any]:
        request = urllib.request.Request(
            f"{self.api_url}/{operation}",
            data=json.dumps(payload).encode(),
            headers={
                "Content-Type": "application/json",
                "X-Cloud-Lan-Proxy-Key": self.access_code,
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=21 * 60) as response:
                value = json.load(response)
        except urllib.error.HTTPError as exc:
            try:
                detail = json.load(exc).get("detail", str(exc))
            except Exception:
                detail = str(exc)
            raise OSError(f"printer storage unavailable: {detail}") from exc
        except (OSError, ValueError) as exc:
            raise OSError(f"printer storage unavailable: {exc}") from exc
        if not isinstance(value, dict):
            raise OSError("printer storage returned an invalid response")
        return value

    async def list(self, directory: PurePosixPath) -> tuple[RemoteFile, ...]:
        kind = self._MOUNTS.get(directory)
        if not self.enabled or kind is None:
            return ()
        now = time.monotonic()
        cached = self._catalog.get(directory)
        if cached and now - cached[0] < self.ttl:
            return cached[1]
        async with self._lock:
            cached = self._catalog.get(directory)
            if cached and time.monotonic() - cached[0] < self.ttl:
                return cached[1]
            merged: dict[str, RemoteFile] = {}
            errors: list[OSError] = []
            for storage in self._storages:
                try:
                    result = await asyncio.to_thread(
                        self._post,
                        "list",
                        {"serial": self.serial, "type": kind, "storage": storage},
                    )
                except OSError as exc:
                    errors.append(exc)
                    continue
                for raw in result.get("files", []):
                    if not isinstance(raw, dict):
                        continue
                    name = str(raw.get("name", "")).replace("\\", "/").rsplit("/", 1)[-1]
                    remote_path = str(raw.get("path", ""))
                    if (
                        not name
                        or name in {".", ".."}
                        or "/" in name
                        or any(char in name for char in ("\x00", "\r", "\n"))
                        or name.startswith((".proxy-", ".upload-"))
                    ):
                        continue
                    try:
                        size = max(0, int(raw.get("size", 0)))
                        mtime = max(0, float(raw.get("time", 0)))
                    except (TypeError, ValueError):
                        continue
                    merged.setdefault(
                        name,
                        RemoteFile(name, remote_path, size, mtime, kind, storage, self.serial),
                    )
            if not merged and len(errors) == len(self._storages):
                raise errors[0]
            files = tuple(sorted(merged.values(), key=lambda item: item.name.lower()))
            self._catalog[directory] = (time.monotonic(), files)
            return files

    async def abilities(self) -> tuple[str, ...]:
        result = await asyncio.to_thread(
            self._post,
            "ability",
            {"serial": self.serial, "api_version": 2},
        )
        labels = tuple(
            str(item)
            for item in result.get("storage", [])
            if isinstance(item, str) and item
        )
        return labels or ("internal",)

    async def list_raw(self, kind: str, storage: str = "") -> tuple[RemoteFile, ...]:
        result = await asyncio.to_thread(
            self._post,
            "list",
            {"serial": self.serial, "type": kind, "storage": storage},
        )
        files: list[RemoteFile] = []
        for raw in result.get("files", []):
            if not isinstance(raw, dict):
                continue
            name = str(raw.get("name", "")).replace("\\", "/").rsplit("/", 1)[-1]
            if not name or name in {".", ".."} or any(c in name for c in ("/", "\0", "\r", "\n")):
                continue
            try:
                files.append(
                    RemoteFile(
                        name=name,
                        path=str(raw.get("path", "")),
                        size=max(0, int(raw.get("size", 0))),
                        mtime=max(0, float(raw.get("time", 0))),
                        kind=kind,
                        storage=storage,
                        serial=self.serial,
                    )
                )
            except (TypeError, ValueError):
                continue
        return tuple(files)

    async def find(self, virtual: PurePosixPath) -> RemoteFile | None:
        if virtual.parent not in self._MOUNTS:
            return None
        return next(
            (entry for entry in await self.list(virtual.parent) if entry.name == virtual.name),
            None,
        )

    async def download(self, entry: RemoteFile) -> Path:
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        result = await asyncio.to_thread(
            self._post,
            "download",
            {
                "serial": self.serial,
                "path": entry.path,
                "name": entry.name,
            },
        )
        cache_key = str(result.get("cache_key", ""))
        if not cache_key.isalnum() or len(cache_key) != 32:
            raise OSError("printer storage returned an invalid cache key")
        target = self.cache_dir / cache_key
        if not target.is_file():
            raise OSError("downloaded printer file is unavailable")
        if target.stat().st_size != entry.size:
            await self.discard(target)
            raise OSError("downloaded printer file has the wrong size")
        return target

    async def download_path(self, *, path: str = "", name: str = "") -> Path:
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        result = await asyncio.to_thread(
            self._post,
            "download",
            {"serial": self.serial, "path": path, "name": name},
        )
        cache_key = str(result.get("cache_key", ""))
        if not cache_key.isalnum() or len(cache_key) != 32:
            raise OSError("printer storage returned an invalid cache key")
        target = self.cache_dir / cache_key
        if not target.is_file():
            raise OSError("downloaded printer file is unavailable")
        return target

    async def discard(self, path: Path) -> None:
        try:
            await asyncio.to_thread(self._post, "discard", {"cache_key": path.name})
        except OSError:
            return

    def invalidate(self) -> None:
        self._catalog.clear()

    async def delete(self, entry: RemoteFile) -> None:
        await asyncio.to_thread(
            self._post,
            "delete",
            {"serial": self.serial, "paths": [entry.path or entry.name]},
        )
        self.invalidate()

    async def upload(self, source: Path, *, path: str, storage: str = "") -> None:
        await asyncio.to_thread(
            self._post,
            "upload",
            {
                "serial": self.serial,
                "source_path": str(source),
                "path": path,
                "storage": storage,
            },
        )
        self.invalidate()

    async def subfile(self, request: dict[str, Any]) -> Path:
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        result = await asyncio.to_thread(
            self._post,
            "subfile",
            {"serial": self.serial, "request": request},
        )
        cache_key = str(result.get("cache_key", ""))
        if not cache_key.isalnum() or len(cache_key) != 32:
            raise OSError("printer storage returned an invalid cache key")
        target = self.cache_dir / cache_key
        if not target.is_file():
            raise OSError("printer subfile is unavailable")
        return target


class RemoteStorageRouter:
    """Retains physical paths for a primary printer and adds serial namespaces."""

    def __init__(self, clients: dict[str, RemoteStorageClient], primary: str) -> None:
        self.clients = clients
        self.primary = primary.upper()

    @property
    def default(self) -> RemoteStorageClient:
        return self.clients.get(self.primary) or next(iter(self.clients.values()))

    @property
    def enabled(self) -> bool:
        return bool(self.clients) and self.default.enabled

    def _route(self, path: PurePosixPath) -> tuple[RemoteStorageClient | None, PurePosixPath]:
        parts = path.parts
        if len(parts) >= 3 and parts[1] == "printers":
            client = self.clients.get(parts[2].upper())
            mapped = PurePosixPath("/", *parts[3:])
            return client, mapped
        return (self.default if self.clients else None), path

    async def list(self, directory: PurePosixPath) -> tuple[RemoteFile, ...]:
        client, mapped = self._route(directory)
        return await client.list(mapped) if client else ()

    async def find(self, virtual: PurePosixPath) -> RemoteFile | None:
        client, mapped = self._route(virtual)
        return await client.find(mapped) if client else None

    async def download(self, entry: RemoteFile) -> Path:
        client = self.clients.get(entry.serial.upper(), self.default)
        return await client.download(entry)

    async def discard(self, path: Path) -> None:
        await self.default.discard(path)

    async def delete(self, entry: RemoteFile) -> None:
        client = self.clients.get(entry.serial.upper(), self.default)
        await client.delete(entry)

    async def upload(
        self, source: Path, *, path: str, storage: str = "", serial: str = ""
    ) -> None:
        client = self.clients.get(serial.upper(), self.default)
        await client.upload(source, path=path, storage=storage)
