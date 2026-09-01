#!/usr/bin/env python3
"""Read-only handshake and media-ability probe for the native file facade."""

from __future__ import annotations

import json
import socket
import ssl
import struct
from pathlib import Path


def access_code() -> str:
    for line in Path(".env").read_text(encoding="utf-8").splitlines():
        if line.startswith("PROXY_ACCESS_CODE="):
            return line.partition("=")[2].strip()
    raise SystemExit("PROXY_ACCESS_CODE is missing from .env")


def main() -> None:
    header = struct.Struct("<IIII")
    sequence = 1
    context = ssl._create_unverified_context()
    with (
        socket.create_connection(("127.0.0.2", 6000), timeout=30) as raw,
        context.wrap_socket(raw, server_hostname="127.0.0.2") as stream,
    ):
        def send(magic: int, payload: bytes) -> None:
            nonlocal sequence
            stream.sendall(header.pack(len(payload), magic, sequence, 0) + payload)
            sequence += 1

        def receive() -> tuple[int, bytes]:
            raw_header = stream.recv(header.size)
            if len(raw_header) != header.size:
                raise RuntimeError("native file service closed early")
            size, magic, _sequence, reserved = header.unpack(raw_header)
            if reserved or size > 2 * 1024 * 1024:
                raise RuntimeError("invalid native response header")
            payload = b""
            while len(payload) < size:
                chunk = stream.recv(size - len(payload))
                if not chunk:
                    raise RuntimeError("truncated native response")
                payload += chunk
            return magic, payload

        login = b"bblp".ljust(8, b"\0") + access_code().encode("ascii").ljust(8, b"\0")
        send(0x0101013F, login)
        assert receive()[0] == 0x0001013F
        send(
            0x0102013F,
            json.dumps(
                {
                    "sequence": 0,
                    "mtype": 0x3003,
                    "req": {"t_av": 1, "mtype": 0x3001, "peer_t": 3},
                },
                separators=(",", ":"),
            ).encode(),
        )
        assert json.loads(receive()[1])["result"] == 0
        send(
            0x0102013F,
            json.dumps(
                {
                    "mtype": 0x3001,
                    "cmdtype": 7,
                    "sequence": 1,
                    "req": {"peer": "studio", "api_version": 2},
                },
                separators=(",", ":"),
            ).encode(),
        )
        response = json.loads(receive()[1])
        assert response["result"] == 0
        assert isinstance(response["reply"]["storage"], list)

        send(
            0x0102013F,
            json.dumps(
                {
                    "mtype": 0x3001,
                    "cmdtype": 1,
                    "sequence": 2,
                    "req": {"api_version": 2, "notify": "DETAIL", "type": "model"},
                },
                separators=(",", ":"),
            ).encode(),
        )
        listing = json.loads(receive()[1])
        assert listing["result"] == 0
        files = listing["reply"]["file_lists"]
        if files and files[0].get("path"):
            send(
                0x0102013F,
                json.dumps(
                    {
                        "mtype": 0x3001,
                        "cmdtype": 2,
                        "sequence": 3,
                        "req": {
                            "paths": [files[0]["path"] + "#_rels/.rels"],
                            "zip": True,
                        },
                    },
                    separators=(",", ":"),
                ).encode(),
            )
            received = bytearray()
            while True:
                frame = receive()[1]
                raw_json, separator, data = frame.partition(b"\n\n")
                result = json.loads(raw_json)
                if separator:
                    received.extend(data)
                if result["result"] == 0:
                    break
            assert received.startswith(b"PK")
    print(
        "Native TLS :6000 ability, model listing, and metadata sub-file probes succeeded."
    )


if __name__ == "__main__":
    main()
