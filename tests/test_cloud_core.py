import asyncio
import json
import os
import sys
import time
from types import SimpleNamespace

import pytest

from bambu_cloud_lan_proxy.cloud_auth import login_payload, signin_url, ticket_from_url
from bambu_cloud_lan_proxy.cloud_printer import CloudPrinter
from bambu_cloud_lan_proxy.commands import command, translate_control
from bambu_cloud_lan_proxy.plugin_install import validate_binary
from bambu_cloud_lan_proxy.plugin_rpc import PluginError, PluginRPC


def test_disabled_facade_does_not_start_mqtt(monkeypatch):
    from bambu_cloud_lan_proxy.bridge import CloudLanFacade
    from bambu_cloud_lan_proxy.config import ProxyConfig

    monkeypatch.setenv("CLOUD_LAN_PROXY_ENABLED", "false")
    facade = CloudLanFacade(ProxyConfig.from_env())
    facade.register(SimpleNamespace(serial="SERIAL"))
    assert not facade._clients
    assert facade._mqtt is None


def test_ticket_validation():
    assert ticket_from_url("http://localhost:13618/?ticket=one") == "one"
    assert ticket_from_url("http://127.0.0.1:13618/#ticket=two") == "two"
    for url in (
        "https://evil.example/?ticket=x",
        "http://localhost:99/?ticket=x",
        "http://localhost:13618/?ticket=a&ticket=b",
        "http://localhost:13618/",
        "http://user@localhost:13618/?ticket=a",
    ):
        with pytest.raises(ValueError):
            ticket_from_url(url)
    assert signin_url("US").startswith("https://bambulab.com/sign-in?")


def test_login_normalizes_protocol_fields_without_logging_tokens():
    data = json.loads(
        login_payload(
            {"accessToken": "a", "refreshToken": "r", "expiresIn": 3600},
            {"uidStr": "123"},
        )
    )["data"]
    assert data["user"]["uidStr"] == data["user_id"] == "123"
    assert data["token"] == data["access_token"] == "a"
    assert data["expires_in"] == "3600"
    with pytest.raises(PluginError):
        login_payload({"accessToken": "a"}, {"uid": "1"})


def test_minimal_state_fails_closed_on_stale_unknown_or_offline():
    host = SimpleNamespace(failure=None)
    printer = CloudPrinter("SERIAL", host)
    assert not printer.get_status().online
    assert printer.receive({"print": {"gcode_state": "FINISH", "ams": {}}})
    printer.connected = True
    assert printer.get_status().online and printer.state == "finished"
    assert printer.receive({"print": {"gcode_state": "SOMETHING_NEW"}})
    assert printer.state == "unknown"
    printer.state_seen = time.monotonic() - 100
    assert not printer.get_status().online
    printer.receive("device_cert_installed")
    assert printer.secure
    printer.receive("cert_revoked")
    assert not printer.secure
    assert not printer.receive("not JSON")
    assert not printer.receive([1, 2])


def test_commands_do_not_need_gateway():
    request = SimpleNamespace(namespace="print", command="pause", body={})
    assert translate_control(request)["print"]["command"] == "pause"
    values = [int(command("print", "pause")["print"]["sequence_id"]) for _ in range(10002)]
    assert min(values) == 20000 and max(values) == 29999
    light = translate_control(
        SimpleNamespace(namespace="system", command="ledctrl", body={"led_mode": "on"})
    )
    assert light["system"]["led_node"] == "chamber_light"


def test_plugin_pin_refuses_unreviewed_binary():
    with pytest.raises(ValueError):
        validate_binary("libbambu_networking.so", b"not ELF")
    with pytest.raises(ValueError):
        validate_binary("libbambu_networking.so", b"\x7fELF\x02\x01" + b"\0" * 12 + b"\x3e\0")


FAKE_HOST = r"""
import json, sys, time
for line in sys.stdin:
    q = json.loads(line)
    if q['method'] == 'die':
        break
    if q['method'] == 'slow':
        time.sleep(.1)
    print(json.dumps({'event': {'kind': 'test'}}), flush=True)
    print(json.dumps({'id': q['id'], 'result': q['params']}), flush=True)
"""


