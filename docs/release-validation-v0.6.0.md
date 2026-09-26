# v0.6.0 release validation

Validation date: 2026-09-25/26 (America/Mexico_City)

This record intentionally omits serial numbers, IP addresses, access codes,
tokens, media URLs, and certificate material.

## Passed

| Area | Evidence |
| --- | --- |
| Backup and rollback | Private pre-cutover archive created and read-tested; previous Compose deployment remains available but stopped. |
| Rust deployment | Gateway, endpoint manager, address manager, and MQTT broker run the standalone Rust image and report healthy. |
| Bambuddy compatibility | Bambuddy connects to its virtual printer IP and receives live reports. |
| MQTT | Authenticated TLS report subscription received a current printer report. |
| Files | Ability, model/timelapse/video listing, native port-6000 login, and a 154,710-byte download passed. Local port 6000 is preferred with cloud fallback. |
| Storage mutation | A uniquely named 287-byte valid ZIP fixture uploaded to `emmc` and appeared in the model listing. The fixture exposed a firmware delete inconsistency described below. |
| Camera | Authenticated RTSPS returned H.264 at 1680×1080 through the virtual endpoint. |
| Controls | A signed chamber-light command changed the physical light and the original state was restored. |
| Recovery | MQTT, address-manager, gateway, endpoint-manager, and an individual relay failure recovered. Old endpoint process groups and camera workers were reaped; no matching zombies remained. |
| Security | Public host ports bind only to loopback. Gateway/MQTT have no capabilities; the endpoint manager has only `NET_BIND_SERVICE`; the address manager alone has `NET_ADMIN`; runtime application UIDs are non-root. |
| Build checks | Rust formatting/tests, Svelte checks/build, Compose validation, production image build, and whitespace checks pass. |

## Firmware observation

On the supervised printer, an uploaded synthetic history fixture is returned by
`LIST_INFO`, but the firmware rejects the official `paths` delete form with
`ERROR_JSON` and acknowledges the legacy name form without removing the entry.
The behavior was reproduced directly against local port 6000, outside the
proxy. The proxy now verifies legacy delete acknowledgements and reports a
failure if the entry remains. The synthetic entry must be removed from the
printer UI if the firmware continues to retain it; no existing user file was
modified during this test.

## Operator gates before merge/tag

- Confirm Bambu Handy and Bambu Studio still operate normally against the
  cloud-paired printer.
- Select a known-safe sliced job and supervise start, visible progress, and
  cancellation through the virtual endpoint. An arbitrary or synthetic 3MF is
  never used for this gate.
- A physical host reboot is optional but recommended. Container, broker,
  address-manager, and child-process recovery have already been exercised.

The branch must not be merged or tagged `v0.6.0` until the first two operator
gates are recorded as passed.
