---
title: Permissions & printing
description: Enable only the printer operations your clients need.
---
The **Permissions** page saves policy to the core's private `admin-settings.json`. Saved settings take precedence over startup environment defaults for the corresponding policy fields.

## MQTT policy modes

| Mode | Intended behavior |
| --- | --- |
| `read_only` | Allow supported query commands |
| `controls` | Add pause, resume, speed, and light commands; cancellation is separately gated |
| `full_control` | Allow valid signed commands supported by the official cloud path, subject to specific gates |

:::caution[“Read-only” is not a master kill switch]
Cloud print submission has its own permission. To operate without printer mutations, keep all write opt-ins off as well as selecting read-only mode. Review the complete settings object rather than just its mode.
:::

There is a developer `unsafe` backend mode, but it is not offered by the admin UI or accepted as persisted admin policy. It is not a recommended community configuration.

## Independent opt-ins

| Setting | Enables |
| --- | --- |
| `allow_cancel` | Cancellation when the relevant command policy permits it |
| `cloud_print_enabled` | Submitting staged jobs through the cloud |
| `remote_reprint_enabled` | Reprinting an existing printer file through the relevant print path |
| `storage_allow_upload` | Uploading into physical printer storage |
| `storage_allow_delete` | Deleting supported physical printer files |
| `advertise_controls` | How control capability is advertised to clients; not authorization by itself |

Printer-resident `file:///` reprint requires full-control mode and its own reprint opt-in; it is distinct from staged cloud submission. Enabling a permission does not override hardware, firmware, account, or protocol constraints.

## Suggested progression

1. Start with `read_only` and every mutation toggle off.
2. Validate reports, file reads, and supported camera access.
3. Enable `controls` if your trusted client needs ordinary controls.
4. Enable printing/reprinting only after reviewing the client's dispatch behavior.
5. Use full control and storage mutations only when you need them and understand the client's commands.

Staging uploads are size-limited. Cloud submissions are ZIP-validated, copied to immutable snapshots, and serialized per printer. Those protections do not make an arbitrary print file mechanically safe.

## Unknown outcomes

A timed-out write may have reached Bambu Cloud or the printer. Do not retry automatically. Check current printer state and job identity through fresh telemetry or Handy first.

Disabling future commands, logging out of the console, or restarting the service does not undo a command already accepted by the printer.
