---
title: Start here
description: Keep Bambu Handy connected while using local printer software.
---
Bambu Cloud LAN Proxy exposes a **virtual LAN printer** for each cloud-paired printer in your Bambu account. Point Bambuddy or another compatible client at the virtual IP instead of the physical printer.

Bambu Handy stays connected to Bambu Cloud. Your local client connects to the proxy. The proxy uses local services where available and the official networking plugin for signed cloud operations.

## Choose your installation

| Method | Best for | What you manage |
| --- | --- | --- |
| [Docker Compose](/docs/install/compose/) | Most users | One coordinated stack, with bootstrap and health checks |
| [Docker](/docs/install/docker/) | Existing container operators | Containers, startup order, mounts, restarts, and networking |
| [Standalone Linux](/docs/install/standalone/) | Linux/systemd operators | Source builds, dependencies, certificates, broker, and services |

Start with [requirements and compatibility](/docs/compatibility/), then configure your [first printer](/docs/first-printer/).

:::caution[Not an offline replacement for Bambu Cloud]
Signed control and print submission require Bambu Cloud and your account. Local status, files, and camera depend on what your printer and firmware expose. This is independent community software, not an official Bambu Lab product.
:::

## What is included?

- One reserved virtual IP per printer, using standard LAN protocol ports.
- TLS MQTT reports and policy-controlled signed commands.
- FTPS job staging and native printer file access.
- Local RTSPS or chamber-image liveview on supported model profiles.
- A Svelte/shadcn-svelte administration console with light and dark themes.
- Optional built-in admin accounts or OIDC; authenticated admin access in every mode.

See [features and data paths](/docs/features/) and the [API reference](/docs/api/).

## Documentation scope

These docs describe the current source tree, including admin authentication added after the original v0.6.0 release. Do not assume an older release contains every documented feature. Install from a reviewed revision containing the documentation and record that revision for upgrades.

The repository must be accessible to you to clone it. No public container registry or prebuilt native release asset is assumed by these instructions.
