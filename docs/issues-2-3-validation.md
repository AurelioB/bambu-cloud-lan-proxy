# GitHub issues 2 and 3: implementation and validation

This record describes the current working-tree implementation, not a deployed release or physical fleet certification.

## Issue 2: image-camera transport

[Issue 2](https://github.com/AurelioB/bambu-cloud-lan-proxy/issues/2) previously had model identification but no image transport. The implementation now covers A1, A1 mini, A2L, P1P, and P1S profiles. P1S was incorrectly classified as RTSPS and is corrected.

The native-file listener owns virtual TLS port 6000. It dispatches the image protocol's fixed 16-byte header separately from the existing file protocol, validates each protocol's credentials, and rejects unknown headers. Image authentication uses the proxy access code downstream and the physical access code upstream. Upstream camera TLS uses a separate persistent `SERIAL-image.sha256` pin, or an explicitly configured fingerprint.

One on-demand physical camera connection fans out the latest frame to authenticated viewers. Reads and writes have timeouts, frame sizes are bounded, incomplete/invalid JPEG framing is rejected, reconnects continue while viewers remain, and the last viewer disconnect releases the upstream connection. This is a compatible Bambu image transport; it is not a new HTTP endpoint or JPEG-to-H.264 conversion.

Normal multi-printer endpoints configure it automatically when the model, physical IP, and printer access code are available. The same implementation is used standalone and in Compose. Native-file operation remains available when an image camera is absent or unreachable. Unknown camera profiles remain disabled. Endpoint camera health uses port 6000 for image profiles and port 322 for RTSPS; listener health does not verify a live frame.

Advanced operators running `native-files` directly can configure `IMAGE_CAMERA_ENABLED`, `IMAGE_CAMERA_UPSTREAM_IP`, `IMAGE_CAMERA_UPSTREAM_PORT` (default 6000), `IMAGE_CAMERA_REGISTRY`, `IMAGE_CAMERA_STATE_DIR`, and `IMAGE_CAMERA_TLS_FINGERPRINT`. The printer selection uses `FTPS_REMOTE_SERIAL`. Existing FTPS TLS/code configuration protects the shared listener.

Automated tests cover:

- Fragmented logins and JPEG frames, padded credentials, and distinct protocol headers.
- Native-file login/setup and camera viewing concurrently on one TLS listener.
- Rejection of invalid camera credentials and unknown headers.
- Replacement of the downstream proxy code with the physical printer code.
- Shared viewers, last-viewer teardown, upstream reconnects, and certificate-pin rejection.

Physical A1/P1-family testing with Bambuddy/Studio is still needed. The existing real H2C RTSPS validation does not establish image-protocol compatibility on every firmware.

## Issue 3: observed capabilities

[Issue 3](https://github.com/AurelioB/bambu-cloud-lan-proxy/issues/3) previously exposed only model profiles. Cloud reports now update sanitized per-printer evidence before virtual report rewriting. The read-only local worker projects evidence into the shared local-status directory; client-published virtual reports are not a capability source.

Fresh local evidence takes precedence (configured local-report window, 20 seconds by default). Fresh cloud evidence fills missing fields (90 seconds); conservative model profiles fill remaining gaps. Stale/future-dated observations are excluded. Evidence includes OTA firmware version, AMS units and observed slot IDs, sensors, dual-extruder data, and reported firmware function bits. Unknown counts stay null, while explicit absence is zero. Missing delta fields preserve earlier observations; AMS existence bits remove detached legacy units. Firmware version changes clear earlier function flags. Unrecognized models do not acquire an assumed camera/toolhead profile merely because AMS data was received.

The overview exposes `source`, `model_known`, `observed_fields`, and nullable `runtime`. Admin cards show reported AMS counts and slots; configuration details show firmware, sensors, and reported functions. Functions describe telemetry evidence and never expand proxy permissions or imply successful command execution.

Automated tests cover partial AMS reports, detachments, unknown slot counts, fresh/stale evidence, local precedence with cloud version fallback, serialization, firmware changes, dummy legacy chamber temperatures, unknown models, and capability presentation in the protected admin UI.

## Checks

- `cargo fmt --check`
- `cargo test --locked --offline`: 29 tests passed.
- Admin `npm run check`, `npm run build`, `npm run test:ui`: 18 browser tests passed, including accessibility checks.
- Website `npm run check`, `npm run build`: 20 pages and 986 local links/assets checked.
- `docker compose config --quiet`
- `git diff --check`
- Production `docker build -f Dockerfile.proxy -t bambu-cloud-lan-proxy:issues-2-3-check .` passed, including the optimized Rust build and embedded admin UI.
- Offline image smoke check: `docker run --rm --network none bambu-cloud-lan-proxy:issues-2-3-check --version`.

## Protocol references reviewed

- [Bambuddy camera transport](https://github.com/maziggy/bambuddy/blob/main/backend/app/services/camera.py): image login framing, JPEG transport, and RTSPS/image model distinction.
- [Home Assistant Bambu models](https://github.com/greghesp/ha-bambulab/blob/main/custom_components/bambu_lab/pybambu/models.py): delta reports, module versions, device/extruder fields, and model-specific placeholder behavior.
- [Bambu Studio DeviceManager](https://github.com/bambulab/BambuStudio/blob/master/src/slic3r/GUI/DeviceManager.cpp): firmware-reported home/function flags.

These sources were reviewed to understand interoperability. The transport and observation implementation are independent Rust code and introduce no Gateway runtime dependency.