def test_rpc_concurrency_late_reply_cancellation_and_process_death():
    async def scenario():
        host = PluginRPC([sys.executable, "-u", "-c", FAKE_HOST])
        await host.start()
        try:
            assert await asyncio.gather(*(host.call("echo", {"i": i}) for i in range(8))) == [
                {"i": i} for i in range(8)
            ]
            with pytest.raises(PluginError, match="outcome unknown"):
                await host.call("slow", {}, timeout=0.01)
            assert await host.call("echo", {"still": "alive"}) == {"still": "alive"}
            task = asyncio.create_task(host.call("slow", {}))
            await asyncio.sleep(0.01)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
            assert await host.call("echo", {}) == {}
            assert not host.pending
            with pytest.raises(PluginError):
                await host.call("die", {})
            assert host.failure
        finally:
            await host.stop()

    asyncio.run(scenario())


def test_uncertain_print_survives_restart_and_retains_safety_latch(tmp_path):
    async def scenario():
        class Host:
            failure = None

            async def call(self, method, params):
                return {"rc": 0}

        printer = CloudPrinter("SERIAL", Host(), tmp_path)
        printer.connected = printer.secure = True
        printer.receive({"print": {"gcode_state": "IDLE"}})
        with pytest.raises(PluginError, match="outcome unknown"):
            async for _ in printer.submit_print(print_params={}, progress_timeout=0.01):
                pass
        assert printer.uncertain_submission
        assert CloudPrinter("SERIAL", Host(), tmp_path).uncertain_submission

    asyncio.run(scenario())


def test_service_routes_are_independent_and_admin_is_authenticated(monkeypatch):
    from fastapi.testclient import TestClient

    from bambu_cloud_lan_proxy.service import create_app

    monkeypatch.setenv("CLOUD_LAN_PROXY_ACCESS_CODE", "TEST1234")
    client = TestClient(create_app())  # no lifespan: no native process/network
    assert client.get("/api/cloud/auth/url").status_code == 401
    assert client.post("/api/cloud/auth/paste", json={"pasted_url": "x"}).status_code == 401
    assert client.get("/internal/cloud-lan-proxy/storage/capabilities").status_code == 401
    response = client.get("/api/cloud/auth/url", headers={"X-Cloud-Lan-Proxy-Key": "TEST1234"})
    assert response.status_code == 200 and response.json()["url"].startswith("https:")
    assert client.get("/api/health").status_code == 503


def test_print_completion_waits_for_native_return_and_clears_latch(tmp_path):
    async def scenario():
        class Host:
            failure = None

            async def call(self, method, params):
                printer._progress.put_nowait({"kind": "OnUpdateStatus", "stage": 6, "code": 0})
                printer._progress.put_nowait({"kind": "PrintComplete", "rc": 0})
                return {"rc": 0}

        printer = CloudPrinter("SERIAL", Host(), tmp_path)
        printer.connected = printer.secure = True
        printer.receive({"print": {"gcode_state": "IDLE"}})
        frames = [frame async for frame in printer.submit_print(print_params={})]
        assert [frame["event"] for frame in frames] == ["progress", "done"]
        assert not printer.uncertain_submission
        assert not printer.marker.exists()

    asyncio.run(scenario())


@pytest.mark.skipif(not os.getenv("BAMBU_TEST_NATIVE"), reason="requires compiled native shim")
def test_compiled_native_rpc_without_plugin():
    async def scenario():
        host = PluginRPC([os.environ["BAMBU_TEST_NATIVE"]])
        await host.start()
        try:
            assert await host.call("echo", {"hello": "native"}) == {"hello": "native"}
            with pytest.raises(PluginError):
                await host.call("unknown", {})
            assert await host.call("echo", {}) == {}
        finally:
            await host.stop()
    asyncio.run(scenario())
