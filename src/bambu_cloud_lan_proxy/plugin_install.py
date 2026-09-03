"""Install only the reviewed official Linux plugin payload; never Gateway.

Hashes are repository pins, not hashes supplied alongside an arbitrary local
binary. An incompatible CDN update fails closed. Existing session data is
never deleted or replaced by installation.
"""

from __future__ import annotations

import hashlib
import io
import os
import platform
import tempfile
import zipfile
from pathlib import Path
from urllib.parse import urlsplit

import httpx

VERSION = "02.05.02.58"
HASHES = {
    "libbambu_networking.so": "c9bdb8e6868c461d5c456dc7ba913744e449e2cc4ce2f29e175b381a1ff95d05",
    "libBambuSource.so": "506a202d225e9efb7e082b7920983bcadb93e85f0927d7b5b43c4dd7bf7d2f21",
}


def api_base(region: str) -> str:
    return {"US": "https://api.bambulab.com", "CN": "https://api.bambulab.cn"}[region]


def studio_headers() -> dict[str, str]:
    return {
        "User-Agent": f"BambuStudio/{VERSION}",
        "X-BBL-Client-Type": "slicer",
        "X-BBL-Client-Name": "BambuStudio",
        "X-BBL-Client-Version": VERSION,
        "X-BBL-OS-Type": "linux",
    }


def validate_binary(name: str, payload: bytes) -> None:
    if payload[:6] != b"\x7fELF\x02\x01" or payload[18:20] != b"\x3e\x00":
        raise ValueError("plugin must be a little-endian x86_64 ELF binary")
    if hashlib.sha256(payload).hexdigest() != HASHES[name]:
        raise ValueError("plugin does not match the reviewed SHA-256 pin")


def unpack_payload(payload: bytes) -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        for member in archive.infolist():
            name = Path(member.filename).name
            if name not in HASHES:
                continue
            if name in files or member.file_size > 128 * 1024 * 1024:
                raise ValueError("duplicate or oversized plugin binary")
            data = archive.read(member)
            validate_binary(name, data)
            files[name] = data
    if files.keys() != HASHES.keys():
        raise ValueError("plugin archive is incomplete")
    return files


async def ensure_plugin(directory: Path, region: str) -> None:
    if platform.machine() not in {"x86_64", "AMD64"}:
        raise ValueError("the pinned Bambu plugin requires x86_64 Linux")
    if all((directory / name).is_file() for name in HASHES):
        for name in HASHES:
            validate_binary(name, (directory / name).read_bytes())
        return
    async with httpx.AsyncClient(timeout=60) as client:
        response = await client.get(
            api_base(region) + "/v1/iot-service/api/slicer/resource"
            "?slicer/plugins/cloud=02.05.02.00",
            headers=studio_headers(),
        )
        response.raise_for_status()
        entries = [
            r
            for r in response.json().get("resources", [])
            if r.get("type") == "slicer/plugins/cloud" and r.get("version") == VERSION
        ]
        if len(entries) != 1:
            raise ValueError("Bambu no longer advertises the reviewed plugin version")
        url = entries[0]["url"]
        if urlsplit(url).scheme != "https":
            raise ValueError("plugin download requires HTTPS")
        blob = bytearray()
        async with client.stream("GET", url) as download:
            download.raise_for_status()
            async for chunk in download.aiter_bytes():
                blob.extend(chunk)
                if len(blob) > 128 * 1024 * 1024:
                    raise ValueError("plugin download exceeds size limit")
    files = unpack_payload(bytes(blob))
    directory.mkdir(parents=True, exist_ok=True)
    for name, payload in files.items():
        with tempfile.NamedTemporaryFile(dir=directory, delete=False) as temporary:
            temporary.write(payload)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.chmod(temporary.name, 0o600)
        os.replace(temporary.name, directory / name)
