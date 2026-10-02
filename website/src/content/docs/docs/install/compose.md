---
title: Install with Docker Compose
description: Recommended installation on a Linux x86-64 Docker host.
---
Compose coordinates the broker, cloud core, endpoint processes, address manager, and certificate bootstrap. It builds images from source; no published container image is required.

## 1. Get the source

Install Docker Engine with Compose v2, Git, and OpenSSL on your Linux x86-64 host. See the [official Docker installation instructions](https://docs.docker.com/engine/install/) for your distribution.

```sh
git clone https://github.com/AurelioB/bambu-cloud-lan-proxy.git
cd bambu-cloud-lan-proxy
./scripts/init.sh
```

The repository must be public or your Git credentials must have access. Initialization creates a local `.env`, generates a random eight-character proxy code if needed, and prepares certificates and storage. It does not complete cloud login.

## 2. Review configuration

Edit `.env`. Find the host's LAN interface with `ip -brief address`. For example:

```dotenv
PROXY_ENDPOINT_INTERFACE=enp3s0
PROXY_MODE=read_only
PROXY_ALLOW_CANCEL=false
PROXY_CLOUD_PRINT_ENABLED=false
PROXY_STORAGE_ALLOW_UPLOAD=false
PROXY_STORAGE_ALLOW_DELETE=false
PROXY_STORAGE_ALLOW_REPRINT=false
BAMBU_CLOUD_REGION=US
GATEWAY_BIND_IP=127.0.0.1
GATEWAY_PORT=4844
```

Keep the generated `PROXY_ACCESS_CODE`. Restrict access to configuration:

```sh
chmod 600 .env
docker compose config --quiet
```

Do not post expanded `docker compose config` output: it contains secrets. Interface names and cloud regions above are examples, not auto-detected choices.

## 3. Start

```sh
./scripts/deploy.sh
docker compose ps
docker compose logs --tail=100 gateway mqtt endpoint-manager endpoint-addresses
```

First start compiles the application and downloads the pinned official plugin. Allow time for those steps.

The service named `gateway` is **this project's Rust cloud/admin core**. It is not an installation of Bambu Gateway.

## 4. Sign in and add printers

Follow [Connect your first printer](/docs/first-printer/). After assigning virtual IPs:

```sh
./scripts/apply-endpoints.sh
```

Leave the legacy-single-endpoint Compose profile disabled for normal multi-printer use.

## Storage and networking

Keep the whole `data/` directory and `.env` between upgrades. The stack uses host networking for per-printer listeners and Linux address aliases, not a separate bridged container IP per printer.

Only the address manager receives network-administration capability. The endpoint manager receives permission to bind low-numbered ports. The cloud core and broker do not need those capabilities.

The default admin and internal broker mappings are loopback-only. If you change `GATEWAY_BIND_IP`, make sure the endpoint manager's configured storage API URL remains reachable; it defaults to `127.0.0.1:GATEWAY_PORT`.

## Stop or update

```sh
docker compose stop
# Start the existing stack again:
docker compose up -d
```

For upgrades, back up first and follow [operations](/docs/operations/). Do not run two stacks with the same virtual addresses or shared writable plugin state.
