"""Construct the small subset of Bambu control envelopes we normalize."""

from __future__ import annotations

from itertools import count
from typing import Any

from .policy import ParsedRequest


class InvalidControl(ValueError):
    pass


_sequences = count()


def command(namespace: str, name: str, **fields: Any) -> dict[str, Any]:
    """Use the slicer sequence range; transport/signing is a separate concern."""
    return {
        namespace: {**fields, "command": name, "sequence_id": str(20000 + next(_sequences) % 10000)}
    }


def translate_control(request: ParsedRequest) -> dict[str, Any]:
    """Validate controls independently of the cloud backend implementation."""

    key = (request.namespace, request.command)
    if key == ("print", "pause"):
        return command("print", "pause")
    if key == ("print", "resume"):
        return command("print", "resume")
    if key == ("print", "stop"):
        return command("print", "stop")
    if key == ("print", "print_speed"):
        raw_level = request.body.get("param", request.body.get("level"))
        try:
            level = int(raw_level)
        except (TypeError, ValueError) as exc:
            raise InvalidControl("print_speed requires an integer level") from exc
        if level not in {1, 2, 3, 4}:
            raise InvalidControl("print_speed level must be between 1 and 4")
        return command("print", "print_speed", param=str(level))
    if key == ("system", "ledctrl"):
        node = str(request.body.get("led_node", "chamber_light"))
        if node not in {"chamber_light", "work_light"}:
            raise InvalidControl(f"unsupported light node {node!r}")
        mode = str(request.body.get("led_mode", "")).lower()
        if mode not in {"on", "off"}:
            raise InvalidControl("ledctrl requires led_mode=on or off")
        return command(
            "system",
            "ledctrl",
            led_node=node,
            led_mode=mode,
            led_on_time=500,
            led_off_time=500,
            loop_times=0,
            interval_time=0,
        )
    raise InvalidControl(f"no cloud translation for {request.namespace}.{request.command}")
