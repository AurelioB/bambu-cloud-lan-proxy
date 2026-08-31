import pytest

from bambu_cloud_lan_proxy.config import ProxyConfig, ProxyMode


def test_default_configuration_is_read_only(monkeypatch: pytest.MonkeyPatch) -> None:
    for key in list(__import__("os").environ):
        if key.startswith("CLOUD_LAN_PROXY_"):
            monkeypatch.delenv(key)

    config = ProxyConfig.from_env()

    assert config.mode is ProxyMode.READ_ONLY
    assert config.advertise_controls is False
    assert config.allow_cancel is False


def test_controls_mode_auto_advertises_controls(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CLOUD_LAN_PROXY_MODE", "controls")
    monkeypatch.setenv("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", "auto")

    config = ProxyConfig.from_env()

    assert config.mode is ProxyMode.CONTROLS
    assert config.advertise_controls is True
