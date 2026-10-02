---
title: Requirements & compatibility
description: Supported hosts, printer capabilities, and known limitations.
---
## Host requirements

- **Linux x86-64**, on a trusted private network. The pinned official networking plugin is x86-64 ELF; ARM hosts and Raspberry Pi are not supported by this build.
- Native Docker Engine with Compose v2 for the recommended installation, or the standalone build dependencies.
- A cloud-paired Bambu printer and access to the correct regional Bambu account.
- Outbound HTTPS and the connectivity needed by Bambu's plugin/cloud services.
- Reserved, unused IPv4 addresses on the host LAN, outside the DHCP pool, if clients run on other machines.
- Disk space for image/source builds, plugin state, staging, and temporary file downloads. Defaults permit a 1 GiB staged job and 10 GiB staging quota; leave additional room for caches.

Docker Desktop on macOS/Windows, rootless container networking, ARM emulation, and arbitrary NAS container environments are **not validated deployment targets** for the host-address implementation.

## Printer capabilities

Fresh reports supply observed AMS units, filament slots, firmware functions, sensors, and dual-extruder evidence. Model profiles fill missing information; they are **not a certification that every firmware version works**. The admin console separates reported capabilities from endpoint health.

| Profile group in current code | Camera path | Notes |
| --- | --- | --- |
| X1 family, P2S, X2D, H2 family | Local RTSPS bridge | Requires actual local port-322 availability, LAN liveview, IP, and printer code |
| A1 / A1 mini / A2L / P1P / P1S | Local image relay on shared port 6000 | Requires local image access, IP, and printer code; automated protocol validation, physical fleet validation pending |
| Unknown model | Unknown; camera disabled conservatively | Report the model identifier before assuming support |

`observed_fields` identifies capabilities derived from reports; remaining toolhead/chamber fields use model metadata. Firmware-reported support does not change policy permissions or guarantee command acceptance. Profiles and observation logic live in `src/capabilities.rs`.

The existing supervised validation record covers a real H2C installation with Bambuddy, Handy, and Studio. It is not evidence of a tested fleet of every Bambu model. Community reports should include model, firmware, client version, and the exact operation.

## Local services are conditional

Keep the printer **cloud-paired**. Enable **LAN-only liveview** for supported local camera access; do not confuse it with switching the printer into LAN-only mode.

Enabling liveview does not guarantee that firmware also permits local MQTT or file access. The proxy prefers fresh local telemetry when available and falls back to cloud reports. Native files prefer local access with cloud-tunnel fallback. Camera has no cloud fallback.

## Known limits

- No browser live-video or print-control panel in the admin console. Use a connected printer client.
- No DHCP client per virtual printer, no automatic router reservations, and no automatic cross-VLAN discovery.
- One Bambu account per core instance; no multi-tenant authorization or per-printer users.
- All LAN clients share the proxy device code. Separate IPs do not form security boundaries.
- Some firmware cannot delete internal model-history files through the native file protocol, even when it acknowledges a request.
- Signed commands depend on cloud service behavior and reviewed plugin compatibility. No firmware bypass or universal feature guarantee is implied.
