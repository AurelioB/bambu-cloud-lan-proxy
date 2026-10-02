# Changelog

## 0.7.0 — unreleased candidate

- Responsive multi-printer Svelte/shadcn-svelte admin workspace, capability-aware
  cards/configuration, and light/dark/system appearance.
- Optional bundled local administrator and OIDC authentication, with private UI
  and HTTP routes protected independently of virtual-printer access codes.
- Local image-camera relay for A1/A1 mini/A2L/P1P/P1S profiles, sharing the native
  file port without collisions. **Automated protocol tests only; these printer
  families have not been physically validated.**
- Firmware, AMS units/slots, sensors, and toolhead evidence from fresh local/cloud
  reports, with conservative model fallback and no automatic permission grants.
- Independent public documentation site, installation examples, OpenAPI reference,
  and animated local/cloud routing and Handy-continuity diagrams.
- Manual release preparation, native/Compose/source archives, SHA-256 checksums,
  versioned container packaging, SBOM publication, and full-history secret scanning.
- Dependency audit with documented reachability-scoped exceptions; updated the
  yanked yoke-derive dependency.

## 0.6.0 — 2026-09-25

Standalone Rust migration and supervised H2C cutover: cloud control/printing,
telemetry, native printer files, RTSPS camera, persistent virtual IP management,
process recovery, and Handy/Studio/Bambuddy continuity. See the release-validation
record for exactly what was exercised and the firmware deletion limitation.
