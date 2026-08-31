"""Small implicit-FTPS staging server shaped like a Bambu printer."""

from __future__ import annotations

import asyncio
import hmac
import ipaddress
import logging
import os
import re
import shutil
import ssl
import stat as stat_module
import time
import uuid
import zipfile
from contextlib import suppress
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath

logger = logging.getLogger("bambu.cloud_lan_proxy.ftps")
_ACCESS_CODE_RE = re.compile(r"[A-Za-z0-9]{8}\Z")
_INTERNAL_PREFIXES = (".upload-", ".proxy-")


class StorageQuotaError(OSError):
    """The configured staging quota would be exceeded."""


def _positive_int(name: str, default: int) -> int:
    raw = os.getenv(name, str(default))
    try:
        value = int(raw)
    except ValueError as exc:
        raise ValueError(f"{name} must be an integer") from exc
    if value <= 0:
        raise ValueError(f"{name} must be positive")
    return value


@dataclass(frozen=True, slots=True)
class FTPSConfig:
    bind_host: str
    port: int
    advertise_ip: str
    passive_min: int
    passive_max: int
    root: Path
    access_code: str
    cert_file: Path
    key_file: Path
    max_upload_bytes: int
    max_total_bytes: int
    idle_timeout: float

    @classmethod
    def from_env(cls) -> FTPSConfig:
        access_code = os.getenv("FTPS_ACCESS_CODE", "")
        if not _ACCESS_CODE_RE.fullmatch(access_code):
            raise ValueError("FTPS_ACCESS_CODE must be exactly eight letters or digits")

        advertise_ip = os.getenv("FTPS_ADVERTISE_IP", "127.0.0.2")
        try:
            ipaddress.IPv4Address(advertise_ip)
        except ipaddress.AddressValueError as exc:
            raise ValueError("FTPS_ADVERTISE_IP must be an IPv4 address") from exc

        port = _positive_int("FTPS_PORT", 9990)
        passive_min = _positive_int("FTPS_PASSIVE_MIN", 50000)
        passive_max = _positive_int("FTPS_PASSIVE_MAX", 50009)
        for name, value in (
            ("FTPS_PORT", port),
            ("FTPS_PASSIVE_MIN", passive_min),
            ("FTPS_PASSIVE_MAX", passive_max),
        ):
            if value > 65535:
                raise ValueError(f"{name} must be at most 65535")
        if passive_min > passive_max:
            raise ValueError("FTPS_PASSIVE_MIN must not exceed FTPS_PASSIVE_MAX")

        max_upload_mb = _positive_int("FTPS_MAX_UPLOAD_MB", 1024)
        max_total_mb = _positive_int("FTPS_MAX_TOTAL_MB", 10240)
        if max_total_mb < max_upload_mb:
            raise ValueError("FTPS_MAX_TOTAL_MB must be at least FTPS_MAX_UPLOAD_MB")

        idle_timeout = float(os.getenv("FTPS_IDLE_TIMEOUT", "300"))
        if idle_timeout <= 0:
            raise ValueError("FTPS_IDLE_TIMEOUT must be positive")

        return cls(
            bind_host=os.getenv("FTPS_BIND_HOST", "0.0.0.0"),  # nosec B104
            port=port,
            advertise_ip=advertise_ip,
            passive_min=passive_min,
            passive_max=passive_max,
            root=Path(os.getenv("FTPS_ROOT", "/staging")),
            access_code=access_code,
            cert_file=Path(os.getenv("FTPS_CERT_FILE", "/secrets/certs/server.crt")),
            key_file=Path(os.getenv("FTPS_KEY_FILE", "/secrets/certs/server.key")),
            max_upload_bytes=max_upload_mb * 1024 * 1024,
            max_total_bytes=max_total_mb * 1024 * 1024,
            idle_timeout=idle_timeout,
        )


