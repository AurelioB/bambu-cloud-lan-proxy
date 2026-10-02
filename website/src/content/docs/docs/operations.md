---
title: Updates, backups & security
description: Maintain the proxy without losing certificates, credentials, or endpoint state.
---
## Update carefully

Record the current revision, back up, then review and install a chosen revision:

```sh
git rev-parse HEAD
git status --short
docker compose stop
umask 077
tar -czf ../bambu-proxy-backup.tar.gz .env data
# Fetch and review upstream changes before checking out the chosen revision.
git fetch --tags origin
# After selecting a reviewed revision:
docker compose up -d --build
docker compose ps
```

The example archive name must not overwrite your only backup: choose a new dated filename for each backup. Archive any secret files and private overrides separately as well. Use sufficient privilege to read root-owned CA material; never loosen its permissions for backup.

Do not switch revisions over uncommitted work. `git fetch` alone does not update your checkout. Do not blindly reset or discard local changes.

The official plugin is pinned by version and hashes in source. A changed upstream download fails closed until maintainers review and update those pins. Do not bypass verification to get past a startup error.

## Restore / rollback

Stop the services. Preserve the failed state in a separate backup, then restore your chosen configuration/state snapshot with original ownership and permissions. Run the matching reviewed source/image revision and check health, account connection, virtual IPs, certificates, and a read-only client.

Do not merge two live plugin-state directories or run two cores against one writable data directory. Restore the public CA **and its existing private key** if you want clients to keep trusting the same CA.

## Routine diagnostics

```sh
docker compose ps
docker compose logs --tail=200 gateway mqtt endpoint-manager endpoint-addresses
curl --fail http://127.0.0.1:4844/api/health
ip -brief address
```

A successful health response only proves the HTTP process responds. The authenticated overview, endpoint health, and a current client report establish more of the path.

Verification scripts in `scripts/` help with individual protocols. Read their required environment/configuration before running them against a real printer.

## Security checklist

- Never expose printer ports or the anonymous internal broker to the internet.
- Use a trusted LAN/VPN and firewall allowlists. Proxy codes are compatibility credentials, not strong per-user authorization.
- Prefer local admin login or OIDC if printer clients must not also have admin access.
- Put remote admin access behind trusted HTTPS or use an SSH tunnel.
- Keep `.env`, account state, callback URLs, private keys, and backups private.
- Trust only the public CA export on clients. Do not distribute the plugin device certificate/key.
- Do not put live printer data in documentation, demo fixtures, or issues.
- Check new upstream TLS fingerprints independently before clearing camera/status pins.
- Treat printer control as physical hardware access. Do not run unattended write tests.

Admin sessions are in memory and expire on restart. Removing an OIDC identity at the provider does not instantly revoke an existing app session; restart the core to revoke all sessions, or wait for their deadline.

## Uninstall

Stop services first. The address manager persists owned aliases and does not promise to remove all of them on an abrupt stop. Inspect its state and the host interface before removing **only addresses belonging to this installation**. Do not flush the host interface.

Keep configuration/data backups until you are certain you no longer need the account state, CA, or staged files. The documentation deliberately does not provide a broad recursive deletion command.
