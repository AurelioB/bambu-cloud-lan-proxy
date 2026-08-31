# Architecture

```text
Bambu Handy ───────────────┐
                           v
H2C <──────> Bambu Cloud <────> Bambu Gateway + compatibility hook
                                             |
                                             | raw report / policy-checked request
                                             v
                                      Mosquitto TLS :8883
                                             ^
                                             |
                                         Bambuddy
```

Gateway's official network-plugin subprocess owns cloud authentication,
subscription, command signing, and token refresh. This project patches one
stable Python boundary in the pinned Gateway image:

1. `CloudPrinterClient.handle_event` publishes the original cloud MQTT JSON to
   `device/<serial>/report` without reconstructing printer state.
2. The embedded facade subscribes to `device/+/request` on an internal broker.
3. A fail-closed policy classifies each request.
4. Read-only requests pass through unchanged. Allowlisted controls are rebuilt
   with Gateway's own command builders so they receive valid Studio sequence
   IDs before the official plugin signs and sends them.

The downstream TLS listener speaks the same username, topics, TLS, and port
that Bambuddy expects from a LAN printer. It is an application-layer facade;
it does not change the H2C's network mode or expose the physical printer's LAN
ports.

## Certificate boundaries

There are three unrelated certificate concerns:

- **Bambu cloud credentials:** kept and refreshed by Bambu Gateway's official
  plugin. This project never extracts or redistributes its signing key.
- **Bambu printer CA:** already bundled by upstream Gateway for direct local
  printer verification. It is not the certificate Bambuddy sees here.
- **Proxy TLS:** generated locally by the Compose bootstrap. The CA private key
  stays under `data/`; `scripts/export-ca.sh` exports only its public certificate.

## Current boundary

The MQTT facade provides monitoring plus optional narrow controls. It does not
yet emulate the printer's implicit-FTPS server on port 990, camera endpoint, or
file system. Consequently, Bambuddy's file-transfer diagnostic and direct
print queue remain unavailable through the facade. Bambu Handy and Gateway can
continue to submit cloud prints.
