"""Fail-closed policy for MQTT requests received from LAN clients."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from .config import ProxyMode


class Action(StrEnum):
    FORWARD_QUERY = "forward_query"
    FORWARD_CONTROL = "forward_control"
    BLOCK = "block"


@dataclass(frozen=True, slots=True)
class ParsedRequest:
    namespace: str
    command: str
    body: dict[str, Any]
    envelope: dict[str, Any]

    @property
    def sequence_id(self) -> str:
        return str(self.body.get("sequence_id", ""))


@dataclass(frozen=True, slots=True)
class Decision:
    action: Action
    request: ParsedRequest | None
    reason: str

    @property
    def allowed(self) -> bool:
        return self.action is not Action.BLOCK


SAFE_QUERIES = frozenset(
    {
        ("pushing", "pushall"),
        ("info", "get_version"),
        ("system", "get_accessories"),
        ("print", "extrusion_cali_get"),
    }
)

SAFE_CONTROLS = frozenset(
    {
        ("print", "pause"),
        ("print", "resume"),
        ("print", "print_speed"),
        ("system", "ledctrl"),
    }
)

CANCEL_COMMAND = ("print", "stop")
NAMESPACES = ("pushing", "info", "system", "print")


def parse_request(envelope: object) -> ParsedRequest | None:
    if not isinstance(envelope, dict):
        return None

    candidates: list[ParsedRequest] = []
    for namespace in NAMESPACES:
        body = envelope.get(namespace)
        if not isinstance(body, dict):
            continue
        command = body.get("command")
        if isinstance(command, str) and command.strip():
            candidates.append(
                ParsedRequest(
                    namespace=namespace,
                    command=command.strip(),
                    body=body,
                    envelope=envelope,
                )
            )
    if len(candidates) != 1:
        return None
    return candidates[0]


def decide(envelope: object, *, mode: ProxyMode, allow_cancel: bool) -> Decision:
    request = parse_request(envelope)
    if request is None:
        return Decision(Action.BLOCK, None, "malformed or ambiguous command envelope")

    command_key = (request.namespace, request.command)
    if command_key in SAFE_QUERIES:
        return Decision(Action.FORWARD_QUERY, request, "read-only query")

    if mode is ProxyMode.READ_ONLY:
        return Decision(Action.BLOCK, request, "proxy is in read-only mode")

    if command_key in SAFE_CONTROLS:
        return Decision(Action.FORWARD_CONTROL, request, "allowlisted control")

    if command_key == CANCEL_COMMAND:
        if allow_cancel:
            return Decision(Action.FORWARD_CONTROL, request, "cancel explicitly enabled")
        return Decision(Action.BLOCK, request, "cancel is disabled")

    if mode is ProxyMode.UNSAFE:
        return Decision(Action.FORWARD_QUERY, request, "unsafe passthrough mode")

    return Decision(Action.BLOCK, request, "command is not on the controls allowlist")


def rejection_envelope(decision: Decision) -> dict[str, Any] | None:
    request = decision.request
    if request is None:
        return None
    body: dict[str, Any] = {
        "command": request.command,
        "result": "failed",
        "reason": f"blocked by cloud LAN proxy: {decision.reason}",
    }
    if request.sequence_id:
        body["sequence_id"] = request.sequence_id
    return {request.namespace: body}
