# Bambu Cloud LAN Proxy

A private-by-default bridge that makes a printer connected through **Bambu
Cloud** look like a local Bambu MQTT endpoint. Its first target is monitoring a
Bambu Lab H2C from [Bambuddy](https://github.com/maziggy/bambuddy) without
turning off Bambu Handy.

```text
H2C ↔ Bambu Cloud ↔ Bambu Gateway ↔ local MQTT/TLS + FTPS facade ↔ Bambuddy
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
- Implicit FTPS on port 990 with protected passive data channels
- Staged file listing, upload, size, download, rename, and delete operations
- Bambuddy archive reads for files that were uploaded through this proxy
- Optional `project_file` handoff to Gateway's signed cloud-print pipeline,
  including plate, AMS, H2C nozzle mapping, and print-option translation
- Bambu Handy at the same time, because the printer remains cloud-connected
- Automatic local TLS CA/server certificate and MQTT credential creation
- A reproducible Docker Compose deployment pinned to Bambu Gateway 3.0.0

The default is deliberately safe: status is read-only and direct cloud-print
dispatch is disabled. FTPS and staged-file/archive access are available without
enabling printer writes. Cloud printing through Handy or Gateway remains
available independently.

The physical H2C's cloud camera is not exposed. Bambu's cloud live view uses a
proprietary TUTK/Agora transport that neither Gateway's plugin host nor
open-bamboo-networking exposes as a reusable video stream. A fake port 322 would
make diagnostics green without producing video, so this project leaves it
closed. Configure an external camera directly in Bambuddy if one is available.

## Quick start

Requirements: Docker Engine with Compose v2 and a Linux host. Clone the repo,
then run:

```sh
./scripts/init.sh
./scripts/deploy.sh
```

The first command creates `.env`, an eight-character local access code, a local
certificate authority, and the staged filesystem. The second builds and starts
Gateway, the TLS MQTT broker, and the implicit-FTPS service.

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
issue a certificate for that address, and redeploy. Allow TCP 8883, 990, and
50000-50009 only from the Bambuddy host.

After deployment, Bambuddy's MQTT and FTPS diagnostics should pass. Its camera
diagnostic remains a warning for the cloud-only H2C, as described above.

## Direct Bambuddy queue dispatch

FTPS staging is always on, but turning a staged file into a physical print is a
separate explicit opt-in. First wait until the printer is idle, then set:

```dotenv
PROXY_CLOUD_PRINT_ENABLED=true
```

Apply it with:

```sh
docker compose up -d --build gateway
```

The facade acknowledges Bambuddy's `project_file`, waits for either observed
slicer ordering (FTP-first or MQTT-first), snapshots the completed 3MF, and
hands it to Gateway's official cloud plugin. It refuses the job when the
printer is offline, busy, the file is missing/corrupt/too large, another cloud
submission is active, or the exact MQTT request is replayed. The physical
printer never downloads `ftp://127.0.0.2/...`; Gateway uploads the snapshot to
Bambu's cloud-print path and signs the dispatch.

`PROXY_MODE=read_only` may remain in place: the cloud-print switch is
independent. Set `PROXY_MODE=controls` only if Bambuddy also needs its optional
pause/resume/speed/light and pre-print control paths. Cancel still needs
`PROXY_ALLOW_CANCEL=true`.

Files uploaded through the facade remain under `data/staging/`, so Bambuddy can
download them for covers and archives. A print started in Handy or directly in
Gateway has no corresponding staged file; the current official plugin exposes
cloud upload/dispatch, but no API to pull the source 3MF back into this
filesystem. Those jobs still monitor normally, but automatic 3MF archival can
only succeed when Bambuddy already has a local copy or the job traversed this
proxy.

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

When controls or cloud printing are active, the facade changes only the
downstream `fun` capability bit so Bambuddy knows the **facade** accepts writes.
It does not alter what the cloud printer reports upstream or change the
physical printer's LAN/Developer Mode setting.

Restrict a multi-printer cloud account with a comma-separated allowlist:

```dotenv
PROXY_PRINTERS=SERIAL_ONE,SERIAL_TWO
```

## Verify and troubleshoot

```sh
docker compose ps
docker compose logs --tail=200 gateway mqtt ftps
./scripts/verify-mqtt.sh YOUR_PRINTER_SERIAL
./scripts/verify-ftps.sh
```

The MQTT verification waits for one real cloud report and sends no printer
command. The FTPS verification round-trips one tiny synthetic ZIP/3MF through
staging and deletes it; it never contacts the physical printer.

- **Bambuddy connects but shows no data:** confirm its serial matches Gateway
  exactly; MQTT topics are case-sensitive. Wait up to 30 seconds for Gateway's
  periodic full snapshot.
- **Authentication refused:** copy the eight-character code from `.env` again,
  then rerun `./scripts/init.sh` and restart `mqtt` if it changed.
- **FTPS/port 990 fails:** confirm `ftps` is healthy and that passive ports
  50000-50009 are reachable from Bambuddy as well as port 990.
- **Queue upload succeeds but no print starts:** direct dispatch is off by
  default. Check `PROXY_CLOUD_PRINT_ENABLED`, then inspect Gateway logs. A busy
  printer is intentionally refused rather than risking its active job.
- **Camera check warns:** expected for a cloud-only H2C. Use Bambuddy's external
  camera setting; the Bambu cloud stream is not available to this bridge.
- **Gateway has no printers:** complete its Bambu cloud sign-in and confirm the
  correct `BAMBU_CLOUD_REGION` (`US` or `CN`).

## Why no Bambu certificate extraction?

[open-bamboo-networking](https://github.com/ClusterM/open-bamboo-networking)
can fetch slicer-side credentials because it replaces the slicer's network
plugin. This proxy has a different boundary: upstream Gateway already loads
the official plugin and owns its cloud credentials. Extracting or shipping
Bambu's shared private signing material would add both security and maintenance
risk without helping Bambuddy.

The Compose bootstrap instead creates the only certificate these new endpoints
need: a private local CA and one server certificate shared by MQTT and FTPS.
Runtime containers receive only a copy of the server material; the CA signing
key is not mounted into them. See [the architecture notes](docs/architecture.md)
for the separation.

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
