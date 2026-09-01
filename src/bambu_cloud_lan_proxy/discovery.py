"""Advertise the LAN facade with Bambu's SSDP-compatible NOTIFY dialect."""

from __future__ import annotations

import logging
import os
import socket
import time

logger = logging.getLogger("bambu.cloud_lan_proxy.discovery")


def build_notify(*, ip: str, serial: str, model: str, name: str, version: str) -> bytes:
    fields = [
        "NOTIFY * HTTP/1.1",
        "HOST: 239.255.255.250:1900",
        "Server: UPnP/1.0",
        f"Location: {ip}",
        "NT: urn:bambulab-com:device:3dprinter:1",
        f"USN: {serial}",
        "Cache-Control: max-age=1800",
        f"DevModel.bambu.com: {model}",
        f"DevName.bambu.com: {name}",
        "DevSignal.bambu.com: -40",
        "DevConnect.bambu.com: lan",
        "DevBind.bambu.com: bind",
        "Devseclink.bambu.com: secure",
        f"DevVersion.bambu.com: {version}",
        "DevCap.bambu.com: 1",
        "",
        "",
    ]
    return "\r\n".join(fields).encode("ascii")


def main() -> None:
    logging.basicConfig(
        level=os.getenv("LOG_LEVEL", "INFO").upper(),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    ip = os.environ["DISCOVERY_PRINTER_IP"]
    serial = os.environ["DISCOVERY_PRINTER_SERIAL"].strip().upper()
    if not serial:
        raise SystemExit("DISCOVERY_PRINTER_SERIAL is required")
    payload = build_notify(
        ip=ip,
        serial=serial,
        model=os.getenv("DISCOVERY_PRINTER_MODEL", "H2C"),
        name=os.getenv("DISCOVERY_PRINTER_NAME", "Bambu Cloud LAN Proxy"),
        version=os.getenv("DISCOVERY_PRINTER_VERSION", "00.00.00.00"),
    )
    target = os.getenv("DISCOVERY_TARGET", "255.255.255.255")
    interval = max(2.0, float(os.getenv("DISCOVERY_INTERVAL", "5")))
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        logger.info("advertising serial=%s ip=%s to %s:2021", serial, ip, target)
        while True:
            sock.sendto(payload, (target, 2021))
            time.sleep(interval)


if __name__ == "__main__":
    main()
