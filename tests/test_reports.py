import json

from bambu_cloud_lan_proxy.reports import (
    DEVELOPER_MODE_DISABLED_BIT,
    prepare_report,
)


def test_read_only_advertises_controls_disabled_without_losing_report() -> None:
    raw = '{"print":{"gcode_state":"RUNNING","fun":"1"}}'
    transformed = json.loads(prepare_report(raw, controls=False))

    assert transformed["print"]["gcode_state"] == "RUNNING"
    assert int(transformed["print"]["fun"], 16) & DEVELOPER_MODE_DISABLED_BIT


def test_controls_mode_clears_disabled_bit() -> None:
    raw = {"print": {"fun": "20000005", "mc_percent": 17}}
    transformed = json.loads(prepare_report(raw, controls=True))

    assert int(transformed["print"]["fun"], 16) == 5
    assert transformed["print"]["mc_percent"] == 17


def test_integer_fun_keeps_its_type() -> None:
    original = {"print": {"fun": DEVELOPER_MODE_DISABLED_BIT}}
    transformed = json.loads(prepare_report(original, controls=True))

    assert transformed["print"]["fun"] == 0
    assert isinstance(transformed["print"]["fun"], int)
    assert original["print"]["fun"] == DEVELOPER_MODE_DISABLED_BIT


def test_non_print_response_is_not_given_a_capability_field() -> None:
    transformed = json.loads(
        prepare_report({"info": {"command": "get_version"}}, controls=False)
    )

    assert transformed == {"info": {"command": "get_version"}}
