# Security

This proxy joins two trust domains: a Bambu Cloud account and a local MQTT
client. Keep its listeners on trusted interfaces and never publish ports 322,
4844, 8883, 990, or 50000-50009 to the public internet.

The default policy is `read_only`. In that mode, only four explicit query
commands reach the cloud plugin. `controls` adds pause, resume, speed, and
light commands. Cancel remains independently disabled. `unsafe` forwards
unknown commands and should be used only for protocol development.

`full_control` forwards every structurally valid MQTT request through the
official cloud plugin except the separately guarded staged-print path. Treat it
as administrator access to the physical printer: it can include heating,
motion, G-code, calibration, and future commands this project does not know by
name. Use a unique access code and keep the listener on loopback or a private
authenticated network.

Cloud printing has its own `PROXY_CLOUD_PRINT_ENABLED` gate and remains off in
every policy mode until explicitly enabled. Jobs are accepted only from the
authenticated local MQTT listener, only for an online idle printer, and only
after the staged file passes path, size, and ZIP validation. Enabling it permits
real physical prints and should be treated as write access to the printer.

The following paths contain credentials and are ignored by Git:

- `.env` — the local MQTT access code
- `data/gateway/` — Rust proxy account/plugin state (the directory name is
  retained for deployment compatibility; Bambu Gateway is not installed)
- `data/proxy/certs/ca.key` — the local proxy CA private key
- `data/proxy/certs/server.key` — the local TLS server private key
- `data/proxy/auth/passwords` — the Mosquitto password database
- `data/proxy/runtime/` — least-privilege copies mounted into MQTT/FTPS/RTSPS
- `data/camera/` — the pinned physical camera TLS fingerprint
- `data/staging/` — user-supplied print files (which may contain private model
  geometry and metadata)

The camera service has host networking solely to reach the local printer, but
binds its client-facing listener to Docker's private host gateway. A separate
unprivileged ingress publishes only the authenticated RTSPS byte stream on the
configured proxy address. Both run read-only, without added Linux capabilities.

Only `data/proxy/export/bambu-cloud-lan-proxy-ca.crt` is intended for export.
It is a public certificate, not a private key.

To report a vulnerability, use GitHub's private security advisory feature for
this repository rather than opening a public issue.
