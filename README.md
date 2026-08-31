# Bambu Cloud LAN Proxy

A private-by-default bridge that makes a printer connected through **Bambu
Cloud** look like a local Bambu MQTT endpoint. Its first target is monitoring a
Bambu Lab H2C from [Bambuddy](https://github.com/maziggy/bambuddy) without
turning off Bambu Handy.

```text
H2C ↔ Bambu Cloud ↔ Bambu Gateway ↔ local MQTT/TLS facade ↔ Bambuddy
```

The H2C stays in normal cloud mode. [Bambu
Gateway](https://github.com/leolobato/bambu-gateway) uses Bambu's official
network plugin for cloud authentication and signed traffic. This project adds
a small compatibility hook and a hardened Mosquitto listener; it does not
reverse proxy Bambu's public broker or place account credentials in Bambuddy.

## What works

- Raw H2C status, temperatures, progress, AMS, errors, and version reports
- Bambuddy's `pushall`, `get_version`, `get_accessories`, and
  `extrusion_cali_get` queries
- Optional signed pause, resume, speed, and light controls
- Bambu Handy at the same time, because the printer remains cloud-connected
- Automatic local TLS CA/server certificate and MQTT credential creation
- A reproducible Docker Compose deployment pinned to Bambu Gateway 3.0.0

The default is deliberately read-only. The project does **not yet** emulate
implicit FTPS (port 990), the camera, or the printer file system, so Bambuddy's
file-transfer diagnostic and direct queue dispatch will not work through this
version. Cloud printing through Handy or Gateway remains available.

## Quick start

Requirements: Docker Engine with Compose v2 and a Linux host. Clone the repo,
then run:

```sh
./scripts/init.sh
./scripts/deploy.sh
```

The first command creates `.env`, an eight-character local access code, and a
local certificate authority. The second builds and starts the pinned Gateway
image plus the TLS MQTT broker.

Open Gateway at `http://127.0.0.1:4844`, then connect the cloud account in
**Settings → Cloud Account**. Bambu's redirect to `localhost:13618` is expected
to fail in the browser; copy the complete redirect URL back into Gateway's
**Complete sign-in** field. The session is stored only in `data/gateway/`.

If a Gateway deployment is already signed in, import it before starting this
stack:

```sh
./scripts/import-gateway-data.sh /path/to/existing/gateway-data
```

The destination must be empty; the script refuses to merge credentials.

## Add the cloud printer to Bambuddy

Use **Printers → Add printer → Manual** and enter:

| Bambuddy field | Value |
| --- | --- |
| IP address | `127.0.0.2` when Bambuddy uses host networking on this host |
| Serial number | Exact physical printer serial shown in Gateway |
| Access code | `PROXY_ACCESS_CODE` from `.env` |
| Model | The physical model, for example H2C |

The MQTT username is always `bblp`, matching a physical printer. Bambuddy
disables TLS verification for printer MQTT, so it does not need the generated
CA. Other clients can obtain the public certificate with:

```sh
./scripts/export-ca.sh
```

If Bambuddy is on another machine, set `PROXY_BIND_IP` in `.env` to this
Docker host's trusted LAN or Tailscale address, rerun `./scripts/init.sh` to
issue a certificate for that address, and redeploy. Firewall TCP/8883 so only
the Bambuddy host can reach it.

## Policy modes

Configure the mode in `.env`, then run `docker compose up -d`:

| Mode | Behavior |
| --- | --- |
| `read_only` | Startup/status queries only; default and recommended initially |
| `controls` | Adds pause, resume, print-speed, and chamber/work light |
| `unsafe` | Forwards unrecognized request envelopes; protocol development only |

Cancel is destructive enough to require a second switch:

```dotenv
PROXY_MODE=controls
PROXY_ALLOW_CANCEL=true
```

When controls are active, the facade changes only the downstream `fun`
capability bit so Bambuddy knows the **facade** accepts controls. It does not
alter what the cloud printer reports upstream or change the physical printer's
LAN/Developer Mode setting.

Restrict a multi-printer cloud account with a comma-separated allowlist:

```dotenv
PROXY_PRINTERS=SERIAL_ONE,SERIAL_TWO
```

## Verify and troubleshoot

```sh
docker compose ps
docker compose logs --tail=200 gateway mqtt
./scripts/verify-mqtt.sh YOUR_PRINTER_SERIAL
```

The verification subscribes through the public TLS listener and waits for one
real cloud report. It sends no printer command.

- **Bambuddy connects but shows no data:** confirm its serial matches Gateway
  exactly; MQTT topics are case-sensitive. Wait up to 30 seconds for Gateway's
  periodic full snapshot.
- **Authentication refused:** copy the eight-character code from `.env` again,
  then rerun `./scripts/init.sh` and restart `mqtt` if it changed.
- **FTPS/port 990 fails:** expected in this release; MQTT telemetry is separate.
- **Gateway has no printers:** complete its Bambu cloud sign-in and confirm the
  correct `BAMBU_CLOUD_REGION` (`US`, `EU`, or `CN`).

## Why no Bambu certificate extraction?

[open-bamboo-networking](https://github.com/ClusterM/open-bamboo-networking)
can fetch slicer-side credentials because it replaces the slicer's network
plugin. This proxy has a different boundary: upstream Gateway already loads
the official plugin and owns its cloud credentials. Extracting or shipping
Bambu's shared private signing material would add both security and maintenance
risk without helping Bambuddy.

The Compose bootstrap instead creates the only certificate this new endpoint
needs: a private local CA and MQTT server certificate. See
[the architecture notes](docs/architecture.md) for the separation.

## Development

```sh
python -m venv .venv
. .venv/bin/activate
python -m pip install -e '.[dev]'
./scripts/check.sh
docker compose build gateway
```

The derived image is intentionally small: it pins Gateway v3.0.0 by digest and
hooks the raw `CloudPrinterClient.handle_event` boundary at startup. If that API
changes, startup fails rather than silently exposing a broken proxy.

This project is independent and is not affiliated with or endorsed by Bambu
Lab, Bambu Gateway, or Bambuddy.
