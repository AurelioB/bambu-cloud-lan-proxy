import json
from pathlib import Path, PurePosixPath

import pytest

from bambu_cloud_lan_proxy.print_jobs import (
    CloudPrintValidationError,
    build_gateway_print_params,
    cloud_print_ack,
    parse_cloud_print_request,
)


def test_h2c_project_file_is_preserved_for_gateway_plugin(tmp_path: Path) -> None:
    request = parse_cloud_print_request(
        {
            "sequence_id": "20000",
            "command": "project_file",
            "param": "Metadata/plate_3.gcode",
            "url": "ftp://rack-test.gcode.3mf",
            "file": "rack-test.gcode.3mf",
            "subtask_name": "Rack test",
            "bed_type": "auto",
            "timelapse": True,
            "bed_leveling": False,
            "auto_bed_leveling": 2,
            "flow_cali": False,
            "extrude_cali_flag": 2,
            "vibration_cali": True,
            "layer_inspect": False,
            "use_ams": True,
            "nozzle_offset_cali": 2,
            "extrude_cali_manual_mode": 0,
            "cfg": "1",
            "ams_mapping": [0, -1],
            "ams_mapping2": [
                {"ams_id": 0, "slot_id": 0},
                {"ams_id": 254, "slot_id": 0},
            ],
            # H2C Studio jobs use a fixed 32-entry nozzle map even when the AMS
            # arrays contain only the filament slots used by this plate.
            "nozzle_mapping": [16, 1, *([-1] * 30)],
        }
    )

    assert request.relative_path == PurePosixPath("rack-test.gcode.3mf")
    assert request.plate_index == 3
    assert request.auto_flow_cali == 2

    staged = tmp_path / request.filename
    params = build_gateway_print_params(request, serial="SERIAL", file_path=staged)

    assert params["dev_id"] == "SERIAL"
    assert params["plate_index"] == 3
    assert json.loads(params["ams_mapping"]) == [0, -1]
    assert json.loads(params["ams_mapping2"])[1] == {"ams_id": 254, "slot_id": 0}
    assert json.loads(params["nozzle_mapping"]) == [16, 1, *([-1] * 30)]
    assert params["task_record_timelapse"] is True
    assert params["auto_bed_leveling"] == 2
    assert params["auto_flow_cali"] == 2
    assert params["auto_offset_cali"] == 2
    assert params["task_ext_change_assist"] is True
    assert params["filename"] == str(staged)
    assert params["config_filename"] == str(staged)

    ack = cloud_print_ack(request)
    assert ack["print"]["result"] == "SUCCESS"
    assert ack["print"]["sequence_id"] == "20000"
    assert ack["print"]["gcode_state"] == "PREPARE"


def test_url_only_request_and_external_spool_mapping(tmp_path: Path) -> None:
    request = parse_cloud_print_request(
        {
            "url": "ftp://folder%20name/cube.3mf",
            "param": "Metadata/plate_1.gcode",
            "ams_mapping": [255],
        }
    )
    params = build_gateway_print_params(
        request,
        serial="SERIAL",
        file_path=tmp_path / "cube.3mf",
    )

    assert request.relative_path == PurePosixPath("cube.3mf")
    assert json.loads(params["ams_mapping2"]) == [{"ams_id": 255, "slot_id": 0}]


@pytest.mark.parametrize(
    "body",
    [
        {"file": "../secret.3mf"},
        {"file": ".upload-job.part"},
        {"file": "unsliced.gcode"},
        {"url": "https://example.test/job.3mf"},
        {"file": "job.3mf", "ams_mapping": [999]},
        {
            "file": "job.3mf",
            "ams_mapping": [0, 1],
            "ams_mapping2": [{"ams_id": 0, "slot_id": 0}],
        },
    ],
)
def test_unsafe_or_untranslatable_requests_are_rejected(body: dict) -> None:
    with pytest.raises(CloudPrintValidationError):
        parse_cloud_print_request(body)
