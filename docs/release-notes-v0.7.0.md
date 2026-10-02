# v0.7.0 — Community release candidate

Keep Bambu Handy connected to Bambu Cloud while local apps use independent
virtual LAN endpoints. This release adds a redesigned admin workspace,
optional local/OIDC admin authentication, runtime capability reporting,
image-camera transport, and community documentation.

## Installation and artifacts

The release workflow prepares native Linux x86-64, prebuilt-image Compose, and
source archives, with `SHA256SUMS`, image digests, and SPDX software bills of
materials. Verify artifact checksums before use. A dry-run artifact is **not**
evidence that its referenced container images have been published.

The Compose bundle contains `release.env` and a build-free override; use its
`scripts/deploy.sh` to initialize and run the stack. The native bundle contains
the Rust core, C++ adapter, and MediaMTX; install Mosquitto/runtime dependencies
and use the standalone guide. No Bambu plugin binaries or account state are
redistributed. The reviewed plugin is obtained from Bambu on first startup.

## Compatibility and security

- Linux x86-64 only. ARM/Raspberry Pi and Docker Desktop are not validated targets.
- Real-printer testing covers the H2C installation on v0.6.0; it does not certify
  every feature of this candidate or other printer models/firmware.
- A1/P1 image cameras have automated TLS/framing/multiplexing tests only.
  Community validation is welcome; include model, firmware, client, and operation.
- Multi-printer and capability behavior is tested with synthetic fixtures, not
  a physical fleet. Fresh local data takes precedence; cloud/model fill gaps.
- OIDC is tested with a real-signature loopback provider. Your identity provider,
  HTTPS origin, callback, and subject allowlist still need operator configuration.
- Admin authentication does not change device protocol credentials. Keep printer
  ports on a trusted LAN/VPN, never on the public internet.
- Read-only is the default. Heating, motion, printing, cancellation, and storage
  writes remain explicit opt-ins; cloud connectivity is required for signed writes.
- Some firmware acknowledges internal-history deletion without deleting the file;
  the proxy detects/fails this operation rather than claiming success.
- Rust advisory exceptions are documented in `docs/dependency-security.md`.
  They describe unreachable affected operations, not upstream fixes.

Physical host reboot, a cloud-connected standalone installation, and additional
printer families remain recommended community validation, not claimed tests.
