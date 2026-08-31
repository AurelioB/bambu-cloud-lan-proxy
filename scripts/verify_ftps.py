"""Round-trip a tiny test 3MF through the public implicit-FTPS listener."""

from __future__ import annotations

import ftplib
import io
import socket
import ssl
import uuid
import zipfile
from contextlib import suppress
from pathlib import Path


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


def load_env(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key] = value
    return values


def test_payload() -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        archive.writestr("[Content_Types].xml", "<Types/>")
        archive.writestr("Metadata/plate_1.gcode", "; proxy FTPS self-test\n")
    return output.getvalue()


def main() -> None:
    project_dir = Path(__file__).resolve().parent.parent
    env = load_env(project_dir / ".env")
    host = env.get("PROXY_BIND_IP", "127.0.0.2")
    access_code = env.get("PROXY_ACCESS_CODE", "")
    if len(access_code) != 8:
        raise SystemExit("Invalid PROXY_ACCESS_CODE; rerun ./scripts/init.sh")

    ca_file = project_dir / "data/proxy/export/bambu-cloud-lan-proxy-ca.crt"
    if not ca_file.is_file():
        raise SystemExit("Missing proxy CA; rerun ./scripts/init.sh")
    context = ssl.create_default_context(cafile=str(ca_file))
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    # Bambuddy's H2C profile pins implicit FTPS to TLS 1.2, so this also
    # exercises the exact protocol version used by the real integration.
    context.maximum_version = ssl.TLSVersion.TLSv1_2
    client = ImplicitFTP_TLS(context=context, timeout=10)
    name = f"proxy-ftps-check-{uuid.uuid4().hex}.gcode.3mf"
    uploaded = False
    payload = test_payload()
    try:
        client.connect(host, 990)
        client.login("bblp", access_code)
        client.prot_p()
        client.storbinary(f"STOR {name}", io.BytesIO(payload))
        uploaded = True
        if client.size(name) != len(payload):
            raise RuntimeError("FTPS SIZE did not match the uploaded test file")
        downloaded = io.BytesIO()
        client.retrbinary(f"RETR {name}", downloaded.write)
        if downloaded.getvalue() != payload:
            raise RuntimeError("FTPS download did not match the uploaded test file")
        print("FTPS staging round-trip succeeded (TLS control + protected data).")
    finally:
        if uploaded:
            with suppress(ftplib.Error):
                client.delete(name)
        try:
            client.quit()
        except (OSError, ftplib.Error):
            client.close()


if __name__ == "__main__":
    main()
