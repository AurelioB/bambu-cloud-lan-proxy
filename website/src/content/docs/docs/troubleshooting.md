---
title: Troubleshooting
description: Trace failures from the browser and client back to the right service.
---
## I cannot open the console

The default admin listener is published only on host loopback. Use the browser on that host or an [SSH tunnel](/docs/first-printer/). `127.0.0.1` on your phone/laptop is not the server.

Check the core logs and `GET /api/health`. Plugin download or initialization failure can prevent startup.

## Sign-in fails

- Admin login and Bambu cloud login are different flows.
- An OIDC/local installation does not accept the printer code as an admin bypass.
- Check exact `ADMIN_PUBLIC_URL`, including port, and reverse-proxy Origin/Cookie handling.
- Confirm the exact issuer and subject allowlist. Email is not the OIDC subject.
- A session expires after its absolute deadline and on core restart.
- The global sign-in limiter allows ten attempts/starts per minute; pause after repeated failures.
- Do not share callback URLs, cookie values, codes, or raw provider tokens.

See [authentication](/docs/authentication/) for provider requirements.

## No printers appear

Check the account region and complete the official Bambu login. Confirm the printer belongs to that account and Handy can see it. Review any `PROXY_PRINTERS` allowlist.

“Cloud connected” and “signed in” are separate signals. Inspect the authenticated overview, not just HTTP liveness.

## Endpoint unavailable or address already in use

Verify the saved virtual IP is unique, outside DHCP, and on the correct interface. Confirm the address manager is healthy and the host supports the required network capabilities.

```sh
ip -brief address
docker compose logs --tail=100 endpoint-addresses endpoint-manager
```

Save the correct endpoint and run `./scripts/apply-endpoints.sh`. Never solve a conflict by taking an address away from another device.

## MQTT works, but files do not

Allow FTPS control **and passive ports** through the client-to-proxy firewall. FTP without TLS, explicit FTP STARTTLS, and SFTP are not substitutes for implicit FTPS.

Port 990 is staging; printer-resident files use the native port-6000 service. Check the core's internal storage URL, service key, shared staging/cache mounts, physical credentials, and cloud media tunnel connectivity.

Internal model-history deletion can be unsupported by firmware. Remove such entries through the printer UI; do not assume an acknowledgement means deletion happened.

## Reports are available, but writes fail

Check all permissions, secure-control readiness, account/cloud connectivity, and the exact operation supported by firmware. Printing and reprint are independent opt-ins, not implied by full control.

If a write timed out, **check the printer before retrying**. Unknown outcomes are not automatically replayed.

## Camera unavailable

Check the model's capability profile, local liveview setting, physical IP/code, and reachability of physical port 322 (RTSPS) or 6000 (A1/P1-family image cameras). Image clients use the virtual port 6000 and the proxy code; file clients share that port using a distinct login protocol. Camera pins are `SERIAL.sha256` for RTSPS and `SERIAL-image.sha256` for image streams. Cloud access cannot substitute for an unavailable local camera. A healthy listener is not proof of upstream frames.

If a certificate pin changed, verify it through an independent trusted path first. Reset only the affected verified pin, never all security state as a troubleshooting shortcut.

## TLS hostname / IP mismatch

Trust the public proxy CA in the client, then confirm the server certificate contains the virtual IP. Re-run certificate bootstrap after address changes and restart broker/endpoints. Do not replace CA material unnecessarily; that invalidates existing client trust.

## Collect a useful issue report

Include the source revision, installation method, host architecture, model/firmware, client version, affected protocol, expected result, and redacted logs. Remove serials, private IPs if sensitive, access codes, account identifiers, callbacks, cookies, and media URLs.

Use the repository's private security reporting route when available for vulnerabilities, not a public issue with working credentials.
