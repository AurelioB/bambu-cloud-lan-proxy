"""Validation and translation for staged ``project_file`` cloud jobs."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any
from urllib.parse import unquote, urlsplit

_PLATE_RE = re.compile(r"(?:^|/)plate_(\d+)\.gcode$", re.IGNORECASE)


class CloudPrintValidationError(ValueError):
    """A downstream print request is unsafe or cannot be translated."""


@dataclass(frozen=True, slots=True)
class CloudPrintRequest:
    sequence_id: str
    relative_path: PurePosixPath
    filename: str
    project_name: str
    plate_index: int
    param: str
    ams_mapping: tuple[int, ...] | None
    ams_mapping2: tuple[dict[str, int], ...] | None
    nozzle_mapping: tuple[int, ...] | None
    use_ams: bool
    bed_leveling: bool
    flow_cali: bool
    vibration_cali: bool
    layer_inspect: bool
    timelapse: bool
    bed_type: str
    auto_bed_leveling: int
    auto_flow_cali: int
    auto_offset_cali: int
    extruder_cali_manual_mode: int
    ext_change_assist: bool


def _request_path(body: dict[str, Any]) -> PurePosixPath:
    candidate = body.get("file")
    if not isinstance(candidate, str) or not candidate.strip():
        raw_url = body.get("url")
        if not isinstance(raw_url, str) or not raw_url.strip():
            raise CloudPrintValidationError("project_file has no file or URL")
        parsed = urlsplit(raw_url)
        if parsed.scheme and parsed.scheme.lower() != "ftp":
            raise CloudPrintValidationError("project_file URL must use ftp://")
        candidate = parsed.path.lstrip("/") or parsed.netloc

    candidate = unquote(candidate.strip()).replace("\\", "/").lstrip("/")
    if not candidate or "\x00" in candidate:
        raise CloudPrintValidationError("project_file path is empty or invalid")

    parts: list[str] = []
    for part in PurePosixPath(candidate).parts:
        if part in {"", "."}:
            continue
        if part == ".." or part.startswith(".proxy-") or part.startswith(".upload-"):
            raise CloudPrintValidationError("project_file path escapes staged storage")
        parts.append(part)
    if not parts:
        raise CloudPrintValidationError("project_file path is empty")

    relative = PurePosixPath(*parts)
    if not relative.name.lower().endswith(".3mf"):
        raise CloudPrintValidationError("cloud printing requires a sliced .3mf file")
    return relative


def _bool(body: dict[str, Any], key: str, default: bool) -> bool:
    value = body.get(key, default)
    if isinstance(value, bool):
        return value
    if isinstance(value, int) and value in {0, 1}:
        return bool(value)
    return default


def _small_int(
    body: dict[str, Any], key: str, default: int, *, minimum: int = 0
) -> int:
    value = body.get(key, default)
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return default
    return parsed if minimum <= parsed <= 2 else default


def _int_list(value: object, *, name: str) -> tuple[int, ...] | None:
    if value is None:
        return None
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except json.JSONDecodeError as exc:
            raise CloudPrintValidationError(f"{name} is not valid JSON") from exc
    if not isinstance(value, list):
        raise CloudPrintValidationError(f"{name} must be an array")
    result: list[int] = []
    for item in value:
        try:
            parsed = int(item)
        except (TypeError, ValueError) as exc:
            raise CloudPrintValidationError(f"{name} contains a non-integer") from exc
        if not -1 <= parsed <= 255:
            raise CloudPrintValidationError(f"{name} contains an out-of-range tray")
        result.append(parsed)
    return tuple(result)


def _mapping2(value: object) -> tuple[dict[str, int], ...] | None:
    if value is None:
        return None
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except json.JSONDecodeError as exc:
            raise CloudPrintValidationError("ams_mapping2 is not valid JSON") from exc
    if not isinstance(value, list):
        raise CloudPrintValidationError("ams_mapping2 must be an array")
    result: list[dict[str, int]] = []
    for entry in value:
        if not isinstance(entry, dict):
            raise CloudPrintValidationError("ams_mapping2 entries must be objects")
        try:
            ams_id = int(entry["ams_id"])
            slot_id = int(entry["slot_id"])
        except (KeyError, TypeError, ValueError) as exc:
            raise CloudPrintValidationError("ams_mapping2 entry is invalid") from exc
        if not 0 <= ams_id <= 255 or not 0 <= slot_id <= 255:
            raise CloudPrintValidationError("ams_mapping2 entry is out of range")
        result.append({"ams_id": ams_id, "slot_id": slot_id})
    return tuple(result)


def parse_cloud_print_request(body: dict[str, Any]) -> CloudPrintRequest:
    """Parse the printer-shaped payload while rejecting ambiguous paths/types."""

    relative = _request_path(body)
    param = body.get("param", "Metadata/plate_1.gcode")
    if not isinstance(param, str):
        param = "Metadata/plate_1.gcode"
    match = _PLATE_RE.search(param)
    plate_index = int(match.group(1)) if match else 1
    if not 1 <= plate_index <= 256:
        raise CloudPrintValidationError("plate index is outside the supported range")

    filename = relative.name
    project_name = filename
    for suffix in (".gcode.3mf", ".3mf"):
        if project_name.lower().endswith(suffix):
            project_name = project_name[: -len(suffix)]
            break
    project_name = project_name or "Bambuddy print"

    cfg = body.get("cfg", "0")
    try:
        cfg_bits = int(cfg)
    except (TypeError, ValueError):
        cfg_bits = 0

    ams_mapping = _int_list(body.get("ams_mapping"), name="ams_mapping")
    ams_mapping2 = _mapping2(body.get("ams_mapping2"))
    nozzle_mapping = _int_list(body.get("nozzle_mapping"), name="nozzle_mapping")
    if (
        ams_mapping is not None
        and ams_mapping2 is not None
        and len(ams_mapping) != len(ams_mapping2)
    ):
        raise CloudPrintValidationError("AMS mapping arrays have different lengths")
    return CloudPrintRequest(
        sequence_id=str(body.get("sequence_id", "0")),
        relative_path=relative,
        filename=filename,
        project_name=str(body.get("subtask_name") or project_name),
        plate_index=plate_index,
        param=param,
        ams_mapping=ams_mapping,
        ams_mapping2=ams_mapping2,
        nozzle_mapping=nozzle_mapping,
        use_ams=_bool(body, "use_ams", True),
        bed_leveling=_bool(body, "bed_leveling", True),
        flow_cali=_bool(body, "flow_cali", True),
        vibration_cali=_bool(body, "vibration_cali", False),
        layer_inspect=_bool(body, "layer_inspect", False),
        timelapse=_bool(body, "timelapse", False),
        bed_type=str(body.get("bed_type", "auto")),
        auto_bed_leveling=_small_int(body, "auto_bed_leveling", 2),
        auto_flow_cali=_small_int(body, "extrude_cali_flag", 2),
        auto_offset_cali=_small_int(body, "nozzle_offset_cali", 0),
        extruder_cali_manual_mode=_small_int(
            body,
            "extrude_cali_manual_mode",
            -1,
            minimum=-1,
        ),
        ext_change_assist=bool(cfg_bits & 1),
    )


def _derive_mapping2(mapping: tuple[int, ...]) -> list[dict[str, int]]:
    result: list[dict[str, int]] = []
    for tray in mapping:
        if tray < 0:
            result.append({"ams_id": 255, "slot_id": 255})
        elif tray >= 128:
            result.append({"ams_id": tray, "slot_id": 0})
        else:
            result.append({"ams_id": tray // 4, "slot_id": tray % 4})
    return result


def build_gateway_print_params(
    request: CloudPrintRequest, *, serial: str, file_path: Path
) -> dict[str, Any]:
    """Translate a LAN ``project_file`` into Gateway's official plugin ABI."""

    mapping = list(request.ams_mapping) if request.ams_mapping is not None else None
    mapping2: list[dict[str, int]] | None
    if request.ams_mapping2 is not None:
        mapping2 = [dict(entry) for entry in request.ams_mapping2]
    elif request.ams_mapping is not None:
        mapping2 = _derive_mapping2(request.ams_mapping)
    else:
        mapping2 = None

    compact = {"separators": (",", ":")}
    return {
        "dev_id": serial,
        "task_name": f"{request.project_name}_plate_{request.plate_index}",
        "project_name": request.project_name,
        "preset_name": "",
        "filename": str(file_path),
        "config_filename": str(file_path),
        "plate_index": request.plate_index,
        "nozzle_mapping": (
            json.dumps(list(request.nozzle_mapping), **compact)
            if request.nozzle_mapping is not None
            else ""
        ),
        "ams_mapping": json.dumps(mapping, **compact) if mapping is not None else "",
        "ams_mapping2": (json.dumps(mapping2, **compact) if mapping2 is not None else ""),
        "ams_mapping_info": "",
        "nozzles_info": "",
        # Gateway's pinned builder deliberately uses an empty value for its
        # pure-cloud route; retain that known-working convention.
        "connection_type": "",
        "task_bed_leveling": request.bed_leveling,
        "task_flow_cali": request.flow_cali,
        "task_vibration_cali": request.vibration_cali,
        "task_layer_inspect": request.layer_inspect,
        "task_record_timelapse": request.timelapse,
        "task_use_ams": request.use_ams,
        "task_bed_type": request.bed_type,
        "auto_bed_leveling": request.auto_bed_leveling,
        "auto_flow_cali": request.auto_flow_cali,
        "auto_offset_cali": request.auto_offset_cali,
        "extruder_cali_manual_mode": request.extruder_cali_manual_mode,
        "task_ext_change_assist": request.ext_change_assist,
        "dev_ip": "",
        "username": "",
        "password": "",
    }


def cloud_print_ack(request: CloudPrintRequest) -> dict[str, Any]:
    return {
        "print": {
            "command": "project_file",
            "sequence_id": request.sequence_id,
            "param": request.param,
            "subtask_name": request.project_name,
            "gcode_state": "PREPARE",
            "gcode_file": request.filename,
            "gcode_file_prepare_percent": "0",
            "result": "SUCCESS",
            "msg": 0,
        }
    }


def cloud_print_failure(*, sequence_id: str, reason: str, filename: str = "") -> dict[str, Any]:
    body: dict[str, Any] = {
        "command": "project_file",
        "sequence_id": sequence_id,
        "result": "FAILED",
        "msg": -1,
        "reason": f"cloud LAN proxy: {reason}",
    }
    if filename:
        body["gcode_file"] = filename
    return {"print": body}
