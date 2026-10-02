# Bambu Cloud LAN Proxy

A standalone, Docker Compose deployable LAN facade for cloud-paired Bambu Lab printers. It lets Bambu Handy remain connected through Bambu Cloud while local software such as Bambuddy sees one independent LAN endpoint per printer.

The application is Rust with an embedded Svelte 5/shadcn-svelte admin console. It does not install, import, fork, or run Bambu Gateway. The only proprietary component is Bambu's official networking plugin, downloaded at startup from Bambu's HTTPS endpoint and accepted only when its version and SHA-256 hashes match the reviewed pins in `src/plugin_install.rs`. A small C++ process isolates that plugin's C++ ABI from safe Rust.

## Community website and documentation

The independent public site in [`website/`](website/README.md) includes a landing page, searchable guides, and an [OpenAPI reference](website/public/openapi.json). It never connects to private printer APIs. Preview it with `cd website && npm ci && npm run dev`.

- [Docker Compose installation](website/src/content/docs/docs/install/compose.md) — recommended.
- [Docker without Compose](website/src/content/docs/docs/install/docker.md).
- [Standalone Linux installation](website/src/content/docs/docs/install/standalone.md), with [systemd/configuration examples](examples/standalone/).
- [Features](website/src/content/docs/docs/features.md), [compatibility](website/src/content/docs/docs/compatibility.md), [HTTP API](website/src/content/docs/docs/api.md), and [operations](website/src/content/docs/docs/operations.md).

The guides describe the current source tree, including features added after the original v0.6.0 release. They do not assume a public container image or prebuilt native binary. Static-site publication is opt-in; see the website README. The repository must be accessible to a reader before clone instructions can work.

## What it exposes

- One private virtual IP per printer, all backed by one shared cloud login.
- MQTT over TLS on `8883`, with serial-isolated cloud reports and policy-controlled signed commands.
- Implicit FTPS on `990` plus passive ports `50000-50009` for staged jobs.
- Bambu's TLS native-file protocol on `6000` for previous jobs, timelapses, downloads, optional uploads, and optional deletion. Printer-resident file operations prefer the configured local printer connection and fall back to the official cloud media tunnel.
- RTSPS camera access on `322`, backed by the printer's local liveview and protected with certificate pinning.
- Local image-camera access for A1 / A1 mini / A2L / P1P / P1S on `6000`, multiplexed with native files and shared across viewers.
- SSDP discovery for LAN clients.
- An authenticated admin console on `127.0.0.1:4844` by default.
- Optional local, read-only MQTT telemetry with automatic cloud-report fallback.

Control and print writes use Bambu Cloud and the official plugin's device certificate. Read-only status normally comes from the cloud; if the printer's LAN MQTT endpoint is configured, fresh local reports take precedence. File browsing uses the local native-file service when local credentials are configured, with cloud fallback. Camera traffic always uses the local printer because Bambu's cloud path does not provide the LAN RTSPS stream.

## Requirements

- Linux x86-64. The reviewed Bambu plugin build is x86-64 ELF.
- Docker Engine with Compose v2.
- The printer and proxy host on a trusted private network.
- For camera/local telemetry: the printer's LAN-only liveview enabled, its LAN IP, and its eight-character access code. The printer itself can remain cloud-paired; do not enable printer LAN-only mode.

Do not publish ports `322`, `990`, `6000`, `8883`, the passive FTPS range, or the admin console to the internet.

## Install

```sh
git clone https://github.com/AurelioB/bambu-cloud-lan-proxy.git
cd bambu-cloud-lan-proxy
./scripts/init.sh
```

Edit `.env` before the first full start. For LAN addresses, set the Linux interface that will carry the reserved virtual printer IPs:

```dotenv
PROXY_ENDPOINT_INTERFACE=eth0
PROXY_MODE=read_only
PROXY_ALLOW_CANCEL=false
PROXY_CLOUD_PRINT_ENABLED=false
PROXY_PRINTERS=
```

Start the shared cloud core:

```sh
./scripts/deploy.sh
```

Open `http://127.0.0.1:4844` (or your configured `GATEWAY_BIND_IP:GATEWAY_PORT`) and enter `PROXY_ACCESS_CODE` from `.env`. Under **Bambu account**, complete the official sign-in flow. The account printers are discovered automatically.

Under **Printers → Configure** (also available in **Connections**), give every printer:

