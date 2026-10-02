---
title: Features & data paths
description: Understand which traffic stays local and which uses Bambu Cloud.
---
## The connection model

```text
Bambuddy / Studio ── virtual IP per printer ── proxy
                                               ├─ Bambu Cloud → signed controls / printing
                                               ├─ printer LAN → optional local reports
                                               ├─ printer LAN → RTSPS or image camera
                                               └─ LAN first, cloud fallback → printer files

Bambu Handy ──────────────────────────────── Bambu Cloud ── printer
```

The facade makes local clients see familiar protocols; it does not place the real printer in LAN-only mode.

### Handy does not pass through the proxy

Handy continues to use its normal Bambu Cloud connection and the printer's existing cloud pairing. It does not connect to the virtual IP, use the proxy's access code, or need the proxy to forward its traffic.

```text
Phone: Bambu Handy ←──────→ Bambu Cloud ←──────→ physical printer
                                  ↑                    ↑
                                  │ cloud routes       │ local routes
Local app ←──────→ virtual LAN proxy ────────────────────┘
```

Stopping the proxy disconnects clients using its virtual endpoints, but leaves Handy's direct cloud path unchanged. This does not make Handy immune to internet, Bambu Cloud, account, or printer outages. Both paths control the **same physical printer**, so a print or pause from either client changes the state seen by the other.

The public [interactive diagrams](/#handy-connection) let you simulate a proxy outage and explore the routes without making any printer connections.

### Routing depends on the operation

“Virtual LAN” describes the interface your local client sees, not a promise that every request travels over LAN. The proxy authenticates clients and applies the relevant permissions before forwarding allowed operations. Missing local access does not bypass these checks.

| Feature | Preferred path | Fallback / dependency |
| --- | --- | --- |
| MQTT status and reports | Fresh local read-only reports, if available | Bambu Cloud reports |
| Explicit read-only MQTT queries | Cloud plugin send path | Allowlisted queries; not the signed-control path or a local MQTT request |
| Controls (pause, resume, speed, light, cancel, etc.) | Bambu Cloud through official signing plugin | Policy-gated; no offline-write fallback |
| Start a staged job | Cloud upload and dispatch through official plugin | Requires cloud printing permission |
| Reprint a printer-resident job | Cloud-signed command referencing the existing file | Requires full control or unsafe mode plus remote reprint permission |
| Browse / download printer files and timelapses | Local native file service with physical credentials | Official cloud media tunnel if a local connection cannot be established |
| Upload / delete printer-resident files | Local native file service, after storage permission checks | Cloud media connection fallback; firmware may reject an operation |
| Live camera | Local RTSPS or chamber-image protocol | No cloud fallback; model-specific protocol |
| Upload to staging | Client → proxy FTPS | Does not by itself print or upload to printer storage |
| Admin UI | Browser → proxy HTTP/HTTPS | Local account, OIDC, or access-code sign-in |

## Status and telemetry

Clients subscribe to `device/SERIAL/report` over TLS MQTT. Reports can include task state, temperatures, filament/AMS information, progress, and other fields supplied by firmware. The proxy forwards available reports; it does not guarantee every field for every model.

Fresh local reports take precedence when configured and available. Stale or unavailable local reports yield to cloud reports. The admin console shows connection and capability information; it is not a complete print-monitoring dashboard.

The proxy also learns sanitized capabilities from those reports: attached AMS units, observed filament-slot IDs, firmware version and reported function flags, temperature/fan/door/Wi-Fi sensors, and observed dual extruders. Partial updates preserve missing fields; explicit AMS existence changes remove detached units. Slot counts stay unknown when unit details are incomplete. Firmware-reported functions do not grant proxy permissions.

Fresh local capability evidence takes priority within the configured local-report window (20 seconds by default). Fresh cloud evidence fills missing fields within a 90-second window, then model profiles fill remaining gaps. The API exposes `source`, `model_known`, `observed_fields`, and `runtime` to distinguish evidence from fallback. Unobserved camera/toolhead capabilities on unknown models remain unknown.

This incoming subscription is distinct from an explicit request such as `get_version`, `get_accessories`, `pushall`, or `extrusion_cali_get`. Those allowlisted queries are sent through the cloud plugin's ordinary message path, not its signed-control path; their reports enter the normal telemetry flow.

## Controls and printing

Commands arrive on `device/SERIAL/request`, pass policy checks, and use the official plugin's signing path. Print staging, cloud dispatch, printer-resident reprint, and cancellation have distinct controls.

A cloud write timeout means **unknown outcome**, not “nothing happened.” The proxy does not automatically replay it. Check the printer and fresh status before trying again.

See [permissions and printing](/docs/permissions/).

## Files and timelapses

There are two different file surfaces:

- **FTPS, port 990:** proxy-local staging, including uploads for later cloud job dispatch. This is implicit TLS FTP, **not SFTP/SSH**.
- **Native TLS file protocol, port 6000:** printer-resident job/history, timelapse, and video access through compatible clients. Supports capability queries, listing, downloads/subfiles, and separately permitted uploads/deletes.

Native downloads use a shared temporary cache and remove transfer files afterward. Uploads must originate inside staging. Firmware may reject particular storage operations, especially internal model-history deletion; the proxy does not claim success if a verified deletion failed.

Local-first selection happens when opening a native file connection: try the local media service, then the cloud media tunnel if that connection cannot be established. It is **not** a general retry of uploads or deletes after an operation fails or returns an uncertain result.

An FTPS upload is not the same as a printer-storage upload. Neither alone is permission to start a print.

## Camera

For supported profiles, the proxy relays the printer's local RTSPS stream through MediaMTX at:

```text
rtsps://VIRTUAL_IP:322/streaming/live/1
```

Use username `bblp` and the proxy code in your client's authentication fields. Avoid embedding credentials in shareable URLs.

For A1 / A1 mini / A2L / P1P / P1S, configure your compatible client's image camera with the virtual IP and **proxy** access code. The virtual TLS port `6000` accepts both image-camera and native-file logins, dispatching each by its protocol header. The proxy uses the physical printer's access code upstream; multiple viewers share one on-demand connection. This is the Bambu chamber-image protocol, not an HTTP/MJPEG endpoint or an RTSPS conversion.

Enable local liveview and configure the real IP/code. Trust-on-first-use certificate pins detect later certificate changes. Image-camera pins use `SERIAL-image.sha256` in the camera state directory; RTSPS uses `SERIAL.sha256`. Verify a changed fingerprint independently before resetting a pin. Relay health checks show listener availability, not proof of a received video frame. Automated tests cover the image protocol over TLS; physical A1/P1-family validation is still needed.

## Admin workspace

The protected console offers printer cards/list, search and filters, endpoint health, per-model capabilities, connection editing, cloud sign-in, CA export, and saved permissions. It supports light, dark, and system appearance.

Admin authentication applies to the UI and HTTP APIs, **not** the virtual-printer protocols. Signing out of the admin console does not sign out the Bambu account or stop the printers.
