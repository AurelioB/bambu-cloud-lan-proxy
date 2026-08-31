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


class RemoteStorageClient:
    """Calls Gateway's loopback-only authenticated storage RPC facade."""

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
            for storage in ("", "internal"):
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
                        RemoteFile(name, remote_path, size, mtime, kind, storage),
                    )
            if not merged and len(errors) == 2:
                raise errors[0]
            files = tuple(sorted(merged.values(), key=lambda item: item.name.lower()))
            self._catalog[directory] = (time.monotonic(), files)
            return files

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

    async def discard(self, path: Path) -> None:
        try:
            await asyncio.to_thread(
                self._post, "discard", {"cache_key": path.name}
            )
        except OSError:
            # A failed cleanup must not turn an otherwise successful FTP
            # transfer into a protocol failure. Startup cleanup handles debris.
            return
