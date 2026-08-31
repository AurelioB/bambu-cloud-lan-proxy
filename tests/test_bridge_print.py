from __future__ import annotations

import asyncio
import io
import json
import time
import zipfile
from pathlib import Path
from types import SimpleNamespace

from bambu_cloud_lan_proxy.bridge import CloudLanFacade
from bambu_cloud_lan_proxy.config import ProxyConfig, ProxyMode
from bambu_cloud_lan_proxy.print_jobs import parse_cloud_print_request


class FakeBroker:
    def __init__(self) -> None:
        self.messages: list[tuple[str, dict]] = []

    def publish(self, topic, payload, **_kwargs):
        self.messages.append((topic, json.loads(payload)))
        return SimpleNamespace(rc=0)


class FakeCloudClient:
    serial = "SERIAL"
    _progress = None

    def __init__(self, state: str = "idle") -> None:
        self.state = state
        self.params: dict | None = None
        self.snapshot_existed = False

    def get_status(self):
        return SimpleNamespace(
            online=True,
            state=SimpleNamespace(value=self.state),
        )

    async def submit_print(self, *, print_params: dict, progress_timeout: float):
        self.params = print_params
        self.snapshot_existed = Path(print_params["filename"]).is_file()
        assert progress_timeout == 15
        yield {"event": "progress", "stage": 2, "code": 0}
        yield {"event": "done"}


def _config(tmp_path: Path) -> ProxyConfig:
    return ProxyConfig(
        enabled=True,
        broker_host="mqtt",
        broker_port=1883,
        mode=ProxyMode.READ_ONLY,
        allow_cancel=False,
        cloud_print_enabled=True,
        cloud_print_wait_seconds=1,
        cloud_print_progress_timeout=15,
        cloud_print_max_bytes=1024 * 1024,
        staging_dir=tmp_path / "staging",
        cloud_print_job_dir=tmp_path / "jobs",
        printers=frozenset(),
        advertise_controls=False,
    )


def _write_three_mf(path: Path) -> None:
    payload = io.BytesIO()
    with zipfile.ZipFile(payload, "w") as archive:
        archive.writestr("Metadata/plate_1.gcode", "; test\n")
    path.parent.mkdir(parents=True)
    path.write_bytes(payload.getvalue())


def test_staged_job_uses_snapshot_and_reaches_gateway_client(tmp_path: Path) -> None:
    config = _config(tmp_path)
    staged = config.staging_dir / "cube.gcode.3mf"
    _write_three_mf(staged)
    request = parse_cloud_print_request(
        {
            "command": "project_file",
            "sequence_id": "20000",
            "file": staged.name,
            "param": "Metadata/plate_1.gcode",
            "ams_mapping": [2],
            "ams_mapping2": [{"ams_id": 0, "slot_id": 2}],
        }
    )
    facade = CloudLanFacade(config)
    facade._mqtt = FakeBroker()
    facade._print_jobs["job"] = (time.monotonic(), "active", request, "")
    cloud = FakeCloudClient()

    asyncio.run(facade._run_cloud_print("SERIAL", cloud, "job", request))

    assert cloud.snapshot_existed is True
    assert cloud.params is not None
    assert Path(cloud.params["filename"]) != staged
    assert not Path(cloud.params["filename"]).exists()
    assert json.loads(cloud.params["ams_mapping2"]) == [{"ams_id": 0, "slot_id": 2}]
    assert facade._print_jobs["job"][1] == "succeeded"
    assert facade._mqtt.messages == []


def test_busy_printer_fails_closed() -> None:
    cloud = FakeCloudClient(state="paused")

    assert CloudLanFacade._printer_start_error(cloud) == "printer is busy (paused)"
