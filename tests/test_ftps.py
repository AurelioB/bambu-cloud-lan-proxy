from __future__ import annotations

import asyncio
import ftplib
import io
import shutil
import socket
import ssl
import subprocess
import zipfile
from pathlib import Path

import pytest

from bambu_cloud_lan_proxy.ftps_server import FTPSConfig, FTPSStagingServer


class ImplicitFTP_TLS(ftplib.FTP_TLS):
    def connect(self, host="", port=990, timeout=-999, source_address=None):
        if host:
            self.host = host
        if port > 0:
            self.port = port
        if timeout != -999:
            self.timeout = timeout
        self.sock = socket.create_connection(
            (self.host, self.port), self.timeout, source_address=source_address
        )
        self.sock = self.context.wrap_socket(self.sock, server_hostname=self.host)
        self.af = self.sock.family
        self.file = self.sock.makefile("r", encoding=self.encoding)
        self.welcome = self.getresp()
        return self.welcome

    def ntransfercmd(self, cmd, rest=None):
        connection, size = ftplib.FTP.ntransfercmd(self, cmd, rest)
        if self._prot_p:
            connection = self.context.wrap_socket(
                connection,
                server_hostname=self.host,
                session=self.sock.session,
            )
        return connection, size


def _free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def _three_mf() -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        archive.writestr("[Content_Types].xml", "<Types/>")
        archive.writestr("Metadata/plate_1.gcode", "; synthetic test\n")
    return output.getvalue()


def _certificate(tmp_path: Path) -> tuple[Path, Path]:
    openssl = shutil.which("openssl")
    if openssl is None:
        pytest.skip("openssl is required for the FTPS integration test")
    cert = tmp_path / "server.crt"
    key = tmp_path / "server.key"
    subprocess.run(
        [
            openssl,
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-keyout",
            str(key),
            "-out",
            str(cert),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return cert, key


def test_implicit_ftps_upload_list_download_and_delete(tmp_path: Path) -> None:
    cert, key = _certificate(tmp_path)
    passive_port = _free_port()
    config = FTPSConfig(
        bind_host="127.0.0.1",
        port=1,
        advertise_ip="127.0.0.1",
        passive_min=passive_port,
        passive_max=passive_port,
        root=tmp_path / "staging",
        access_code="A1B2C3D4",
        cert_file=cert,
        key_file=key,
        max_upload_bytes=1024 * 1024,
        max_total_bytes=2 * 1024 * 1024,
        idle_timeout=30,
    )
    server = FTPSStagingServer(config)
    payload = _three_mf()

    async def scenario() -> None:
        server.store.initialize()
        control = await asyncio.start_server(
            server._handle_control,  # noqa: SLF001 - intentional protocol integration test
            config.bind_host,
            0,
            ssl=server.ssl_context,
        )
        control_port = int(control.sockets[0].getsockname()[1])

        def client_flow() -> None:
            context = ssl._create_unverified_context()
            context.minimum_version = ssl.TLSVersion.TLSv1_2
            context.maximum_version = ssl.TLSVersion.TLSv1_2
            client = ImplicitFTP_TLS(context=context, timeout=10)
            client.connect("127.0.0.1", control_port)
            assert client.sock.version() == "TLSv1.2"
            with pytest.raises(ftplib.error_perm, match="530"):
                client.sendcmd("STAT /")
            client.login("bblp", config.access_code)
            client.prot_p()
            with pytest.raises(ftplib.error_perm, match="550"):
                client.rmd("/")
            with pytest.raises(ftplib.error_perm, match="550"):
                client.rename("/", "/moved-root")
            client.storbinary("STOR queue-test.gcode.3mf", io.BytesIO(payload))
            assert client.size("queue-test.gcode.3mf") == len(payload)
            listing: list[str] = []
            client.retrlines("LIST /", listing.append)
            assert any(line.endswith(" queue-test.gcode.3mf") for line in listing)
            assert client.sendcmd("AVBL").startswith("213 ")
            downloaded = io.BytesIO()
            client.retrbinary("RETR queue-test.gcode.3mf", downloaded.write)
            assert downloaded.getvalue() == payload
            client.delete("queue-test.gcode.3mf")
            with pytest.raises(ftplib.error_perm):
                client.size("queue-test.gcode.3mf")
            client.quit()

        try:
            await asyncio.wait_for(asyncio.to_thread(client_flow), timeout=20)
        finally:
            control.close()
            await control.wait_closed()

    asyncio.run(scenario())
