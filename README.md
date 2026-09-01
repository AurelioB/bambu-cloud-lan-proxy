# Bambu Cloud LAN Proxy

A private-by-default bridge that makes a printer connected through **Bambu
Cloud** look like a local Bambu MQTT endpoint. Its first target is monitoring a
Bambu Lab H2C from [Bambuddy](https://github.com/maziggy/bambuddy) without
turning off Bambu Handy.

```text
H2C ↔ Bambu Cloud ↔ Bambu Gateway ↔ local MQTT/TLS + file facade ↔ clients
 ├──── read-only LAN MQTT (preferred while reachable) ─────────────┤
 └──── LAN-only RTSPS camera ───────── local RTSPS :322 ──────────┘
```

The H2C stays in normal cloud mode. [Bambu
Gateway](https://github.com/leolobato/bambu-gateway) uses Bambu's official
network plugin for cloud authentication and signed traffic. This project adds
a small compatibility hook and a hardened Mosquitto listener; it does not
reverse proxy Bambu's public broker or place account credentials in Bambuddy.

## What works

- Local-first, certificate-pinned read-only H2C telemetry with automatic cloud
  fallback (temperatures, progress, AMS, errors, versions, and task state)
- Bambuddy's `pushall`, `get_version`, `get_accessories`, and
  `extrusion_cali_get` queries
- Optional signed pause, resume, speed, and light controls
- Optional signed passthrough for the complete MQTT request envelope used by
  Bambuddy (`full_control` mode)
- Implicit FTPS on port 990 with protected passive data channels
- Staged file listing, upload, size, download, rename, and delete operations
- Read-through browsing and downloads of the printer's previous jobs,
  timelapses, and recordings through the official cloud media tunnel
- Studio-compatible TLS file service on port 6000: media ability discovery,
  lists, downloads, 3MF metadata/thumbnails, cancellation, guarded upload and
  delete, and change notification
- Optional physical-storage upload/delete and existing-file reprint, each
  protected by its own configuration gate
- Bambu SSDP-shaped discovery and per-serial FTPS namespaces
- Bambuddy archive reads for both staged files and files already on the printer
- Authenticated RTSPS camera relay on port 322 when the printer's **LAN-only
  liveview** endpoint is enabled and reachable
- Optional `project_file` handoff to Gateway's signed cloud-print pipeline,
  including plate, AMS, H2C nozzle mapping, and print-option translation
- Bambu Handy at the same time, because the printer remains cloud-connected
- Automatic local TLS CA/server certificate and MQTT credential creation
- A reproducible Docker Compose deployment pinned to Bambu Gateway 3.0.0

The default is deliberately safe: status is read-only and direct cloud-print
dispatch is disabled. FTPS and staged-file/archive access are available without
enabling printer writes. Cloud printing through Handy or Gateway remains
available independently.

The camera path is deliberately local: it pulls the printer's RTSPS liveview,
pins the physical camera certificate, and re-publishes one authenticated stream
to local clients. It does not proxy Bambu's cloud-video service. Consequently,
camera access requires the printer to remain reachable on LAN port 322 and the
**LAN-only liveview** toggle to stay enabled. The proprietary cloud camera path
is not re-published by this project.

## Quick start

Requirements: Docker Engine with Compose v2 and a Linux host. Clone the repo,
then run:

```sh
./scripts/init.sh
./scripts/deploy.sh
```

The first command creates `.env`, an eight-character local access code, a local
certificate authority, and the staged filesystem. The second builds and starts
Gateway, the TLS MQTT broker, implicit FTPS, and the RTSPS camera relay.

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
issue a certificate for that address, and redeploy. Allow TCP 322, 6000, 8883,
990, and 50000-50009 only from trusted clients.

After deployment, Bambuddy's MQTT and FTPS diagnostics should pass. Its camera
diagnostic passes whenever the printer's local liveview endpoint is reachable.
The downstream camera username is `bblp` and it uses the same proxy access code.

Despite Bambuddy's historical wording, the file service is implicit **FTPS**,
not SSH SFTP. Local files and uploads live in `data/staging/`. The standard
`cache/`, `timelapse/`, and `video/` directories also merge in read-only files
reported by the printer through Studio's official cloud media tunnel. A remote
file is downloaded only when a client retrieves it, validated by size and the
printer-provided MD5, then removed from the transient cache. Local files win a
same-name collision. The default keeps printer storage read-only. Enable
physical mutations only for trusted local clients:

```dotenv
PROXY_STORAGE_ALLOW_UPLOAD=true
PROXY_STORAGE_ALLOW_DELETE=true
PROXY_STORAGE_ALLOW_REPRINT=true
```

With upload enabled, `STOR /cache/<name>` and native port-6000 uploads mirror
the validated file into printer storage. Delete can remove a remote-only entry.
Reprint accepts a printer-resident `file:///` job and additionally requires
`PROXY_MODE=full_control`.

Enable this for one printer with:

```dotenv
PROXY_STORAGE_PRINTER_SERIAL=YOUR_SERIAL
```

The cloud file path does not require Developer Mode or physical LAN-mode ports.
It does require the printer and Gateway cloud session to be online.

For multiple allowed printers, FTPS adds
`/printers/<serial>/{cache,timelapse,video}`. The primary printer also retains
the standard paths Bambuddy expects.

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
| `full_control` | Signed passthrough for structurally valid MQTT commands; intended for a trusted Bambuddy instance |
| `unsafe` | Legacy development mode for unknown envelopes; prefer `full_control` |

Cancel is destructive enough to require a second switch:

```dotenv
PROXY_MODE=controls
PROXY_ALLOW_CANCEL=true
```

For the closest MQTT approximation to LAN mode, while the printer is idle:

```dotenv
PROXY_MODE=full_control
PROXY_ALLOW_CANCEL=true
PROXY_CLOUD_PRINT_ENABLED=true
```

`full_control` sends Bambuddy's request envelope through Gateway's official
network plugin, which performs cloud authentication and signing. It permits
real motion, heating, G-code, calibration, and other commands supported by the
cloud service. It cannot make a cloud-rejected or LAN-only firmware operation
work, so this remains a LAN-shaped hybrid rather than literal firmware LAN mode.

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
docker compose logs --tail=200 gateway mqtt ftps camera camera-ingress
./scripts/verify-mqtt.sh YOUR_PRINTER_SERIAL
./scripts/verify-ftps.sh
./scripts/verify-ftps.sh --remote
./scripts/verify-camera.sh
./scripts/verify-native.sh
```

The MQTT verification waits for one real cloud report and sends no printer
command. The normal FTPS verification round-trips one tiny synthetic ZIP/3MF
through staging and deletes it. `--remote` additionally lists the printer's
timelapses and downloads the smallest one without changing printer storage.

- **Bambuddy connects but shows no data:** confirm its serial matches Gateway
  exactly; MQTT topics are case-sensitive. Wait up to 30 seconds for Gateway's
  periodic full snapshot.
- **Authentication refused:** copy the eight-character code from `.env` again,
  then rerun `./scripts/init.sh` and restart `mqtt` if it changed.
- **FTPS/port 990 fails:** confirm `ftps` is healthy and that passive ports
  50000-50009 are reachable from Bambuddy as well as port 990.
- **Printer files are missing:** set `PROXY_STORAGE_PRINTER_SERIAL`, confirm it
  is allowed by `PROXY_PRINTERS`, and verify that both the printer and Gateway's
  cloud account are online. Listings are cached for 30 seconds by default.
- **Queue upload succeeds but no print starts:** direct dispatch is off by
  default. Check `PROXY_CLOUD_PRINT_ENABLED`, then inspect Gateway logs. A busy
  printer is intentionally refused rather than risking its active job.
- **Camera check fails:** confirm LAN-only liveview is enabled and
  `CAMERA_UPSTREAM_IP` is current, then test that the Docker host can reach that
  IP on TCP 322. Status may still work through cloud while this local path is
  down. If port 322 is unavailable, use Bambuddy's external-camera fallback.
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
need: a private local CA and one server certificate shared by MQTT, FTPS, and
RTSPS.
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
