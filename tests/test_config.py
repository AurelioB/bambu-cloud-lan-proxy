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
    assert config.cloud_print_enabled is False
    assert config.cloud_print_max_bytes == 1024 * 1024 * 1024


def test_controls_mode_auto_advertises_controls(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CLOUD_LAN_PROXY_MODE", "controls")
    monkeypatch.setenv("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", "auto")

    config = ProxyConfig.from_env()

    assert config.mode is ProxyMode.CONTROLS
    assert config.advertise_controls is True


def test_full_control_mode_auto_advertises_controls(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CLOUD_LAN_PROXY_MODE", "full_control")
    monkeypatch.setenv("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", "auto")

    config = ProxyConfig.from_env()

    assert config.mode is ProxyMode.FULL_CONTROL
    assert config.advertise_controls is True


def test_cloud_print_auto_advertises_writable_facade(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("CLOUD_LAN_PROXY_MODE", "read_only")
    monkeypatch.setenv("CLOUD_LAN_PROXY_CLOUD_PRINT_ENABLED", "true")
    monkeypatch.setenv("CLOUD_LAN_PROXY_ADVERTISE_CONTROLS", "auto")

    config = ProxyConfig.from_env()

    assert config.cloud_print_enabled is True
    assert config.advertise_controls is True
