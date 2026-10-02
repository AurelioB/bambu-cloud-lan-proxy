---
title: Connect your first printer
description: From a running proxy to a working virtual printer in Bambuddy.
---
## 1. Open the admin console

With the default installation, visit `http://127.0.0.1:4844` **on the proxy host**. For a remote server, tunnel it from your workstation:

```sh
ssh -N -L 4844:127.0.0.1:4844 your-user@your-proxy-host
```

Then open the same loopback URL on your workstation. Sign in with `PROXY_ACCESS_CODE`, or your configured local/OIDC admin identity. Do not expose the HTTP port publicly to make it reachable.

## 2. Connect the Bambu account

Open **Bambu account**. Follow the official sign-in link, then paste the resulting callback URL into the console as directed. Treat the entire callback URL as a secret; never put it into an issue or screenshot.

Choose the account's region in host configuration before starting the core. Printers in the account appear automatically. A shared core uses one Bambu account, not one separate account login per endpoint.

## 3. Configure a virtual endpoint

Under **Printers → Configure** (or **Connections**), enter:

| Field | Example | Meaning |
| --- | --- | --- |
| Virtual IP | `192.168.1.201` | An unused address reserved for this proxy endpoint |
| Physical IP | `192.168.1.42` | The real printer; optional for cloud-only use |
| Physical access code | Your printer's eight-character code | Enables attempts at local services; never use the proxy code here |
| Enabled | On | Start that printer's virtual endpoint |

These addresses are examples. Use your own subnet and verify the virtual IP is unassigned. Reserve a different address for each printer.

## 4. Apply addresses and TLS certificates

For Compose, on the proxy host:

```sh
./scripts/apply-endpoints.sh
```

Saving in the UI updates the registry, but certificates also need the new virtual IP in their Subject Alternative Names. This command refreshes the certificate and restarts the affected listeners. It can briefly interrupt connected clients.

Docker and standalone users must repeat their certificate bootstrap and restart their broker/endpoints as described in their installation guide.

## 5. Add the proxy to your client

In Bambuddy, add a printer manually using:

- **Address:** the virtual IP, not the real printer or the admin port.
- **Serial:** the real serial shown in the console.
- **Username:** `bblp`, where requested.
- **Access code:** `PROXY_ACCESS_CODE`, not the physical-printer code or admin password.

Import the public proxy CA if your client validates the TLS certificate. In Compose, use `./scripts/export-ca.sh` or download it from the authenticated console.

## 6. Verify before enabling writes

Confirm endpoint health, current reports in your client, and continued access through Handy. Camera and files are separate checks. Keep every write permission off until read-only access works.

Then review [permissions](/docs/permissions/) and enable only what you need. Do not use an unattended print or cancellation as an installation smoke test.
