---
title: CLI & LAN protocols
description: Worker commands and the virtual printer's native interfaces.
---
## Command line

```sh
bambu-cloud-lan-proxy --help
bambu-cloud-lan-proxy --version
```

Configuration is through environment variables; subcommands do not accept a config-file flag. Running the binary with no subcommand starts `service`.

| Command | Purpose |
| --- | --- |
| `service` | Cloud session, plugin adapter, admin HTTP API, MQTT bridge |
| `endpoint-manager` | Reconcile and supervise per-printer worker trees |
| `endpoint-address-manager` | Continuously reconcile virtual IPv4 aliases |
| `endpoint-addresses` | One-shot address reconciliation |
| `endpoint-addresses-check` | Check expected addresses |
| `endpoint-health` | Check manager status snapshot |
| `hash-admin-password` | Read password from stdin; output an Argon2id PHC hash |
| `endpoint` | One printer's protocol worker tree (normally managed) |
| `endpoint-optional` | Retry/supervise optional camera or status worker |
| `ftps`, `native-files`, `camera`, `discovery`, `local-status`, `tcp-relay` | Individual protocol workers |
| `native-probe` | Native file-service diagnostic client |

Normal users should run the coordinated deployment, not start all subcommands independently with defaults.

## MQTT

TLS port `8883`, username `bblp`, shared proxy code.

- Subscribe: `device/SERIAL/report`.
- Publish requests: `device/SERIAL/request`.
- Payloads use Bambu-style JSON; command policy is enforced by the proxy.
- Printer serials are real account serials, not virtual-IP identifiers.

This documentation does not promise a stable command schema for every firmware command. There is no generic REST print endpoint; use the compatible LAN client/protocol path.

## Files and discovery

Implicit FTPS uses port `990` and passive `50000–50009`. Native TLS files and supported image cameras share `6000`, with protocol-specific authenticated logins. RTSPS uses `322` and `/streaming/live/1` where supported. SSDP discovery advertises virtual-printer identities on the LAN.

Use [networking](/docs/networking/) for the port/trust matrix and [features](/docs/features/) for storage distinctions.

The internal HTTP storage API is a worker interface with shared-filesystem semantics, not an internet-facing file-download service.
