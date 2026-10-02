# Full Rust migration plan

Status: implementation and supervised cutover were completed and merged to
`main` for v0.6.0. The Rust stack is the active deployment. Handy, Studio,
Bambuddy, and the supervised print/cancel test passed; see
`release-validation-v0.6.0.md`. A physical host reboot remains optional.
New admin/camera/capability features belong to the unpublished v0.7.0 candidate;
see `release-readiness.md` for its separate release and deployment gates.

## Target architecture

- A Rust workspace owns the cloud supervisor, policy engine, MQTT bridge,
  implicit FTPS server, native-file protocol, discovery, local telemetry,
  camera/TCP relays, storage API, and admin API.
- The proprietary Bambu plugin remains isolated behind the small C++ ABI host.
  Rust communicates with it over bounded JSON-lines RPC and never models STL
  objects directly.
- Mosquitto remains the LAN-facing MQTT broker and MediaMTX remains the RTSPS
  media implementation. They are independent infrastructure, not application
  code that benefits from being rewritten.
- A Svelte 5 static SPA built from official shadcn-svelte source components is
  embedded in the Rust admin service and served from the same authenticated
  origin.

## Delivered phases

1. Established a Cargo workspace, shared typed configuration/state/protocol
   models, compatibility fixtures, and a multi-stage build.
2. Ported cloud authentication, plugin download/integrity checks, JSON-RPC host
   supervision, device lifecycle, commands, policy, print staging, reports,
   and storage orchestration.
3. Ported implicit FTPS, port-6000 native files, SSDP discovery, local telemetry,
   certificate-pinned camera relay, and generic TCP relay.
4. Built the Svelte/shadcn-svelte admin application for setup, health, devices,
   policy gates, account sign-in, and export of the public proxy CA. The v0.7.0
   admin API authenticates that export; only the certificate itself is public.
5. Replaced Python images and Compose entrypoints, deleted Python runtime source,
   and updated bootstrap, CI, documentation, attribution, and operator tooling.
6. Deployed the production image, migrated the live stack, verified Bambuddy,
   telemetry, controls, local camera, local/cloud file browsing and download,
   safe file upload, process/broker recovery, least privilege, and rollback
   artifacts. The final operator-observed gates are tracked in
   `release-validation-v0.6.0.md`.

## Compatibility and safety gates

- Preserve existing environment names, ports, volumes, MQTT topics, usernames,
  and on-disk registry/session paths during migration.
- Treat timed-out writes as unknown outcomes and never replay them.
- Reject stale or unknown printer state before print submission.
- Keep remote upload/delete/reprint disabled unless explicitly enabled.
- Never expose account tokens, access codes, signing material, camera URLs, or
  native-plugin diagnostics through the admin UI or logs.
- No production service imports or ships Python and the offline checks pass.
  Physical control/printing parity remains
  an explicit supervised cutover step because CI cannot prove hardware state.
