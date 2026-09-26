# Architecture

```text
Bambu Handy ───────────────┐
                           v
Printer(s) <─> Bambu Cloud <────> Rust session service + native plugin shim
                                |            ^  |
                         media tunnel        |  | raw report / checked request
                                v            |  v
                         Rust FTPS <── staged 3MF + Mosquitto TLS :8883
                              ^                         ^
                              └──────── Bambuddy ───────┘

Printer RTSPS :322 ──> MediaMTX (on-demand, cert-pinned) ──> endpoint :322
                                                               ^
                                                               └── Bambuddy
```

A separate read-only client also attempts the printer's local MQTT/TLS port.
It pins that endpoint's certificate and republishes only report messages. A
fresh heartbeat makes the cloud bridge suppress duplicate cloud reports; if
LAN telemetry stops, cloud reporting resumes automatically.

The Rust service orchestrates authentication and subscriptions through
the official networking plugin. The plugin owns signing and persisted token
refresh. Our native subprocess communicates via private JSON-lines RPC:

1. `Session.events` publishes the original cloud MQTT JSON to
   `device/<serial>/report` without reconstructing printer state.
2. The embedded facade subscribes to `device/+/request` on an internal broker.
3. A fail-closed policy classifies each request.
4. Read-only requests pass through unchanged. Allowlisted controls are rebuilt
   with our own command builders. In `full_control`, other structurally
   valid envelopes pass through unchanged. In both cases, the official plugin
   owns cloud signing and transmission.

The FTPS container provides a confined, quota-limited filesystem at
`data/staging/`. Uploads are streamed into hidden partial files, capped,
ZIP-checked when they end in `.3mf`, flushed, then atomically renamed. Listings
never expose partial/internal files. Both control and `PROT P` data channels
use the locally generated server certificate.

For printer-resident files, our native adapter asks the same official networking plugin
used by Studio to mint a short-lived authenticated `bambu:///tutk` URL. The
native host passes that URL directly to the official `libBambuSource` control
stream; the URL is never logged, persisted, or returned to FTPS. `LIST_INFO`
responses are projected into `cache/`, `timelapse/`, and `video/`. A `RETR`
causes an on-demand `FILE_DOWNLOAD` into `data/remote-cache/`, validates size
and MD5, streams it over the TLS port-6000 protocol, then asks the proxy to remove it. The
same bridge supports ability, sub-file, upload, delete, and cancel operations.
Remote writes are separately gated. A native TLS port-6000 facade translates
BambuTunnelLocal framing and emits a list-change notice after mutations.

When cloud printing is explicitly enabled, `print.project_file` is never
forwarded as raw MQTT: its `ftp://` URL would only be meaningful to the local
facade. Instead the cloud bridge:

1. validates the request and checks that the live cloud state is idle;
2. acknowledges it so MQTT-first slicers may begin their FTPS upload;
3. waits for the atomic staged file and snapshots it under the proxy's data
   volume;
4. maps plate, AMS, H2C nozzle, calibration, and timelapse options into the
   pinned plugin's `PrintParams` ABI;
5. invokes `CloudPrinter.submit_print`, then removes the snapshot only after
   the native operation has returned. Unknown outcomes preserve the snapshot
   and a persistent per-printer safety latch under `data/gateway/submissions/`.

One lock per physical serial plus request-digest replay tracking prevents
parallel or QoS-replayed jobs from becoming duplicate starts. The printer state
is checked again immediately before the plugin call.

The endpoint manager watches the private printer registry and maintains one
serial-pinned listener bundle per configured virtual IP. Every bundle exposes
the normal Bambu ports while sharing one cloud session and internal MQTT
broker. Per-printer staging directories and injected serials prevent file and
command routing from crossing endpoint boundaries.

The camera is the exception because H-series LAN-only liveview remains a direct
local RTSPS service while cloud pairing is active. An unprivileged MediaMTX
process uses the physical access code, pins the upstream leaf certificate
(explicitly or by persistent trust-on-first-use), and pulls only when a client
requests `/streaming/live/1`. A separate unprivileged byte relay lets Docker
publish standard port 322 without giving MediaMTX root or low-port capability.
The downstream side uses the proxy certificate and proxy access code. This
path is local-only and cannot fall back to Bambu's proprietary cloud video.

## Certificate boundaries

There are three unrelated certificate concerns:

- **Bambu cloud credentials:** kept and refreshed by Bambu's official plugin.
  This project never extracts or redistributes its signing key.
- **Physical local TLS:** the telemetry/camera clients pin the physical leaf
  certificates. The new cloud-only native shim uses the system public CA store.
- **Proxy TLS:** generated locally by the Compose bootstrap and used for MQTT,
  FTPS, and RTSPS. The CA private key stays under `data/`;
  `scripts/export-ca.sh` exports only its public certificate. The runtime
  containers mount a separate `data/proxy/runtime/` projection with the server
  certificate/key (and, for MQTT, the hashed password database); neither gets
  the CA signing key.

## Deliberate upstream boundaries

The remote file facade exposes the file categories returned by Studio's media
API: previous models/jobs, timelapses, and recordings. It does not expose an
arbitrary remote filesystem. Physical upload and deletion are explicit
opt-ins; rename remains a local staging operation.

The H2C cloud live view is a proprietary TUTK/Agora session rather than RTSPS
forwarded through Bambu's MQTT broker. Our native adapter does not expose that video stack.
The proxy therefore relays only the direct LAN-only liveview endpoint; if the
printer closes port 322 or leaves the local network, there is no cloud fallback.

The port-6000 facade emulates the documented file-browser subset, not every
firmware-private service sharing that port. Stock Studio may also require its
own trusted Bambu certificate chain; custom clients can trust the public proxy
CA exported by `scripts/export-ca.sh`.
