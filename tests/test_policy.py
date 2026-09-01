from bambu_cloud_lan_proxy.config import ProxyMode
from bambu_cloud_lan_proxy.policy import Action, decide, rejection_envelope


def test_read_only_allows_bambuddy_startup_queries() -> None:
    envelopes = [
        {"pushing": {"command": "pushall"}},
        {"info": {"command": "get_version", "sequence_id": "1"}},
        {
            "print": {
                "command": "extrusion_cali_get",
                "nozzle_diameter": "0.4",
                "sequence_id": "2",
            }
        },
    ]

    for envelope in envelopes:
        decision = decide(envelope, mode=ProxyMode.READ_ONLY, allow_cancel=False)
        assert decision.action is Action.FORWARD_QUERY


def test_read_only_blocks_control_and_preserves_sequence_in_rejection() -> None:
    decision = decide(
        {"print": {"command": "pause", "sequence_id": "42"}},
        mode=ProxyMode.READ_ONLY,
        allow_cancel=False,
    )

    assert not decision.allowed
    assert rejection_envelope(decision) == {
        "print": {
            "command": "pause",
            "sequence_id": "42",
            "result": "failed",
            "reason": "blocked by cloud LAN proxy: proxy is in read-only mode",
        }
    }


def test_controls_mode_has_narrow_allowlist() -> None:
    pause = decide(
        {"print": {"command": "pause"}},
        mode=ProxyMode.CONTROLS,
        allow_cancel=False,
    )
    arbitrary = decide(
        {"print": {"command": "gcode_line", "param": "M104 S300"}},
        mode=ProxyMode.CONTROLS,
        allow_cancel=False,
    )

    assert pause.action is Action.FORWARD_CONTROL
    assert arbitrary.action is Action.BLOCK


def test_cancel_requires_separate_opt_in() -> None:
    envelope = {"print": {"command": "stop"}}
    denied = decide(envelope, mode=ProxyMode.CONTROLS, allow_cancel=False)
    allowed = decide(envelope, mode=ProxyMode.CONTROLS, allow_cancel=True)

    assert denied.action is Action.BLOCK
    assert allowed.action is Action.FORWARD_CONTROL


def test_full_control_forwards_bambuddy_extended_namespaces_for_signing() -> None:
    decision = decide(
        {
            "xcam": {
                "command": "xcam_control_set",
                "module_name": "spaghetti_detector",
                "enable": True,
            }
        },
        mode=ProxyMode.FULL_CONTROL,
        allow_cancel=True,
    )

    assert decision.action is Action.FORWARD_SIGNED


def test_full_control_still_requires_cancel_opt_in() -> None:
    decision = decide(
        {"print": {"command": "stop"}},
        mode=ProxyMode.FULL_CONTROL,
        allow_cancel=False,
    )

    assert decision.action is Action.BLOCK


def test_ambiguous_envelope_is_rejected() -> None:
    decision = decide(
        {
            "print": {"command": "pause"},
            "system": {"command": "ledctrl", "led_mode": "on"},
        },
        mode=ProxyMode.UNSAFE,
        allow_cancel=True,
    )

    assert decision.action is Action.BLOCK
    assert decision.request is None


def test_cloud_print_needs_its_own_opt_in_even_in_unsafe_mode() -> None:
    envelope = {
        "print": {
            "command": "project_file",
            "file": "cube.gcode.3mf",
            "url": "ftp://cube.gcode.3mf",
        }
    }
    denied = decide(
        envelope,
        mode=ProxyMode.UNSAFE,
        allow_cancel=True,
        allow_cloud_print=False,
    )
    allowed = decide(
        envelope,
        mode=ProxyMode.READ_ONLY,
        allow_cancel=False,
        allow_cloud_print=True,
    )

    assert denied.action is Action.BLOCK
    assert denied.reason == "cloud print is disabled"
    assert allowed.action is Action.CLOUD_PRINT


def test_printer_resident_reprint_requires_full_control_and_its_own_gate() -> None:
    envelope = {
        "print": {
            "command": "project_file",
            "sequence_id": "12",
            "url": "file:///cache/existing.gcode.3mf",
        }
    }
    denied = decide(
        envelope,
        mode=ProxyMode.FULL_CONTROL,
        allow_cancel=False,
        allow_remote_reprint=False,
    )
    allowed = decide(
        envelope,
        mode=ProxyMode.FULL_CONTROL,
        allow_cancel=False,
        allow_remote_reprint=True,
    )
    assert denied.action is Action.BLOCK
    assert allowed.action is Action.FORWARD_SIGNED
