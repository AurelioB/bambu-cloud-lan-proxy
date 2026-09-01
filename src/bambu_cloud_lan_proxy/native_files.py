"""BambuTunnelLocal-compatible TLS file facade on TCP port 6000."""

from __future__ import annotations

import asyncio
import hashlib
import hmac
import json
import logging
import os
import ssl
import struct
import uuid
from contextlib import suppress
from pathlib import Path
from typing import Any

logger = logging.getLogger("bambu.cloud_lan_proxy.native_files")

_HEADER = struct.Struct("<IIII")
_CLIENT_LOGIN = 0x0101013F
_SERVER_LOGIN = 0x0001013F
_CLIENT_CTRL = 0x0102013F
_SERVER_CTRL = 0x0002013F
_MAX_FRAME = 2 * 1024 * 1024


class NativeFileSession:
    def __init__(
        self,
        server: NativeFileServer,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        self.server = server
        self.reader = reader
        self.writer = writer
        self.send_sequence = 1
        self.upload: dict[str, Any] | None = None

    async def read_frame(self) -> tuple[int, int, bytes]:
        header = await self.reader.readexactly(_HEADER.size)
        size, magic, sequence, reserved = _HEADER.unpack(header)
        if reserved != 0 or size > _MAX_FRAME:
            raise OSError("invalid native file frame")
        return magic, sequence, await self.reader.readexactly(size)

    async def send_frame(self, magic: int, payload: bytes) -> None:
        self.writer.write(_HEADER.pack(len(payload), magic, self.send_sequence, 0))
        self.send_sequence += 1
        self.writer.write(payload)
        await self.writer.drain()

    async def reply(
        self,
        *,
        command: int,
        sequence: int,
        result: int = 0,
        reply: dict[str, Any] | None = None,
        data: bytes = b"",
    ) -> None:
        root = {
            "mtype": 0x3001,
            "cmdtype": command,
            "sequence": sequence,
            "result": result,
            "reply": reply or {},
        }
        payload = json.dumps(root, separators=(",", ":")).encode()
        if data:
            payload += b"\n\n" + data
        await self.send_frame(_SERVER_CTRL, payload)

    async def run(self) -> None:
        peer = self.writer.get_extra_info("peername")
        try:
            magic, _seq, payload = await asyncio.wait_for(self.read_frame(), 15)
            if magic != _CLIENT_LOGIN or len(payload) != 16:
                raise OSError("invalid login frame")
            user = payload[:8].rstrip(b"\0").decode("ascii", "ignore")
            password = payload[8:16].rstrip(b"\0").decode("ascii", "ignore")
            if user != "bblp" or not hmac.compare_digest(password, self.server.access_code):
                raise OSError("login rejected")
            await self.send_frame(_SERVER_LOGIN, b"\0\0\0\0")

            magic, _seq, payload = await asyncio.wait_for(self.read_frame(), 15)
            setup = json.loads(payload)
            if magic != _CLIENT_CTRL or int(setup.get("mtype", 0)) != 0x3003:
                raise OSError("invalid CTRL setup")
            setup_reply = {
                "mtype": 0x3003,
                "sequence": setup.get("sequence", 0),
                "result": 0,
                "reply": {},
            }
            await self.send_frame(
                _SERVER_CTRL,
                json.dumps(setup_reply, separators=(",", ":")).encode(),
            )

            while True:
                magic, _seq, payload = await self.read_frame()
                if magic != _CLIENT_CTRL:
                    raise OSError("invalid CTRL frame")
                raw_json, separator, data = payload.partition(b"\n\n")
                request = json.loads(raw_json)
                command = int(request.get("cmdtype", 0))
                sequence = int(request.get("sequence", 0))
                body = request.get("req", {})
                if not isinstance(body, dict):
                    await self.reply(command=command, sequence=sequence, result=8)
                    continue
                await self.handle(command, sequence, body, data if separator else b"")
        except (
            asyncio.IncompleteReadError,
            ConnectionError,
            OSError,
            ssl.SSLError,
            TimeoutError,
            ValueError,
            json.JSONDecodeError,
        ) as exc:
            logger.info("native file session %s ended: %s", peer, exc)
        finally:
            if self.upload is not None:
                Path(self.upload["partial"]).unlink(missing_ok=True)
            self.writer.close()
            with suppress(OSError, TimeoutError):
                await asyncio.wait_for(self.writer.wait_closed(), 5)

    async def handle(self, command: int, sequence: int, body: dict[str, Any], data: bytes) -> None:
        try:
            if command == 7:
                labels = await self.server.remote.abilities()
                await self.reply(
                    command=command,
                    sequence=sequence,
                    reply={"storage": list(labels)},
                )
            elif command == 1:
                await self.list_files(sequence, body)
            elif command == 2:
                await self.binary_file(command, sequence, await self.server.remote.subfile(body))
            elif command == 4:
                await self.binary_file(
                    command,
                    sequence,
                    await self.server.remote.download_path(
                        path=str(body.get("path", "")), name=str(body.get("file", ""))
                    ),
                )
            elif command == 3:
                await self.delete_files(sequence, body)
            elif command == 5:
                await self.upload_file(sequence, body, data)
            elif command == 0x1000:
                await self.cancel(sequence, body)
            else:
                await self.reply(command=command, sequence=sequence, result=18)
        except OSError as exc:
            logger.warning("native command=%d failed: %s", command, exc)
            await self.reply(command=command, sequence=sequence, result=17)

    async def list_files(self, sequence: int, body: dict[str, Any]) -> None:
        kind = str(body.get("type", ""))
        storage = str(body.get("storage", ""))
        if kind not in {"model", "timelapse", "video"}:
            await self.reply(command=1, sequence=sequence, result=15)
            return
        files = await self.server.remote.list_raw(kind, storage)
        payload = [
            {"name": item.name, "path": item.path, "size": item.size, "time": int(item.mtime)}
            for item in files
        ]
        await self.reply(command=1, sequence=sequence, reply={"file_lists": payload})

    async def binary_file(self, command: int, sequence: int, source: Path) -> None:
        try:
            total = source.stat().st_size
            digest = await asyncio.to_thread(self._md5, source)
            offset = 0
            with source.open("rb") as stream:
                while True:
                    data = stream.read(512 * 1024)
                    if not data:
                        if total == 0:
                            await self.reply(
                                command=command,
                                sequence=sequence,
                                reply={"offset": 0, "size": 0, "total": 0, "file_md5": digest},
                            )
                        break
                    final = offset + len(data) == total
                    reply = {"offset": offset, "size": len(data), "total": total}
                    if final:
                        reply["file_md5"] = digest
                    await self.reply(
                        command=command,
                        sequence=sequence,
                        result=0 if final else 1,
                        reply=reply,
                        data=data,
                    )
                    offset += len(data)
        finally:
            await self.server.remote.discard(source)

    @staticmethod
    def _md5(path: Path) -> str:
        digest = hashlib.md5(usedforsecurity=False)
        with path.open("rb") as stream:
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk)
        return digest.hexdigest()

    async def delete_files(self, sequence: int, body: dict[str, Any]) -> None:
        raw_paths = body.get("paths")
        if not isinstance(raw_paths, list):
            raw_paths = [body.get("path") or body.get("file")]
        paths = [str(value) for value in raw_paths if value]
        if not paths:
            await self.reply(command=3, sequence=sequence, result=10)
            return
        # Use the authenticated, separately gated internal API.
        await asyncio.to_thread(
            self.server.remote._post,  # noqa: SLF001 - same trust boundary
            "delete",
            {"serial": self.server.remote.serial, "paths": paths},
        )
        self.server.remote.invalidate()
        await self.reply(command=3, sequence=sequence)
        await self.notify_change()

    async def upload_file(self, sequence: int, body: dict[str, Any], data: bytes) -> None:
        if not data:
            total = int(body.get("total", 0))
            name = Path(str(body.get("path", ""))).name
            if total <= 0 or total > self.server.max_upload_bytes or not name:
                await self.reply(command=5, sequence=sequence, result=21)
                return
            partial = self.server.staging_root / f".proxy-native-{uuid.uuid4().hex}.part"
            self.upload = {
                "sequence": sequence,
                "partial": partial,
                "name": name,
                "storage": str(body.get("storage", "")),
                "total": total,
                "offset": 0,
                "md5": hashlib.md5(usedforsecurity=False),
            }
            partial.parent.mkdir(parents=True, exist_ok=True)
            partial.touch(mode=0o600)
            await self.reply(
                command=5,
                sequence=sequence,
                result=1,
                reply={"chunk_size": 255, "offset": 0},
            )
            return

        state = self.upload
        if state is None or state["sequence"] != sequence:
            await self.reply(command=5, sequence=sequence, result=8)
            return
        offset = int(body.get("offset", -1))
        size = int(body.get("size", -1))
        if offset != state["offset"] or size != len(data) or offset + size > state["total"]:
            raise OSError("upload frame mismatch")
        with Path(state["partial"]).open("ab") as stream:
            stream.write(data)
        state["md5"].update(data)
        state["offset"] += size
        if state["offset"] != state["total"]:
            return
        expected = str(body.get("file_md5", "")).lower()
        actual = state["md5"].hexdigest()
        if expected and not hmac.compare_digest(expected, actual):
            raise OSError("upload checksum mismatch")
        destination = self.server.staging_root / "cache" / state["name"]
        destination.parent.mkdir(parents=True, exist_ok=True)
        os.replace(state["partial"], destination)
        await self.server.remote.upload(
            destination, path=state["name"], storage=state["storage"]
        )
        self.upload = None
        await self.reply(command=5, sequence=sequence)
        await self.notify_change()

    async def cancel(self, sequence: int, body: dict[str, Any]) -> None:
        tasks = body.get("tasks", [])
        if self.upload is not None and self.upload["sequence"] in tasks:
            Path(self.upload["partial"]).unlink(missing_ok=True)
            self.upload = None
        await self.reply(command=0x1000, sequence=sequence, reply={"tasks": tasks})

    async def notify_change(self) -> None:
        payload = json.dumps(
            {"mtype": 0x3001, "cmdtype": 0x100, "sequence": 0, "notify": {}},
            separators=(",", ":"),
        ).encode()
        await self.send_frame(_SERVER_CTRL, payload)


