# Security

This proxy joins two trust domains: a Bambu Cloud account and a local MQTT
client. Keep its listeners on trusted interfaces and never publish ports 4844
or 8883 to the public internet.

The default policy is `read_only`. In that mode, only four explicit query
commands reach the cloud plugin. `controls` adds pause, resume, speed, and
light commands. Cancel remains independently disabled. `unsafe` forwards
unknown commands and should be used only for protocol development.

The following paths contain credentials and are ignored by Git:

- `.env` — the local MQTT access code
- `data/gateway/` — Bambu Gateway account session data
- `data/proxy/certs/ca.key` — the local proxy CA private key
- `data/proxy/certs/server.key` — the local TLS server private key
- `data/proxy/auth/passwords` — the Mosquitto password database

Only `data/proxy/export/bambu-cloud-lan-proxy-ca.crt` is intended for export.
It is a public certificate, not a private key.

To report a vulnerability, use GitHub's private security advisory feature for
this repository rather than opening a public issue.
