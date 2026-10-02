---
title: Configuration reference
description: Compose settings, native equivalents, and persistence.
---
Compose reads `.env` and maps user-facing names to the Rust process environment. A native process or `docker run --env-file` **does not perform that mapping**, and does not automatically read `.env`. Use the supplied deployment examples.

## Main settings

| Compose .env | Rust process variable | Default / role |
| --- | --- | --- |
| `PROXY_ACCESS_CODE` | `CLOUD_LAN_PROXY_ACCESS_CODE` (core), `PROXY_ACCESS_CODE` (endpoints) | Required, eight ASCII letters/digits |
| `PROXY_MODE` | `CLOUD_LAN_PROXY_MODE` | `read_only` |
| `PROXY_ALLOW_CANCEL` | `CLOUD_LAN_PROXY_ALLOW_CANCEL` | `false` |
| `PROXY_CLOUD_PRINT_ENABLED` | `CLOUD_LAN_PROXY_CLOUD_PRINT_ENABLED` | `false` |
| `PROXY_STORAGE_ALLOW_REPRINT` | `CLOUD_LAN_PROXY_REMOTE_REPRINT_ENABLED` | `false` |
| `PROXY_STORAGE_ALLOW_UPLOAD` | `CLOUD_LAN_PROXY_STORAGE_ALLOW_UPLOAD` | `false` |
| `PROXY_STORAGE_ALLOW_DELETE` | `CLOUD_LAN_PROXY_STORAGE_ALLOW_DELETE` | `false` |
| `PROXY_ADVERTISE_CONTROLS` | `CLOUD_LAN_PROXY_ADVERTISE_CONTROLS` | `auto` derives from mode/printing |
| `PROXY_PRINTERS` | `CLOUD_LAN_PROXY_PRINTERS` | Empty = all account printers; otherwise comma-separated serials |
| `BAMBU_CLOUD_REGION` | Same | `US`; use the region appropriate to the account |
| `PROXY_ENDPOINT_INTERFACE` | `ENDPOINT_INTERFACE` | Required for LAN aliases |
| `PROXY_DISCOVERY_TARGET` | `DISCOVERY_TARGET` | `255.255.255.255` |
| `PROXY_LOCAL_STATUS_ENABLED` | `ENDPOINT_LOCAL_STATUS` | `true`; needs local credentials/services |

## Limits

| Compose setting | Default | Purpose |
| --- | --- | --- |
| `PROXY_FTPS_MAX_UPLOAD_MB` | 1024 | Individual staged upload/cloud job |
| `PROXY_FTPS_MAX_TOTAL_MB` | 10240 | Staging quota (each endpoint has its own staging root) |
| `PROXY_STORAGE_MAX_DOWNLOAD_MB` | 4096 | Native transfer limit |
| `PROXY_STORAGE_SUBFILE_MAX_MB` | 256 | Native subfile limit |
| `PROXY_CLOUD_PRINT_WAIT_SECONDS` | 900 | Cloud print wait |
| `PROXY_CLOUD_PRINT_PROGRESS_TIMEOUT` | 900 | Cloud print progress timeout |

The implementation multiplies MB values by 1024×1024. Budget disk capacity for multiple printers and simultaneous staging/cache data.

## Admin and service paths

- `ADMIN_*`: same names in Compose and native; see [authentication](/docs/authentication/).
- `GATEWAY_BIND_IP` / `GATEWAY_PORT`: Compose host publication, default `127.0.0.1:4844`.
- `PROXY_HTTP_BIND`: Rust HTTP listener; native example explicitly restricts it to loopback.
- `PROXY_DATA_DIR`: core state path, default `/data`.
- `BAMBU_PLUGIN_HOST`: native adapter executable, default `/usr/local/bin/bambu-proxy-native`.
- `PROXY_CA_EXPORT_FILE`: public CA download path.
- `CLOUD_LAN_PROXY_BROKER_HOST` / `_PORT`: internal MQTT connection; defaults `mqtt:1883`.
- `ENDPOINT_REGISTRY`: shared printer registry; contains private local credentials.
- `ENDPOINT_CERT_FILE` / `ENDPOINT_KEY_FILE`: certificate for endpoint services.
- `ENDPOINT_STATUS_FILE` / `CLOUD_LAN_PROXY_ENDPOINT_STATUS_FILE`: manager writer/core reader of the same health file.
- `CAMERA_SERVER_CERT` / `CAMERA_SERVER_KEY`: camera TLS paths when not using container defaults.
- `ENDPOINT_ADDRESS_STATE`: owned-alias tracking file; preserve it.
- `RUST_LOG` / `LOG_LEVEL`: logging filter. Avoid verbose logs when sharing diagnostics.

## Persistence and precedence

Default Compose paths:

| Directory | Contents |
| --- | --- |
| `data/gateway` | Account/plugin state, device identity, printer registry, saved admin settings, submissions |
| `data/proxy` | Local CA/private key, server certificates, hashed broker credentials, public export |
| `data/staging` | Per-printer job staging |
| `data/remote-cache` | Temporary native file transfers |
| `data/camera` / `data/local-status` | Upstream certificate pins and local status state |
| `data/endpoint-state` | Runtime endpoint health |
| `data/address-state` | Managed IP ownership |

Use the UI for printer and policy changes. Do not hand-edit the registry while running. Startup environment changes require service recreation/restart; `docker compose restart` alone does not apply changed Compose environment values.
