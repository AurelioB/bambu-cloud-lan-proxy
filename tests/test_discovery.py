from bambu_cloud_lan_proxy.discovery import build_notify


def test_bambu_discovery_notify_contains_facade_identity() -> None:
    payload = build_notify(
        ip="127.0.0.2",
        serial="SERIAL",
        model="H2C",
        name="Proxy",
        version="01.02.03.04",
    )
    assert payload.startswith(b"NOTIFY * HTTP/1.1\r\n")
    assert b"Location: 127.0.0.2\r\n" in payload
    assert b"USN: SERIAL\r\n" in payload
    assert b"DevModel.bambu.com: H2C\r\n" in payload
    assert payload.endswith(b"\r\n\r\n")
