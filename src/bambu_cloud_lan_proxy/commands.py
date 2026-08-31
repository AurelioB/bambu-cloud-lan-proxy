"""Translate allowlisted LAN controls to Gateway's signed cloud commands."""

from __future__ import annotations

from typing import Any

from .policy import ParsedRequest


class InvalidControl(ValueError):
    pass


def translate_control(request: ParsedRequest) -> dict[str, Any]:
    """Build a fresh Gateway command with a valid Studio sequence ID.

    Importing the builders lazily keeps the policy package independently
    testable while reusing Gateway's transport-neutral command definitions at
    runtime.
    """

    from app.mqtt_client import (  # type: ignore[import-not-found]
        build_cancel_command,
        build_light_command,
        build_pause_command,
        build_resume_command,
        build_speed_command,
    )

    key = (request.namespace, request.command)
    if key == ("print", "pause"):
        return build_pause_command()
    if key == ("print", "resume"):
        return build_resume_command()
    if key == ("print", "stop"):
        return build_cancel_command()
    if key == ("print", "print_speed"):
        raw_level = request.body.get("param", request.body.get("level"))
        try:
            level = int(raw_level)
        except (TypeError, ValueError) as exc:
            raise InvalidControl("print_speed requires an integer level") from exc
        if level not in {1, 2, 3, 4}:
            raise InvalidControl("print_speed level must be between 1 and 4")
        return build_speed_command(level)
    if key == ("system", "ledctrl"):
        node = str(request.body.get("led_node", "chamber_light"))
        if node not in {"chamber_light", "work_light"}:
            raise InvalidControl(f"unsupported light node {node!r}")
        mode = str(request.body.get("led_mode", "")).lower()
        if mode not in {"on", "off"}:
            raise InvalidControl("ledctrl requires led_mode=on or off")
        return build_light_command(mode == "on", node=node)
    raise InvalidControl(
        f"no cloud translation for {request.namespace}.{request.command}"
    )
