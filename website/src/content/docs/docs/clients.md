---
title: Bambuddy & LAN clients
description: Point compatible clients at your virtual printers.
---
## Bambuddy

Create one printer entry per virtual IP after completing [first-printer setup](/docs/first-printer/).

| Client field | Value |
| --- | --- |
| Host / IP | That printer's virtual IPv4 address |
| Serial number | That printer's real serial |
| Access code | Shared `PROXY_ACCESS_CODE` |
| MQTT username, if configurable | `bblp` |
| MQTT port | `8883` |

The shared cloud account does not merge printers: each endpoint injects its own serial for file operations and publishes the appropriate discovery identity. MQTT uses serial-specific topics.

If Bambuddy runs in another Docker bridge network, `127.0.0.2` refers to **that container**, not the proxy host. Use reachable LAN virtual addresses, or deliberately configure host networking on Linux.

## Bambu Studio

Keep your normal Bambu account login for cloud access. For workflows using the virtual LAN printer, use the configured virtual IP, real serial, and proxy code. Native file browsing is exposed on port 6000.

Client versions and firmware may differ in discovery and certificate handling; manual address configuration is more predictable than relying only on broadcast discovery. The supervised release validation confirmed Studio remained usable alongside the proxy, not that every Studio feature has been emulated.

## Certificates

Export the public CA:

```sh
./scripts/export-ca.sh
```

Trust `data/proxy/export/bambu-cloud-lan-proxy-ca.crt` in the appropriate client trust configuration. Never share `ca.key`, `server.key`, plugin state, `.env`, or the physical-printer code.

If an IP changes, refresh the server certificate and restart listeners. Installing a CA does not fix a certificate whose SAN lacks the address you use.

## Read-only MQTT check

With Mosquitto clients installed, replace the address, serial, and code locally:

```sh
mosquitto_sub -h 192.168.1.201 -p 8883 \
  --cafile data/proxy/export/bambu-cloud-lan-proxy-ca.crt \
  -u bblp -P 'YOUR_PROXY_CODE' \
  -t 'device/YOUR_SERIAL/report' -C 1 -W 30
```

The literal credential shown is a placeholder. Command-line passwords can be visible to local process inspection; on shared hosts use your client's protected credential configuration. Do not disable certificate verification as a permanent fix.

Do not publish arbitrary MQTT commands just to test connectivity: some commands move hardware or start/cancel jobs.
