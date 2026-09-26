# Gateway independence and Rust assessment

Decision date: 2026-09-03. Scope: replace Gateway, not Bambu's proprietary plugin.

## Findings from the code review

The former `launcher.py` imported `app.main`, patched
`CloudPrinterClient.__init__/handle_event`, and reused `app.mqtt_client` command
builders. Docker built the entire vendored Python app and React frontend.
The native host also came from Gateway plus our storage patch. Thus vendoring
only removed upstream availability risk; it did not create a focused proxy.

What we actually need:

| Responsibility | First-party replacement |
| --- | --- |
| Plugin acquisition/integrity | `plugin_install.rs`: HTTPS, exact version and checked-in binary hashes |
| Browser-ticket sign-in | `cloud_auth.rs` plus authenticated admin API |
| Plugin process, request correlation, events | `plugin_rpc.rs`: bounded JSON-lines RPC, no command retries |
| Cloud login/subscription lifecycle | `service.rs`: account discovery, subscriptions, reconnect, certificate events |
| Printer reports | Preserve raw JSON; `cloud_printer.rs` only interprets job-safety state |
| Pause/resume/speed/light | `commands.rs`, separate from transport |
| Print upload/dispatch | Existing staging/policy code plus native PrintArguments ABI |
| Printer file/media access | Existing project-owned storage bridge and file facades |
| Physical ABI bridge | `native/`: dlopen, C++ argument types/callbacks, isolated process |

Not carried forward: Gateway's dashboard, slicer/catalog/profile tools, APNs,
notification system, parsed AMS/temperature UI model, duplicate LAN clients,
and REST command endpoints bypassing the proxy policy. Bambuddy/Studio remain
the primary clients; the embedded Svelte console covers setup, health, and policy.

The Compose service name `gateway` and data directory are compatibility names,
not dependencies. `import-gateway-data.sh` is optional migration tooling only.
There are no runtime imports of Gateway, vendored sources, source/image fetches,
or monkeypatches. The native storage bridge predates this change and is our code.
Source-informed ABI/protocol review is acknowledged in `THIRD_PARTY_NOTICES`;
this is not claimed to be a clean-room implementation.

## Rust migration decision

The application layer is now Rust/Tokio. The migration keeps the small isolated
C++ shim because the proprietary plugin exposes C++ ABI types that must not be
modeled as a plain Rust `extern "C"` interface.

Rust provides typed state/concurrency checks and one application binary for the
cloud service, FTPS, native files, discovery, telemetry, camera setup, and TCP
relay. It does not remove cloud/firmware uncertainty or the need for supervised
hardware compatibility testing.

The plugin's functions use `std::string`, `std::vector`, `std::map`,
`std::function`, and a by-value print-argument struct. Plain Rust `extern "C"`
cannot safely pretend these are C types. The Rust daemon still needs C++ glue.
Rust cannot make a wrong
proprietary ABI layout safe, eliminate the proprietary binary, or by itself
produce native ARM support for an x86_64-only plugin. Process isolation remains
useful regardless of the supervisor language.

Primary references: [Rust FFI documentation](https://doc.rust-lang.org/nomicon/ffi.html),
[CXX interop](https://cxx.rs/), and
[BambuStudio plugin declarations](https://github.com/bambulab/BambuStudio/blob/master/src/slic3r/Utils/bambu_networking.hpp).
Our pinned ABI is 02.05.02.58; current upstream declarations are not an
automatic replacement for those exact binary layouts.

Native plugin crash isolation and the bounded RPC contract remain unchanged.

## Safety and limitations

- An RPC timeout is an unknown outcome. Do not automatically replay writes.
- Stale or unrecognized print state prevents automatic job dispatch.
- Submission completion waits for the native call to return. An uncertain
  outcome keeps the staging snapshot and `submissions/<serial>.pending` latch.
  Restart does not clear it. Before manually clearing it, stop the proxy and
  confirm on the printer/Handy whether the job started; do not clear while a
  native worker may still be running. Preserved snapshots need manual cleanup.
- A crashed native process fails readiness and pending calls. It is not
  restarted in-process; restart the container after examining the cause.
  Docker health status alone does not restart an unhealthy live container.
- Native stdout is isolated from proprietary diagnostic output; stderr is
  discarded rather than risk surfacing credentials. Native errors are sanitized.
- Download pins are hashes of the previously validated Bambu binaries, not a
  Bambu signature. If the CDN stops offering those exact files, installation
  fails until an explicit compatibility review/update. No fallback to latest.
- Account sessions remain inside plugin state; signing material is not extracted.
- Runtime and base-image pins do not make apt/npm/crates.io transitive dependencies a
  hermetic build. The project is independent of Gateway, not every package source.

## Cutover checklist

Do not run both cloud supervisors against the same live data directory.
The rewrite has not been automatically deployed to the printer environment.

1. Run unit, native-RPC and image-build checks without printer/network access.
2. Back up existing private data; test old session import and fresh browser
   login against a separate copy, never concurrent writers of the same state.
3. Disable printer-write gates for initial cutover. Verify fresh reports,
   local/cloud fallback, reconnect after network loss, and account expiry.
4. Verify camera plus printer-file listing/download with the new media bridge.
5. With explicit operator supervision and the printer idle, verify a harmless
   control, opt-in storage mutations, and a deliberately selected test print.
6. Validate cancellation/unknown outcomes and rollback before declaring parity.

Unit tests and offline plugin initialization cannot establish end-to-end
control/printing parity. Prior tests of the Gateway-based stack are not proof
that the newly implemented session orchestration behaves identically.
