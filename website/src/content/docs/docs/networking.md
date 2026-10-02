---
title: Multiple printers & networking
description: Static virtual addresses, standard ports, and one shared cloud account.
---
## One cloud account, several LAN endpoints

One core owns the account session and discovers its printers. Each configured printer gets its own host IP alias and endpoint process tree. Local clients see standard ports on different IPs:

| Printer | Physical IP (example) | Virtual IP (example) | Client connection |
| --- | --- | --- | --- |
| Workshop | 192.168.1.42 | 192.168.1.201 | 192.168.1.201:8883 |
| Studio | 192.168.1.43 | 192.168.1.202 | 192.168.1.202:8883 |
| Little maker | 192.168.1.44 | 192.168.1.203 | 192.168.1.203:8883 |

Reserve those virtual addresses outside your router's DHCP pool. They must be unique and unused, on a network reachable through the configured Linux interface. Never reuse a physical printer's address.

The address manager does not request DHCP leases, configure the router, or invent a free address. It reconciles only configured aliases and removes only addresses recorded in its managed state.

## Trust boundaries

All endpoints use the same proxy code. MQTT ACLs allow authenticated clients to subscribe to serial topics; a different endpoint IP is **not** per-printer authorization. Treat every client that knows the code as trusted across the workspace.

For distinct Bambu accounts or security domains, run separately configured deployments with independent data, credentials, addresses, internal broker/admin ports, and network isolation. Copying the Compose file unchanged on the same host will collide on loopback ports.

## Ports

| Destination | Protocol | Purpose |
| --- | --- | --- |
| Each virtual IP:8883 | TCP/TLS MQTT | Reports and requests |
| Each virtual IP:990 | TCP/implicit FTPS | Job staging |
| Each virtual IP:50000–50009 | TCP/passive FTPS | Data transfers |
| Each virtual IP:6000 | TCP/TLS | Native files |
| Supported virtual IP:322 | TCP/RTSPS | RTSPS camera |
| Image-capable virtual IP:6000 | TCP/TLS | Image camera and native files, multiplexed |
| LAN discovery | UDP, SSDP | Discovery announcements / searches |
| Host loopback:4844 | HTTP | Admin and internal storage API |
| Host loopback:1883 / 18883 | MQTT / TLS MQTT | Internal broker, not for untrusted clients |

Keep the anonymous internal broker loopback-only (native/Docker host examples) or confined to the private Compose network with loopback publication. Never expose it to the LAN.

## VLANs and remote clients

Broadcast discovery generally does not cross routed networks. Use manual IP configuration, explicit firewall rules, and a VPN where needed. Permit only trusted client addresses to reach virtual-printer ports. The proxy does not manage host firewalls.

Loopback virtual IPs such as `127.0.0.2` are useful for same-host tests only. A remote client cannot connect to them. The interface setting is needed for non-loopback aliases.

## Address changes

Update the endpoint through the console, refresh certificates, and restart broker/endpoints. For Compose:

```sh
./scripts/apply-endpoints.sh
```

Update client configurations and verify the old alias has been reconciled. Preserve address-manager state across upgrades so it can identify addresses it owns.
