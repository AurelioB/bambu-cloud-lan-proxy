"""Small compatibility transforms for reports sent to LAN clients."""

from __future__ import annotations

import copy
import json
from typing import Any

DEVELOPER_MODE_DISABLED_BIT = 0x20000000


def _parse_fun(value: object) -> tuple[int, str] | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value, "int"
    if isinstance(value, str):
        try:
            return int(value, 16), "str"
        except ValueError:
            return None
    return None


def _format_fun(value: int, original_type: str) -> int | str:
    if original_type == "int":
        return value
    return f"{value:X}"


def _set_control_capability(container: dict[str, Any], enabled: bool) -> None:
    parsed = _parse_fun(container.get("fun"))
    if parsed is None:
        current, original_type = 0, "str"
    else:
        current, original_type = parsed
    if enabled:
        current &= ~DEVELOPER_MODE_DISABLED_BIT
    else:
        current |= DEVELOPER_MODE_DISABLED_BIT
    container["fun"] = _format_fun(current, original_type)


def prepare_report(raw: str | bytes | dict[str, Any], *, controls: bool) -> str:
    """Return a valid raw report with the facade's control capability advertised.

    Bambu's ``fun`` bit describes whether unsigned LAN commands are accepted.
    Bambuddy uses that bit to enable its controls. The facade deliberately
    advertises its own policy instead: disabled in read-only mode and enabled
    only when signed, allowlisted cloud controls are active.
    """

    if isinstance(raw, bytes):
        raw = raw.decode("utf-8")
    payload = json.loads(raw) if isinstance(raw, str) else copy.deepcopy(raw)
    if not isinstance(payload, dict):
        raise ValueError("cloud report must be a JSON object")

    if "fun" in payload:
        _set_control_capability(payload, controls)

    print_block = payload.get("print")
    if isinstance(print_block, dict):
        _set_control_capability(print_block, controls)

    return json.dumps(payload, separators=(",", ":"), ensure_ascii=False)