class NativeFileServer:
    def __init__(
        self,
        *,
        bind_host: str,
        port: int,
        ssl_context: ssl.SSLContext,
        access_code: str,
        remote: Any,
        staging_root: Path,
        max_upload_bytes: int,
    ) -> None:
        self.bind_host = bind_host
        self.port = port
        self.ssl_context = ssl_context
        self.access_code = access_code
        self.remote = remote
        self.staging_root = staging_root
        self.max_upload_bytes = max_upload_bytes

    async def serve(self) -> None:
        server = await asyncio.start_server(
            lambda reader, writer: NativeFileSession(self, reader, writer).run(),
            self.bind_host,
            self.port,
            ssl=self.ssl_context,
        )
        logger.info("native Bambu file service listening on %s:%d", self.bind_host, self.port)
        async with server:
            await server.serve_forever()


def main() -> None:
    from .ftps_server import FTPSConfig, StagingStore
    from .remote_storage import RemoteStorageClient

    logging.basicConfig(
        level=os.getenv("LOG_LEVEL", "INFO").upper(),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    config = FTPSConfig.from_env()
    store = StagingStore(config)
    store.initialize()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(config.cert_file, config.key_file)
    remote = RemoteStorageClient(
        api_url=config.remote_api_url,
        access_code=config.access_code,
        serial=config.remote_serial,
        cache_dir=config.remote_cache_dir,
        ttl=config.remote_catalog_ttl,
    )
    server = NativeFileServer(
        bind_host=os.getenv("NATIVE_FILES_BIND_HOST", "0.0.0.0"),  # nosec B104
        port=int(os.getenv("NATIVE_FILES_PORT", "6000")),
        ssl_context=context,
        access_code=config.access_code,
        remote=remote,
        staging_root=config.root,
        max_upload_bytes=config.max_upload_bytes,
    )
    with suppress(KeyboardInterrupt):
        asyncio.run(server.serve())


if __name__ == "__main__":
    main()
