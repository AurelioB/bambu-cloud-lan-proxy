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
| Controlled print | The most recent known-good printer-resident job was re-submitted through the virtual MQTT endpoint with its recorded plate and AMS mapping. Fresh telemetry identified the expected title in `RUNNING` at 0%; cancellation was then sent through the same endpoint and fresh status settled at `FAILED`, 0%, zero remaining time, with nozzle and bed near ambient. No replay was attempted. |
| Bambu Handy continuity | The operator confirmed that Bambu Handy on iPhone remained connected and operational after the proxy deployment and controlled print/cancel test. |
| Recovery | MQTT, address-manager, gateway, endpoint-manager, and an individual relay failure recovered. Old endpoint process groups and camera workers were reaped; no matching zombies remained. |
| Security | Public host ports bind only to loopback. Gateway/MQTT have no capabilities; the endpoint manager has only `NET_BIND_SERVICE`; the address manager alone has `NET_ADMIN`; runtime application UIDs are non-root. |
| Build checks | Rust formatting/tests, Svelte checks/build, Compose validation, production image build, and whitespace checks pass. |

## Firmware observation

On the supervised printer, an uploaded synthetic history fixture is returned by
`LIST_INFO`, but the firmware rejects the official `paths` delete form with
`ERROR_JSON` and acknowledges the legacy name form without removing the entry.
The behavior was reproduced directly against local port 6000, outside the
proxy. Independent protocol research also documents internal model-history
deletion as unsupported through port 6000. The proxy now rejects that category
with an explicit internal diagnostic, verifies other legacy delete
acknowledgements, and reports a failure if an entry remains. The synthetic entry
must be removed by long-pressing it under **Print Files → Internal** on the
printer; no existing user file was modified during this test.

## Operator gates before merge/tag

- Confirm Bambu Studio still operates normally against the cloud-paired
  printer.
- A physical host reboot is optional but recommended. Container, broker,
  address-manager, and child-process recovery have already been exercised.

The branch must not be merged or tagged `v0.6.0` until Studio continuity is
recorded as passed.
