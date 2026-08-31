from __future__ import annotations

import asyncio
from pathlib import Path, PurePosixPath

import pytest

from bambu_cloud_lan_proxy.remote_storage import RemoteStorageClient


def test_catalog_merges_internal_and_default_and_caches(tmp_path: Path) -> None:
    calls: list[dict[str, str]] = []
    client = RemoteStorageClient(
        api_url="http://gateway/internal",
        access_code="A1B2C3D4",
        serial="SERIAL",
        cache_dir=tmp_path,
        ttl=60,
    )

    def fake_post(operation: str, payload: dict[str, str]):
        calls.append(payload)
        assert operation == "list"
        if payload["storage"] == "internal":
            return {
                "files": [
                    {"name": "internal.3mf", "path": "/jobs/i.3mf", "size": 12, "time": 2},
                    {"name": "same.3mf", "path": "/internal/same", "size": 99, "time": 3},
                ]
            }
        return {
            "files": [
                {"name": "same.3mf", "path": "/sd/same", "size": 10, "time": 1}
            ]
        }

    client._post = fake_post  # type: ignore[method-assign]

    async def scenario() -> None:
        first = await client.list(PurePosixPath("/cache"))
        second = await client.list(PurePosixPath("/cache"))
        assert first == second
        assert [item.name for item in first] == ["internal.3mf", "same.3mf"]
        assert next(item for item in first if item.name == "same.3mf").size == 10

    asyncio.run(scenario())
    assert len(calls) == 2


def test_download_validates_shared_cache_file(tmp_path: Path) -> None:
    client = RemoteStorageClient(
        api_url="http://gateway/internal",
        access_code="A1B2C3D4",
        serial="SERIAL",
        cache_dir=tmp_path,
    )
    entry_data = {"name": "movie.mp4", "path": "/video/movie.mp4", "size": 4, "time": 1}

    def fake_post(operation: str, _payload: dict[str, str]):
        if operation == "list":
            return {"files": [entry_data]}
        if operation == "discard":
            (tmp_path / ("a" * 32)).unlink(missing_ok=True)
            return {"discarded": True}
        (tmp_path / ("a" * 32)).write_bytes(b"data")
        return {"cache_key": "a" * 32, "size": 4}

    client._post = fake_post  # type: ignore[method-assign]

    async def scenario() -> None:
        entry = (await client.list(PurePosixPath("/video")))[0]
        downloaded = await client.download(entry)
        assert downloaded.read_bytes() == b"data"
        await client.discard(downloaded)
        assert not downloaded.exists()

    asyncio.run(scenario())


def test_download_rejects_wrong_size(tmp_path: Path) -> None:
    client = RemoteStorageClient(
        api_url="http://gateway/internal",
        access_code="A1B2C3D4",
        serial="SERIAL",
        cache_dir=tmp_path,
    )
    target = tmp_path / ("b" * 32)
    target.write_bytes(b"bad")
    def fake_post(operation: str, _payload: dict[str, str]):
        if operation == "discard":
            target.unlink(missing_ok=True)
            return {"discarded": True}
        return {"cache_key": "b" * 32}

    client._post = fake_post  # type: ignore[method-assign]
    entry = type("Entry", (), {"path": "/x", "name": "x", "size": 4})()
    with pytest.raises(OSError, match="wrong size"):
        asyncio.run(client.download(entry))
    assert not target.exists()