class StagingStore:
    """Confined filesystem plus atomic upload/quota operations."""

    def __init__(self, config: FTPSConfig) -> None:
        self.root = config.root
        self.root_resolved = config.root.resolve()
        self.max_upload_bytes = config.max_upload_bytes
        self.max_total_bytes = config.max_total_bytes
        self._commit_lock = asyncio.Lock()

    def initialize(self) -> None:
        self.root.mkdir(parents=True, exist_ok=True)
        for relative in ("cache", "timelapse", "model", "data", "data/Metadata"):
            (self.root / relative).mkdir(parents=True, exist_ok=True)
        cutoff = time.time() - 24 * 60 * 60
        for path in self.root.rglob(".upload-*.part"):
            try:
                if path.is_file() and path.stat().st_mtime < cutoff:
                    path.unlink()
            except OSError:
                logger.warning("could not clean stale partial upload %s", path)

    def used_bytes(self) -> int:
        total = 0
        for path in self.root.rglob("*"):
            try:
                if (
                    path.is_file()
                    and not path.is_symlink()
                    and not path.name.startswith(_INTERNAL_PREFIXES)
                ):
                    total += path.stat().st_size
            except OSError:
                continue
        return total

    async def commit(self, partial: Path, destination: Path) -> None:
        async with self._commit_lock:
            partial_size = partial.stat().st_size
            old_size = destination.stat().st_size if destination.is_file() else 0
            projected = self.used_bytes() - old_size + partial_size
            if projected > self.max_total_bytes:
                raise StorageQuotaError("staging storage quota exceeded")
            os.replace(partial, destination)
            destination.chmod(0o644)


