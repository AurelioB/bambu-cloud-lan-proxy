#!/usr/bin/env python3
"""Verify that the local RTSPS facade can DESCRIBE the physical camera."""

from __future__ import annotations

import base64
import os
import socket
import ssl
from pathlib import Path


def load_env(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip()
    return values


def receive_response(connection: ssl.SSLSocket) -> tuple[str, bytes]:
    buffer = bytearray()
    while b"\r\n\r\n" not in buffer:
        chunk = connection.recv(4096)
        if not chunk:
            raise RuntimeError("camera relay closed before the RTSP response headers")
        buffer.extend(chunk)
        if len(buffer) > 1024 * 1024:
            raise RuntimeError("camera relay returned oversized RTSP headers")
    raw_headers, body = bytes(buffer).split(b"\r\n\r\n", 1)
    header_text = raw_headers.decode("iso-8859-1")
    content_length = 0
    for line in header_text.split("\r\n")[1:]:
        if line.lower().startswith("content-length:"):
            content_length = int(line.split(":", 1)[1].strip())
    while len(body) < content_length:
        chunk = connection.recv(min(65536, content_length - len(body)))
        if not chunk:
            raise RuntimeError("camera relay closed during the RTSP response body")
        body += chunk
    return header_text, body[:content_length]


def main() -> None:
    project_dir = Path(__file__).resolve().parent.parent
    values = load_env(project_dir / ".env")
    host = os.getenv("PROXY_BIND_IP", values.get("PROXY_BIND_IP", "127.0.0.2"))
    access_code = os.getenv("PROXY_ACCESS_CODE", values.get("PROXY_ACCESS_CODE", ""))
    if not access_code:
        raise RuntimeError("PROXY_ACCESS_CODE is not configured")
    ca_file = project_dir / "data/proxy/export/bambu-cloud-lan-proxy-ca.crt"
    context = ssl.create_default_context(cafile=str(ca_file))
    credentials = base64.b64encode(f"bblp:{access_code}".encode()).decode("ascii")
    request = (
        f"DESCRIBE rtsps://{host}:322/streaming/live/1 RTSP/1.0\r\n"
        "CSeq: 1\r\n"
        "Accept: application/sdp\r\n"
        f"Authorization: Basic {credentials}\r\n"
        "User-Agent: bambu-cloud-lan-proxy-check\r\n\r\n"
    ).encode("ascii")

    with (
        socket.create_connection((host, 322), timeout=25) as raw,
        context.wrap_socket(raw, server_hostname=host) as connection,
    ):
        connection.settimeout(25)
        connection.sendall(request)
        headers, body = receive_response(connection)
    status = headers.split("\r\n", 1)[0]
    if " 200 " not in status or b"m=video" not in body:
        raise RuntimeError(f"camera DESCRIBE failed: {status}")
    print("Camera relay OK: authenticated RTSPS DESCRIBE returned an SDP video track")


if __name__ == "__main__":
    main()
