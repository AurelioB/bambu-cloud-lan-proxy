# Bambu Cloud LAN Proxy

A standalone, Docker Compose deployable LAN facade for a cloud-paired Bambu Lab printer. It lets Bambu Handy remain connected through Bambu Cloud while local software such as Bambuddy sees printer-shaped LAN services.

The application is Rust with an embedded Svelte 5/shadcn-svelte admin console. It does not install, import, fork, or run Bambu Gateway. The only proprietary component is Bambu's official networking plugin, downloaded at startup from Bambu's HTTPS endpoint and accepted only when its version and SHA-256 hashes match the reviewed pins in `src/plugin_install.rs`. A small C++ process isolates that plugin's C++ ABI from safe Rust.

## What it exposes

- MQTT over TLS on `8883`, with cloud reports and policy-controlled signed commands.
- Implicit FTPS on `990` plus passive ports `50000-50009` for staged jobs.
- Bambu's TLS native-file protocol on `6000` for previous jobs, timelapses, downloads, optional uploads, and optional deletion.
- RTSPS camera access on `322`, backed by the printer's local liveview and protected with certificate pinning.
- SSDP discovery for LAN clients.
- An authenticated admin console on `127.0.0.1:4844` by default.
- Optional local, read-only MQTT telemetry with automatic cloud-report fallback.

Writes use Bambu Cloud and the official plugin's device certificate. Read-only status normally comes from the cloud; if the printer's LAN MQTT endpoint is configured, fresh local reports take precedence. Camera traffic always uses the local printer because Bambu's cloud path does not provide the LAN RTSPS stream.

## Requirements

- Linux x86-64. The reviewed Bambu plugin build is x86-64 ELF.
- Docker Engine with Compose v2.
- The printer and proxy host on a trusted private network.
- For camera/local telemetry: the printer's LAN-only liveview enabled, its LAN IP, and its eight-character access code. The printer itself can remain cloud-paired; do not enable printer LAN-only mode.

Do not publish ports `322`, `990`, `6000`, `8883`, the passive FTPS range, or the admin console to the internet.

## Install

```sh
git clone git@github.com:AurelioB/bambu-cloud-lan-proxy.git
cd bambu-cloud-lan-proxy
./scripts/init.sh
```

Edit `.env` before the first full start:

```dotenv
PROXY_BIND_IP=192.168.1.20
PROXY_MODE=full_control
PROXY_ALLOW_CANCEL=true
PROXY_CLOUD_PRINT_ENABLED=true
PROXY_STORAGE_PRINTER_SERIAL=YOUR_SERIAL
PROXY_PRINTERS=YOUR_SERIAL

CAMERA_PRINTER_SERIAL=YOUR_SERIAL
CAMERA_UPSTREAM_IP=192.168.1.42
```

Put the printer's local access code into `data/gateway/printers.json`. The `gateway` name is retained only as an on-disk/Compose compatibility name; it is the Rust service:

```json
[
  {
    "serial": "YOUR_SERIAL",
    "name": "My H2C",
    "ip": "192.168.1.42",
    "access_code": "A1B2C3D4"
  }
]
```

The Rust cloud discovery process preserves `ip` and `access_code` while refreshing the printer name/model.

Start everything:

```sh
./scripts/deploy.sh
```

Open `http://127.0.0.1:4844` (or your configured `GATEWAY_BIND_IP:GATEWAY_PORT`) and enter `PROXY_ACCESS_CODE` from `.env`. Under **Bambu account**, open the official sign-in page, complete login, copy the browser's final `http://localhost:13618/?ticket=...` URL, and paste it into the console. Account tokens and certificate material are never shown in the UI.

## Bambuddy / LAN client setup

Use `PROXY_BIND_IP` as the printer address, the printer serial, username `bblp`, and `PROXY_ACCESS_CODE` as the access code. Install the generated proxy CA if the client validates the self-signed LAN certificate:

```sh
./scripts/export-ca.sh
```

The public CA is safe to distribute to your LAN client. Never copy files from `data/gateway/bambu-plugin/`, plugin state, `.env`, or the server private key.

## Safety controls

Policy can be changed in the admin console and persists across restarts:

- `read_only`: queries only.
- `controls`: pause, resume, speed, and light; cancellation is separately gated.
- `full_control`: forwards valid signed commands through the official plugin.

Cloud print submission, remote reprint, storage upload, storage deletion, and cancellation are independent opt-ins. A timed-out cloud write is recorded as an unknown outcome and is never automatically replayed. Staged jobs are confined to `/staging`, size checked, ZIP validated, copied to an immutable snapshot, and serialized per printer.

## Operations

```sh
docker compose ps
docker compose logs --tail=200 gateway mqtt ftps native-files camera local-status
./scripts/verify-ftps.sh
./scripts/verify-native.sh
./scripts/verify-camera.sh
./scripts/check.sh
```

Camera and local MQTT certificates use trust-on-first-use pins under `data/camera/` and `data/local-status/`. If the printer legitimately replaces a certificate, compare the new fingerprint out of band before removing the corresponding pin file.

To update, pull the reviewed source and rebuild. Plugin updates intentionally fail closed until their version and hashes are reviewed and updated in source.

## Architecture and limits

See [docs/architecture.md](docs/architecture.md), [docs/gateway-independence.md](docs/gateway-independence.md), and [docs/rust-migration-plan.md](docs/rust-migration-plan.md).

This is an independent community project, not affiliated with Bambu Lab, Bambu Gateway, or Bambuddy. Cloud behavior is subject to Bambu's service and account policies. Prefer official local interfaces whenever Bambu makes them available.

License: MIT. Third-party notices are in `THIRD_PARTY_NOTICES`.
