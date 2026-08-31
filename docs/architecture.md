# Architecture

```text
Bambu Handy ───────────────┐
                           v
H2C <──────> Bambu Cloud <────> Bambu Gateway + compatibility hook
                                             ^  |
                         signed cloud upload |  | raw report / checked request
                                             |  v
                                     staged 3MF + Mosquitto TLS :8883
                                         ^             ^
                                         | FTPS :990   |
                                         +── Bambuddy ─+

H2C RTSPS :322 ──> MediaMTX (on-demand, cert-pinned) ──> TCP ingress :322
                                                               ^
                                                               └── Bambuddy
```

Gateway's official network-plugin subprocess owns cloud authentication,
subscription, command signing, and token refresh. This project patches one
stable Python boundary in the pinned Gateway image:

1. `CloudPrinterClient.handle_event` publishes the original cloud MQTT JSON to
   `device/<serial>/report` without reconstructing printer state.
2. The embedded facade subscribes to `device/+/request` on an internal broker.
3. A fail-closed policy classifies each request.
4. Read-only requests pass through unchanged. Allowlisted controls are rebuilt
   with Gateway's own command builders. In `full_control`, other structurally
   valid envelopes pass through unchanged. In both cases, the official plugin
   owns cloud signing and transmission.

The FTPS container provides a confined, quota-limited filesystem at
`data/staging/`. Uploads are streamed into hidden partial files, capped,
ZIP-checked when they end in `.3mf`, flushed, then atomically renamed. Listings
never expose partial/internal files. Both control and `PROT P` data channels
use the locally generated server certificate.

When cloud printing is explicitly enabled, `print.project_file` is never
forwarded as raw MQTT: its `ftp://` URL would only be meaningful to the local
facade. Instead the Gateway hook:

1. validates the request and checks that the live cloud state is idle;
2. acknowledges it so MQTT-first slicers may begin their FTPS upload;
3. waits for the atomic staged file and snapshots it under Gateway's data
   volume;
4. maps plate, AMS, H2C nozzle, calibration, and timelapse options into the
   pinned plugin's `PrintParams` ABI;
5. invokes `CloudPrinterClient.submit_print`, then removes the snapshot.

One lock per physical serial plus request-digest replay tracking prevents
parallel or QoS-replayed jobs from becoming duplicate starts. The printer state
is checked again immediately before the plugin call.

The downstream TLS listener speaks the same username, topics, TLS, and port
that Bambuddy expects from a LAN printer. It is an application-layer facade;
it does not change the H2C's network mode or expose the physical printer's LAN
ports.

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

- **Bambu cloud credentials:** kept and refreshed by Bambu Gateway's official
  plugin. This project never extracts or redistributes its signing key.
- **Bambu printer CA:** already bundled by upstream Gateway for direct local
  printer verification. It is not the certificate Bambuddy sees here.
- **Proxy TLS:** generated locally by the Compose bootstrap and used for MQTT,
  FTPS, and RTSPS. The CA private key stays under `data/`;
  `scripts/export-ca.sh` exports only its public certificate. The runtime
  containers mount a separate `data/proxy/runtime/` projection with the server
  certificate/key (and, for MQTT, the hashed password database); neither gets
  the CA signing key.

## Deliberate upstream boundaries

The facade can archive only files present in its staged filesystem. The pinned
official plugin supports cloud upload/print submission but exposes no source
3MF download primitive, so a Handy-originated job cannot be reconstructed into
FTPS storage.

The H2C cloud live view is a proprietary TUTK/Agora session rather than RTSPS
forwarded through Bambu's MQTT broker. Gateway does not expose that video stack.
The proxy therefore relays only the direct LAN-only liveview endpoint; if the
printer closes port 322 or leaves the local network, there is no cloud fallback.

The physical printer's FTPS/storage service is likewise unavailable while its
LAN service ports are closed. The facade can stage and re-serve files locally,
then use the official cloud upload/dispatch path, but it cannot list or download
files that exist only on the printer.