class FTPSSession:
    def __init__(
        self,
        server: FTPSStagingServer,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        self.server = server
        self.config = server.config
        self.store = server.store
        self.reader = reader
        self.writer = writer
        peer = writer.get_extra_info("peername")
        self.remote_ip = str(peer[0]) if peer else "unknown"
        self.username = ""
        self.authenticated = False
        self.current_dir = PurePosixPath("/")
        self.transfer_type = "A"
        self.private_data = True
        self.restart_offset = 0
        self.rename_from: Path | None = None
        self.passive_server: asyncio.Server | None = None
        self.data_reader: asyncio.StreamReader | None = None
        self.data_writer: asyncio.StreamWriter | None = None
        self.data_ready = asyncio.Event()
        self.data_finished = asyncio.Event()
        self.closing = False

    async def run(self) -> None:
        tls = self.writer.get_extra_info("ssl_object")
        logger.debug(
            "control connection from %s tls=%s",
            self.remote_ip,
            tls.version() if tls else "none",
        )
        await self.reply(220, "Bambu Cloud LAN Proxy FTPS ready")
        try:
            while not self.closing:
                try:
                    line = await asyncio.wait_for(
                        self.reader.readline(), timeout=self.config.idle_timeout
                    )
                except TimeoutError:
                    await self.reply(421, "Control connection timed out")
                    break
                if not line:
                    break
                if len(line) > 16384:
                    await self.reply(500, "Command line too long")
                    break
                try:
                    decoded = line.decode("utf-8").rstrip("\r\n")
                except UnicodeDecodeError:
                    decoded = line.decode("latin-1").rstrip("\r\n")
                if not decoded:
                    continue
                command, _, argument = decoded.partition(" ")
                command = command.upper()
                handler = getattr(self, f"cmd_{command}", None)
                if handler is None:
                    await self.reply(502, f"Command {command} not implemented")
                    continue
                if command == "PASS":
                    logger.debug("%s PASS ********", self.remote_ip)
                else:
                    logger.debug("%s %s %s", self.remote_ip, command, argument)
                try:
                    await handler(argument)
                except (OSError, ValueError) as exc:
                    logger.warning("%s command %s failed: %s", self.remote_ip, command, exc)
                    await self.reply(550, str(exc) or "Requested operation failed")
        except (ConnectionError, ssl.SSLError, asyncio.IncompleteReadError):
            logger.debug("control connection closed abruptly by %s", self.remote_ip)
        finally:
            await self.close_data()
            self.writer.close()
            with suppress(OSError, TimeoutError):
                await asyncio.wait_for(self.writer.wait_closed(), timeout=5)
            logger.debug("control connection ended from %s", self.remote_ip)

    async def reply(self, code: int, message: str) -> None:
        self.writer.write(f"{code} {message}\r\n".encode())
        await self.writer.drain()

    async def multiline(self, code: int, lines: list[str]) -> None:
        if not lines:
            await self.reply(code, "")
            return
        self.writer.write(f"{code}-{lines[0]}\r\n".encode())
        for line in lines[1:]:
            self.writer.write(f" {line}\r\n".encode())
        self.writer.write(f"{code} End\r\n".encode())
        await self.writer.drain()

    async def require_auth(self) -> bool:
        if self.authenticated:
            return True
        await self.reply(530, "Not logged in")
        return False

    def virtual_path(self, argument: str | None) -> PurePosixPath:
        raw = (argument or "").strip()
        if not raw:
            return self.current_dir
        base: list[str] = [] if raw.startswith("/") else list(self.current_dir.parts[1:])
        for part in PurePosixPath(raw.replace("\\", "/")).parts:
            if part in {"", "/", "."}:
                continue
            if part == "..":
                if base:
                    base.pop()
                continue
            if "\x00" in part or part.startswith(_INTERNAL_PREFIXES):
                raise ValueError("Invalid path")
            base.append(part)
        return PurePosixPath("/", *base)

    def real_path(self, virtual: PurePosixPath) -> Path:
        candidate = self.store.root.joinpath(*virtual.parts[1:])
        resolved = candidate.resolve(strict=False)
        if not resolved.is_relative_to(self.store.root_resolved):
            raise ValueError("Path escapes staging storage")
        return candidate

    async def reset_passive(self) -> None:
        await self.close_data()
        self.data_ready = asyncio.Event()
        self.data_finished = asyncio.Event()
        self.data_reader = None
        self.data_writer = None

    async def on_data_connection(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        peer = writer.get_extra_info("peername")
        data_ip = str(peer[0]) if peer else "unknown"
        if data_ip != self.remote_ip:
            logger.warning(
                "rejected passive data connection from %s for control peer %s",
                data_ip,
                self.remote_ip,
            )
            writer.close()
            with suppress(OSError, ConnectionError, TimeoutError, ssl.SSLError):
                await asyncio.wait_for(writer.wait_closed(), timeout=5)
            return
        if self.data_writer is not None:
            writer.close()
            await writer.wait_closed()
            return
        self.data_reader = reader
        self.data_writer = writer
        if self.passive_server is not None:
            self.passive_server.close()
        self.data_ready.set()
        await self.data_finished.wait()

    async def begin_transfer(self, description: str) -> bool:
        if self.passive_server is None and not self.data_ready.is_set():
            await self.reply(425, "Use PASV or EPSV first")
            return False
        await self.reply(150, f"Opening data connection for {description}")
        try:
            await asyncio.wait_for(self.data_ready.wait(), timeout=30)
        except TimeoutError:
            await self.reply(425, "Data connection timed out")
            await self.close_data()
            return False
        return self.data_reader is not None and self.data_writer is not None

    async def close_data(self) -> None:
        self.data_finished.set()
        if self.data_writer is not None:
            self.data_writer.close()
            with suppress(OSError, ConnectionError, TimeoutError, ssl.SSLError):
                await asyncio.wait_for(self.data_writer.wait_closed(), timeout=5)
        if self.passive_server is not None:
            self.passive_server.close()
            with suppress(OSError):
                await self.passive_server.wait_closed()
        self.data_reader = None
        self.data_writer = None
        self.passive_server = None

    async def send_data(self, payload: bytes) -> bool:
        if self.data_writer is None:
            return False
        try:
            self.data_writer.write(payload)
            await self.data_writer.drain()
            return True
        except (ConnectionError, OSError, ssl.SSLError):
            return False

    async def cmd_USER(self, argument: str) -> None:
        self.username = argument
        self.authenticated = False
        if argument.lower() == "bblp":
            await self.reply(331, "Password required")
        else:
            await self.reply(530, "Invalid user")

    async def cmd_PASS(self, argument: str) -> None:
        if self.username.lower() != "bblp":
            await self.reply(503, "Login with USER first")
        elif hmac.compare_digest(argument, self.config.access_code):
            self.authenticated = True
            await self.reply(230, "Login successful")
            logger.info("login accepted from %s", self.remote_ip)
        else:
            await self.reply(530, "Login incorrect")
            logger.warning("login rejected from %s", self.remote_ip)

    async def cmd_QUIT(self, _argument: str) -> None:
        await self.reply(221, "Goodbye")
        self.closing = True

    async def cmd_SYST(self, _argument: str) -> None:
        await self.reply(215, "UNIX Type: L8")

    async def cmd_FEAT(self, _argument: str) -> None:
        await self.multiline(
            211,
            [
                "Features",
                "EPSV",
                "PASV",
                "PBSZ",
                "PROT",
                "REST STREAM",
                "SIZE",
                "MDTM",
                "MLST type*;size*;modify*;",
                "UTF8",
            ],
        )

    async def cmd_AUTH(self, _argument: str) -> None:
        await self.reply(503, "TLS is already active (implicit FTPS)")

    async def cmd_PBSZ(self, argument: str) -> None:
        if argument.strip() != "0":
            await self.reply(501, "PBSZ must be 0")
        else:
            await self.reply(200, "PBSZ=0")

    async def cmd_PROT(self, argument: str) -> None:
        level = argument.upper().strip()
        if level == "P":
            self.private_data = True
            await self.reply(200, "Data channel protection is private")
        elif level == "C":
            self.private_data = False
            await self.reply(200, "Data channel protection is clear")
        else:
            await self.reply(504, "Unsupported protection level")

    async def cmd_OPTS(self, argument: str) -> None:
        if argument.upper().startswith("UTF8"):
            await self.reply(200, "UTF8 enabled")
        else:
            await self.reply(501, "Option not supported")

    async def cmd_CLNT(self, _argument: str) -> None:
        await self.reply(200, "Client noted")

    async def cmd_NOOP(self, _argument: str) -> None:
        await self.reply(200, "OK")

    async def cmd_TYPE(self, argument: str) -> None:
        if not await self.require_auth():
            return
        value = argument.upper().strip()
        if value not in {"A", "I", "L 8"}:
            await self.reply(504, "Type not supported")
            return
        self.transfer_type = "I" if value in {"I", "L 8"} else "A"
        await self.reply(200, f"Type set to {self.transfer_type}")

    async def cmd_MODE(self, argument: str) -> None:
        await self.reply(200 if argument.upper() == "S" else 504, "Stream mode")

    async def cmd_STRU(self, argument: str) -> None:
        await self.reply(200 if argument.upper() == "F" else 504, "File structure")

    async def cmd_PWD(self, _argument: str) -> None:
        if await self.require_auth():
            await self.reply(257, f'"{self.current_dir}" is current directory')

    async def cmd_CWD(self, argument: str) -> None:
        if not await self.require_auth():
            return
        virtual = self.virtual_path(argument)
        if not self.real_path(virtual).is_dir():
            await self.reply(550, "Directory not found")
            return
        self.current_dir = virtual
        await self.reply(250, "Directory changed")

    async def cmd_CDUP(self, _argument: str) -> None:
        await self.cmd_CWD("..")

    async def cmd_PASV(self, _argument: str) -> None:
        if not await self.require_auth():
            return
        await self.reset_passive()
        port = await self.server.open_passive(self)
        if port is None:
            await self.reply(425, "No passive data port is available")
            return
        octets = self.config.advertise_ip.split(".")
        await self.reply(
            227,
            f"Entering Passive Mode ({','.join(octets)},{port // 256},{port % 256})",
        )

    async def cmd_EPSV(self, argument: str) -> None:
        if not await self.require_auth():
            return
        if argument.upper().strip() == "ALL":
            await self.reply(200, "EPSV ALL accepted")
            return
        await self.reset_passive()
        port = await self.server.open_passive(self)
        if port is None:
            await self.reply(425, "No passive data port is available")
            return
        await self.reply(229, f"Entering Extended Passive Mode (|||{port}|)")

    async def cmd_PORT(self, _argument: str) -> None:
        await self.reply(502, "Active mode is not supported")

    async def cmd_EPRT(self, _argument: str) -> None:
        await self.reply(502, "Active mode is not supported")

    async def cmd_STOR(self, argument: str) -> None:
        if not await self.require_auth():
            return
        virtual = self.virtual_path(argument)
        destination = self.real_path(virtual)
        if destination.name in {"", ".", ".."} or not destination.parent.is_dir():
            await self.reply(550, "Destination directory does not exist")
            await self.close_data()
            return
        if not await self.begin_transfer(virtual.name):
            return

        partial = destination.parent / f".upload-{uuid.uuid4().hex}.part"
        received = 0
        failure: Exception | None = None
        try:
            with partial.open("xb") as output:
                while True:
                    assert self.data_reader is not None
                    chunk = await asyncio.wait_for(self.data_reader.read(65536), timeout=60)
                    if not chunk:
                        break
                    received += len(chunk)
                    if received > self.store.max_upload_bytes:
                        raise StorageQuotaError("single-file upload limit exceeded")
                    output.write(chunk)
                output.flush()
                os.fsync(output.fileno())
            if virtual.name.lower().endswith(".3mf"):
                with zipfile.ZipFile(partial) as archive:
                    archive.infolist()
            await self.store.commit(partial, destination)
        except Exception as exc:
            failure = exc
            partial.unlink(missing_ok=True)
        await self.close_data()
        if failure is None:
            await self.reply(226, "Transfer complete")
            logger.info("stored %s (%d bytes) from %s", virtual, received, self.remote_ip)
        elif isinstance(failure, StorageQuotaError):
            await self.reply(552, str(failure))
        else:
            await self.reply(426, f"Transfer failed: {failure}")

    async def cmd_RETR(self, argument: str) -> None:
        if not await self.require_auth():
            return
        virtual = self.virtual_path(argument)
        source = self.real_path(virtual)
        if not source.is_file():
            await self.reply(550, "File not found")
            await self.close_data()
            return
        if not await self.begin_transfer(virtual.name):
            return
        sent = 0
        failure = False
        try:
            with source.open("rb") as input_file:
                if self.restart_offset:
                    input_file.seek(self.restart_offset)
                while chunk := input_file.read(65536):
                    if not await self.send_data(chunk):
                        failure = True
                        break
                    sent += len(chunk)
        except OSError:
            failure = True
        finally:
            self.restart_offset = 0
            await self.close_data()
        await self.reply(
            426 if failure else 226,
            "Transfer failed" if failure else "Transfer complete",
        )
        if not failure:
            logger.info("served %s (%d bytes) to %s", virtual, sent, self.remote_ip)

    @staticmethod
    def _listing_argument(argument: str) -> str:
        tokens = argument.split()
        while tokens and tokens[0].startswith("-"):
            tokens.pop(0)
        return " ".join(tokens)

    def _entries(self, argument: str) -> list[Path]:
        virtual = self.virtual_path(self._listing_argument(argument))
        target = self.real_path(virtual)
        if target.is_dir():
            entries = list(target.iterdir())
        elif target.exists():
            entries = [target]
        else:
            raise FileNotFoundError("Path not found")
        return sorted(
            (
                entry
                for entry in entries
                if not entry.is_symlink() and not entry.name.startswith(_INTERNAL_PREFIXES)
            ),
            key=lambda entry: entry.name.lower(),
        )

    @staticmethod
    def _list_line(path: Path) -> str:
        details = path.stat()
        mode = stat_module.filemode(details.st_mode)
        stamp = datetime.fromtimestamp(details.st_mtime)
        if abs(time.time() - details.st_mtime) < 180 * 24 * 60 * 60:
            rendered_time = stamp.strftime("%b %d %H:%M")
        else:
            rendered_time = stamp.strftime("%b %d  %Y")
        size = details.st_size if path.is_file() else 0
        return f"{mode} 1 bblp bblp {size} {rendered_time} {path.name}"

    async def _send_listing(self, argument: str, *, machine: bool, names: bool) -> None:
        if not await self.require_auth():
            return
        try:
            entries = self._entries(argument)
        except FileNotFoundError:
            await self.reply(550, "Path not found")
            await self.close_data()
            return
        if not await self.begin_transfer("directory listing"):
            return
        lines: list[str] = []
        for entry in entries:
            if names:
                lines.append(entry.name)
            elif machine:
                details = entry.stat()
                kind = "dir" if entry.is_dir() else "file"
                modified = datetime.fromtimestamp(details.st_mtime, UTC).strftime(
                    "%Y%m%d%H%M%S"
                )
                lines.append(
                    f"type={kind};size={details.st_size if entry.is_file() else 0};"
                    f"modify={modified}; {entry.name}"
                )
            else:
                lines.append(self._list_line(entry))
        ok = await self.send_data(("\r\n".join(lines) + ("\r\n" if lines else "")).encode())
        await self.close_data()
        await self.reply(226 if ok else 426, "Transfer complete" if ok else "Transfer failed")

    async def cmd_LIST(self, argument: str) -> None:
        await self._send_listing(argument, machine=False, names=False)

    async def cmd_NLST(self, argument: str) -> None:
        await self._send_listing(argument, machine=False, names=True)

    async def cmd_MLSD(self, argument: str) -> None:
        await self._send_listing(argument, machine=True, names=False)

    async def cmd_MLST(self, argument: str) -> None:
        if not await self.require_auth():
            return
        target = self.real_path(self.virtual_path(argument))
        if not target.exists():
            await self.reply(550, "Path not found")
            return
        details = target.stat()
        kind = "dir" if target.is_dir() else "file"
        modified = datetime.fromtimestamp(details.st_mtime, UTC).strftime("%Y%m%d%H%M%S")
        await self.multiline(
            250,
            [
                "Listing",
                f"type={kind};size={details.st_size if target.is_file() else 0};"
                f"modify={modified}; {target.name}",
            ],
        )

    async def cmd_SIZE(self, argument: str) -> None:
        if not await self.require_auth():
            return
        target = self.real_path(self.virtual_path(argument))
        if not target.is_file():
            await self.reply(550, "File not found")
            return
        await self.reply(213, str(target.stat().st_size))

    async def cmd_MDTM(self, argument: str) -> None:
        if not await self.require_auth():
            return
        target = self.real_path(self.virtual_path(argument))
        if not target.exists():
            await self.reply(550, "Path not found")
            return
        stamp = datetime.fromtimestamp(target.stat().st_mtime, UTC).strftime(
            "%Y%m%d%H%M%S"
        )
        await self.reply(213, stamp)

    async def cmd_REST(self, argument: str) -> None:
        if not await self.require_auth():
            return
        try:
            offset = int(argument)
        except ValueError:
            await self.reply(501, "Invalid restart offset")
            return
        if offset < 0:
            await self.reply(501, "Invalid restart offset")
            return
        self.restart_offset = offset
        await self.reply(350, f"Restarting at {offset}")

    async def cmd_DELE(self, argument: str) -> None:
        if not await self.require_auth():
            return
        target = self.real_path(self.virtual_path(argument))
        if not target.is_file():
            await self.reply(550, "File not found")
            return
        target.unlink()
        await self.reply(250, "File deleted")

    async def cmd_MKD(self, argument: str) -> None:
        if not await self.require_auth():
            return
        virtual = self.virtual_path(argument)
        target = self.real_path(virtual)
        target.mkdir(mode=0o755, parents=False, exist_ok=False)
        await self.reply(257, f'"{virtual}" directory created')

    async def cmd_RMD(self, argument: str) -> None:
        if not await self.require_auth():
            return
        target = self.real_path(self.virtual_path(argument))
        if target.resolve(strict=False) == self.store.root_resolved:
            await self.reply(550, "Cannot remove staging root")
            return
        target.rmdir()
        await self.reply(250, "Directory removed")

    async def cmd_RNFR(self, argument: str) -> None:
        if not await self.require_auth():
            return
        target = self.real_path(self.virtual_path(argument))
        if target.resolve(strict=False) == self.store.root_resolved:
            await self.reply(550, "Cannot rename staging root")
            return
        if not target.exists():
            await self.reply(550, "Path not found")
            return
        self.rename_from = target
        await self.reply(350, "Ready for RNTO")

    async def cmd_RNTO(self, argument: str) -> None:
        if not await self.require_auth():
            return
        if self.rename_from is None:
            await self.reply(503, "RNFR required first")
            return
        destination = self.real_path(self.virtual_path(argument))
        if destination.resolve(strict=False) == self.store.root_resolved:
            await self.reply(550, "Cannot replace staging root")
            self.rename_from = None
            return
        if not destination.parent.is_dir():
            await self.reply(550, "Destination directory does not exist")
            return
        os.replace(self.rename_from, destination)
        self.rename_from = None
        await self.reply(250, "Rename successful")

    async def cmd_AVBL(self, _argument: str) -> None:
        if not await self.require_auth():
            return
        quota_free = max(0, self.store.max_total_bytes - self.store.used_bytes())
        disk_free = shutil.disk_usage(self.store.root).free
        await self.reply(213, str(min(quota_free, disk_free)))

    async def cmd_STAT(self, argument: str) -> None:
        if not await self.require_auth():
            return
        if argument.strip():
            try:
                lines = [self._list_line(entry) for entry in self._entries(argument)]
            except FileNotFoundError:
                await self.reply(550, "Path not found")
                return
            await self.multiline(211, ["Status", *lines])
        else:
            await self.multiline(
                211,
                [
                    "Server status",
                    f"Connected from {self.remote_ip}",
                    f"Logged in: {'yes' if self.authenticated else 'no'}",
                    f"Current directory: {self.current_dir}",
                ],
            )

    async def cmd_ALLO(self, _argument: str) -> None:
        await self.reply(202, "Allocation is not required")

    async def cmd_ABOR(self, _argument: str) -> None:
        await self.close_data()
        await self.reply(226, "Abort successful")


class FTPSStagingServer:
    def __init__(self, config: FTPSConfig) -> None:
        self.config = config
        self.store = StagingStore(config)
        self.ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.ssl_context.minimum_version = ssl.TLSVersion.TLSv1_2
        self.ssl_context.load_cert_chain(config.cert_file, config.key_file)
        self.control_server: asyncio.Server | None = None
        self._sessions = asyncio.Semaphore(32)

    async def open_passive(self, session: FTPSSession) -> int | None:
        context = self.ssl_context if session.private_data else None
        for port in range(self.config.passive_min, self.config.passive_max + 1):
            try:
                session.passive_server = await asyncio.start_server(
                    session.on_data_connection,
                    self.config.bind_host,
                    port,
                    ssl=context,
                )
                return port
            except OSError:
                continue
        return None

    async def _handle_control(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        async with self._sessions:
            await FTPSSession(self, reader, writer).run()

    async def serve(self) -> None:
        self.store.initialize()
        self.control_server = await asyncio.start_server(
            self._handle_control,
            self.config.bind_host,
            self.config.port,
            ssl=self.ssl_context,
        )
        logger.info(
            "implicit FTPS listening on %s:%d, passive=%d-%d, advertise=%s",
            self.config.bind_host,
            self.config.port,
            self.config.passive_min,
            self.config.passive_max,
            self.config.advertise_ip,
        )
        async with self.control_server:
            await self.control_server.serve_forever()


def main() -> None:
    logging.basicConfig(
        level=os.getenv("LOG_LEVEL", "INFO").upper(),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    os.umask(0o077)
    config = FTPSConfig.from_env()
    with suppress(KeyboardInterrupt):
        asyncio.run(FTPSStagingServer(config).serve())


if __name__ == "__main__":
    main()
