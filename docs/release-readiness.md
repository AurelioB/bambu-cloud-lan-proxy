# Public release readiness

Target: **v0.7.0**, currently an unpublished candidate. The repository, packages,
site, and deployment are separate publication/operation decisions.

## Prepared in source

- Rust application and independently authenticated Svelte admin workspace.
- Local image-camera transport and runtime capabilities (issues 2 and 3).
- Source, native Linux x86-64, and build-free Compose packaging; offline artifact
  smoke tests and checksums run in CI.
- Full-history secret scan with one exact public OIDC-test-key exception.
- npm/Rust advisory checks; Rust exceptions documented with reachability reasoning.
- Public documentation, feature/API guides, interactive diagrams, and explicit
  unsupported/unverified compatibility labels.
- Manual **Prepare release** workflow: default is validation/packaging only.
  `publish=true` requires main, green CI/docs/security, an unused version tag,
  and produces versioned GHCR images and a **draft**, never an automatic public
  release or a moving `latest` tag.

## Owner-controlled publication steps

1. Review and merge the release branch after all checks pass.
2. Review full Git history, license notices, and generated artifacts. Scanner
   success is not a guarantee that every private datum was detected.
3. Decide whether to make the source repository public. Enable private security
   reporting and review Actions/issue access settings for community participation.
4. Run **Prepare release** on main with `publish=false` for a fresh dry run.
5. After approving registry writes, run with `publish=true`. Confirm the three
   packages' visibility explicitly; GHCR package visibility is separate from
   repository visibility. Verify pulls without credentials before recommending
   anonymous installation. Inspect SPDX inventories and upstream notices.
6. Review draft assets, image digests, checksums, release notes, and fresh-host
   installation. Only then publish the draft. No workflow changes repository
   visibility, package visibility, or promotes the draft automatically.
7. Choose the documentation URL. Set `DOCS_SITE_URL` and `DOCS_BASE_PATH`, enable
   Pages from Actions, and manually run **Publish documentation** on the approved
   revision. A static host/container is an alternative; never publish runtime data.
8. Verify anonymous clone/download, website links, container pulls, and the final
   installation instructions. Announce the tested scope and known limits.

## Validation not available in this installation

These are **not** hidden blockers requiring hardware the maintainer does not own:

- A1/P1 camera protocol with physical hardware: community validation needed.
- Physical multi-printer/AMS fleet: fixtures cover software behavior only.
- Real external OIDC provider: depends on the operator's provider configuration.
- Fresh native systemd/cloud installation: documented examples plus automated
  certificate/bootstrap and packaged-binary smoke checks, not fleet certification.
- Physical host reboot: optional; earlier container/process recovery tests passed.

The established H2C deployment was supervised and validated on v0.6.0. Deployment
of this new candidate requires a separate backup, controlled update, read-only
smoke check, and Handy/Bambuddy continuity check. No unattended physical print or
host reboot is part of public release preparation.

## Migration boundary

No application feature is waiting for a Python-to-Rust port. `native/` retains
869 lines of C++ source/headers for plugin ABI calls, callbacks, RPC dispatch,
and BambuSource media/storage framing. Its storage framing could be refactored
into Rust later, but the C++ plugin ABI still needs glue and the present process
isolation is intentional. Shell scripts remain deployment/verification helpers.
Mosquitto (C) and MediaMTX (Go) are external infrastructure, not Gateway code.
Website build/link helpers use Node; no Node or Python backend runs in production.