- a unique reserved virtual IPv4 address;
- its physical LAN IP if camera or local telemetry is wanted;
- its physical eight-character LAN liveview access code if using those local services.

The private registry then resembles:

```json
[
  {
    "serial": "YOUR_SERIAL",
    "name": "Workshop printer",
    "machine_model": "X1 Carbon",
    "virtual_ip": "192.168.1.201",
    "endpoint_enabled": true,
    "ip": "192.168.1.42",
    "access_code": "A1B2C3D4"
  }
]
```

The Rust cloud discovery process preserves endpoint and local-access fields while refreshing the printer name/model. Do not edit this file while the service is running; use the admin console.

Apply the addresses and regenerate the proxy certificate with every virtual IP in its SAN:

```sh
./scripts/apply-endpoints.sh
```

The narrowly scoped address manager is the only long-running container with `NET_ADMIN`. It reconciles the configured aliases every 15 seconds, removes only addresses recorded in its private managed-address state, and restores them automatically after a host reboot. The cloud core and endpoint manager do not receive network-administration capability. On a standalone Linux installation, run `endpoint-address-manager` under an equivalent supervised boot service; `endpoint-addresses` remains available as a one-shot reconciliation command.

The endpoint manager publishes per-printer MQTT, FTPS, native-file, and applicable camera health to the admin console. Core endpoint processes are restarted automatically; failure of the isolated native plugin exits the cloud container so Compose can restart it without replaying commands.

## Admin workspace

The responsive console includes searchable printer cards and a compact list, per-printer connection editing, a connection guide, account sign-in, and explicit permission controls. Choose light, dark, or system appearance from the header. Camera support and unknown model capabilities are labelled individually; live video and print controls remain in your connected apps.

Optional bundled local administrator login and OIDC protect **only the admin UI and its APIs**. Virtual-printer access codes and client compatibility are unchanged. The default remains access-code sign-in; no mode allows anonymous private access. See [admin authentication setup and security boundaries](docs/admin-authentication.md) for local, OIDC-only, and combined modes.

See [the UI design review](docs/ui-redesign.md) for reference comparisons, the complete shadcn-svelte component audit, and browser-test instructions.

## Bambuddy / LAN client setup

Create one Bambuddy printer entry for every configured endpoint. Use that printer's `virtual_ip`, its real serial, username `bblp`, and the shared `PROXY_ACCESS_CODE`. Each address exposes the standard ports independently. Install the generated proxy CA if the client validates the self-signed LAN certificate:

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
docker compose logs --tail=200 gateway mqtt endpoint-manager
./scripts/verify-ftps.sh
./scripts/verify-native.sh
./scripts/verify-camera.sh
./scripts/check.sh
```

Camera and local MQTT certificates use trust-on-first-use pins under `data/camera/` and `data/local-status/`. If the printer legitimately replaces a certificate, compare the new fingerprint out of band before removing the corresponding pin file.

The admin API derives attached AMS units, reported filament slots, firmware functions, and sensors from fresh telemetry. Local evidence takes precedence; cloud evidence fills missing fields, and conservative model profiles fill remaining gaps. `observed_fields` identifies the evidence-backed fields. Stale observations expire rather than inventing capabilities.

RTSPS-capable models use the local port-322 bridge. A1, A1 mini, A2L, P1P, and P1S image clients use the same virtual port `6000` as native files, with distinct authenticated protocol dispatch. The proxy validates the proxy access code, replaces it with the physical printer code upstream, pins the camera certificate, and shares one on-demand camera connection across viewers. No extra port or transcoder is needed for Bambuddy's image protocol. Unknown models do not automatically gain a camera or toolhead profile. The image path has automated TLS/protocol tests; physical A1/P1-family validation remains outstanding.

To update, pull the reviewed source and rebuild. Plugin updates intentionally fail closed until their version and hashes are reviewed and updated in source.

## Architecture and limits

See [docs/architecture.md](docs/architecture.md), [docs/gateway-independence.md](docs/gateway-independence.md), [docs/rust-migration-plan.md](docs/rust-migration-plan.md), and the [v0.6.0 validation record](docs/release-validation-v0.6.0.md).

This is an independent community project, not affiliated with Bambu Lab, Bambu Gateway, or Bambuddy. Cloud behavior is subject to Bambu's service and account policies. Prefer official local interfaces whenever Bambu makes them available.

License: MIT. Third-party notices are in `THIRD_PARTY_NOTICES`.
