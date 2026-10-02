---
title: Releases & prebuilt installation
description: Verify release artifacts, choose a deployment method, and understand what has been validated.
---
The current source is a **v0.7.0 candidate**, not a claim of a published public
image or binary. Check the [GitHub releases](https://github.com/AurelioB/bambu-cloud-lan-proxy/releases)
for the actual artifacts and tested revision. Repository and package access
must be available before these download/install paths can work.

## Artifacts

| Artifact | Contents | Requirements |
| --- | --- | --- |
| `*-compose.tar.gz` | Compose configuration, build-free override, image references, setup scripts | Linux x86-64, Docker Engine, Compose 2.24.4+; published images must be accessible |
| `*-linux-x86_64.tar.gz` | Rust core with embedded admin, C++ adapter, MediaMTX, service examples | Compatible glibc/OpenSSL 3 host; Mosquitto and other runtime packages |
| `*-source.tar.gz` | Source from the exact reviewed commit | Rust/Node/C++ toolchains for a source build |
| `SHA256SUMS`, `build.json`, `IMAGE_DIGESTS`, `*.spdx.json` | Integrity checks, revision/platform metadata, published image digests, dependency inventories | Review alongside the release notes |

Dry-run CI artifacts have archives/checksums/build metadata, but no published
image digests or registry availability guarantee. Published draft preparation
adds the image digests and three SPDX inventories. No archive contains Bambu's
proprietary plugin, printer credentials, private certificates, or account state.

## Prebuilt Docker Compose

After downloading a published release's Compose archive and `SHA256SUMS`, check
the archive's checksum. The checksum file lists all assets; check the selected
archive's entry if you have not downloaded them all:

```sh
grep 'bambu-cloud-lan-proxy-0.7.0-compose.tar.gz' SHA256SUMS | sha256sum --check
tar -xzf bambu-cloud-lan-proxy-0.7.0-compose.tar.gz
cd bambu-cloud-lan-proxy-0.7.0-compose
./scripts/init.sh
# Edit .env, set your interface/region, and keep writes disabled initially.
./scripts/deploy.sh
./scripts/compose.sh ps
```

The bundle's `release.env` selects the core, bootstrap, and broker image versions.
The wrapper merges the build-free override; no Rust/Node/C++ compilation runs.
For subsequent operations use `scripts/compose.sh`, not plain `docker compose`,
so the correct images and override remain selected. It keeps `.env` secrets
separate from the distributed image references. Do not use `--build` with a
prebuilt bundle.

Versioned registry tags are not a universal immutability guarantee. You can
replace image references in `release.env` with the corresponding `@sha256:...`
references from `IMAGE_DIGESTS`. First-printer setup, reserved virtual IPs,
certificate refresh, and safe policy selection follow the
[Compose guide](/docs/install/compose/) and [first-printer guide](/docs/first-printer/).

Back up your configuration/state before upgrades. Never extract a new release
over your only `.env`/data backup or run two cores against the same plugin state.

## Prebuilt standalone

The binary bundle is built for Debian 12 Linux x86-64 and compatible newer
glibc/OpenSSL 3 systems. It is not static, not ARM, and not a Windows/macOS app.

Verify and extract the native archive, install runtime dependencies, then follow
the [standalone guide](/docs/install/standalone/) from the dedicated-account step.
Use `bin/bambu-cloud-lan-proxy` and `bin/bambu-proxy-native` instead of source-build
paths; install the included `bin/mediamtx` too, rather than downloading another
camera implementation. Rust, npm, CMake, and compiler toolchains are unnecessary
for this installation path. The bundle's `docs/standalone-binary.md` documents
runtime package requirements.

## Tested scope

- The v0.6.0 H2C deployment was physically tested with Handy, Studio, and Bambuddy.
- New A1/P1 image-camera handling has automated TLS/protocol tests only; hardware
  compatibility reports are welcome. Lack of these printers is not disguised as
  successful fleet validation or an impossible maintainer release gate.
- Multi-printer/AMS fixtures and a signed loopback OIDC provider cover software
  behavior; they are not a physical fleet or provider-specific certification.
- CI tests the actual packaged executables offline, configuration, browser UI,
  links, and certificate bootstrap. Native cloud-connected fresh-host installs
  and host reboot remain additional validation.
- Rust advisory exceptions have explicit reachability reasoning in the
  [dependency security record](https://github.com/AurelioB/bambu-cloud-lan-proxy/blob/main/docs/dependency-security.md).

## Maintainer publication

The manual **Prepare release** workflow defaults to validation/packaging only.
With explicit `publish=true`, after all checks pass on main, it prepares versioned
GHCR images, SBOMs, and a **draft** release. It does not change repository/package
visibility, automatically publish the draft, or deploy a printer stack.

Verify anonymous repository access and container pulls before announcing a
public release; GHCR package visibility is a separate owner-controlled setting.
Review generated assets/licenses and publish the static docs on the chosen URL.
The detailed owner checklist lives in the
[release-readiness record](https://github.com/AurelioB/bambu-cloud-lan-proxy/blob/main/docs/release-readiness.md).
