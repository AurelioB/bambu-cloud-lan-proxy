---
title: Docker without Compose
description: Run the same service topology with explicit Docker commands.
---
Use [Compose](/docs/install/compose/) unless you deliberately want to manage container lifecycle yourself. A single `docker run` of the application image is **not** the full stack: it also needs a broker, certificates, persistent/shared storage, and endpoint/address workers.

This example uses Linux Docker Engine, a private bridge for core/broker traffic, and host networking for virtual IP workers. Run commands from a fresh repository checkout. They use distinct container names and do not replace an existing deployment.

## 1. Build the images

```sh
git clone https://github.com/AurelioB/bambu-cloud-lan-proxy.git
cd bambu-cloud-lan-proxy
docker build -f Dockerfile.proxy -t bambu-cloud-lan-proxy:local .
docker build -f docker/bootstrap.Dockerfile -t bambu-proxy-bootstrap:local .
docker build -f docker/mqtt.Dockerfile -t bambu-proxy-mqtt:local .
docker run --rm --network none bambu-cloud-lan-proxy:local --version
```

## 2. Prepare private configuration

```sh
umask 077
mkdir -p .secrets/docker data/gateway
cp examples/docker/*.env .secrets/docker/
id -u
id -g
openssl rand -hex 4
```

Edit the four files in `.secrets/docker/`:

- Use the same generated eight-character code in `bootstrap.env`, `core.env`, and `endpoints.env`.
- In `bootstrap.env`, set runtime UID/GID to your host user's numeric IDs.
- In `addresses.env`, set the real Linux LAN interface.
- In `core.env`, choose the appropriate Bambu region and optional admin authentication.

These are **Docker env files**, not Compose interpolation files. Do not write `$OTHER_VARIABLE` or quote values expecting expansion. Keep them out of Git. Admin secrets belong in the core file, not endpoint files.

Set these shell variables for the following commands:

```sh
proxy_uid=$(id -u)
proxy_gid=$(id -g)
```

## 3. Bootstrap (also used after address changes)

```sh
docker run --rm --network none \
  --env-file .secrets/docker/bootstrap.env \
  -v "$PWD/data/proxy:/data" \
  -v "$PWD/data/gateway:/registry:ro" \
  -v "$PWD/data/staging:/staging" \
  -v "$PWD/data/remote-cache:/remote-cache" \
  -v "$PWD/data/camera:/camera-state" \
  -v "$PWD/data/local-status:/local-status" \
  -v "$PWD/data/endpoint-state:/endpoint-state" \
  -v "$PWD/data/address-state:/address-state" \
  bambu-proxy-bootstrap:local
```

This creates server material, a private local CA, a public CA export, hashed broker credentials, and writable runtime directories. Keep `data/` intact.

## 4. Broker and core

```sh
docker network create bambu-proxy-net
docker run -d --name bambu-mqtt --restart unless-stopped \
  --network bambu-proxy-net --user 1883:1883 \
  --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges \
  -p 127.0.0.1:1883:1883 -p 127.0.0.1:18883:8883 \
  -v "$PWD/data/proxy/runtime:/mosquitto/secrets:ro" \
  bambu-proxy-mqtt:local
docker exec bambu-mqtt mosquitto_sub -h 127.0.0.1 -p 1883 \
  -t '$SYS/broker/version' -C 1 -W 5
docker run -d --name bambu-core --restart unless-stopped \
  --network bambu-proxy-net --user "$proxy_uid:$proxy_gid" \
  --read-only --tmpfs /tmp:size=256m --cap-drop ALL --security-opt no-new-privileges \
  --env-file .secrets/docker/core.env -p 127.0.0.1:4844:4844 \
  -v "$PWD/data/gateway:/data" \
  -v "$PWD/data/proxy/export:/proxy-ca:ro" \
  -v "$PWD/data/staging:/staging:ro" \
  -v "$PWD/data/remote-cache:/remote-cache" \
  -v "$PWD/data/local-status:/local-status:ro" \
  -v "$PWD/data/endpoint-state:/endpoint-state:ro" \
  bambu-cloud-lan-proxy:local service
```

Wait for `curl --fail http://127.0.0.1:4844/api/health` to succeed. Check `docker logs bambu-core` if it does not; the plugin must download and initialize first.

## 5. Address and endpoint workers

```sh
docker run -d --name bambu-addresses --restart unless-stopped \
  --network host --user 0:0 --read-only --cap-drop ALL \
  --cap-add NET_ADMIN --cap-add SETPCAP --cap-add SETUID --cap-add SETGID \
  --security-opt no-new-privileges --env-file .secrets/docker/addresses.env \
  -v "$PWD/data/gateway:/data:ro" -v "$PWD/data/address-state:/address-state" \
  --entrypoint setpriv bambu-cloud-lan-proxy:local \
  --reuid="$proxy_uid" --regid="$proxy_gid" --clear-groups \
  --inh-caps=+net_admin --ambient-caps=+net_admin \
  --bounding-set=-setpcap,-setuid,-setgid \
  bambu-cloud-lan-proxy endpoint-address-manager
docker run -d --name bambu-endpoints --restart unless-stopped \
  --network host --user 0:0 --read-only --tmpfs /tmp:size=64m --cap-drop ALL \
  --cap-add NET_BIND_SERVICE --cap-add SETPCAP --cap-add SETUID --cap-add SETGID \
  --security-opt no-new-privileges --env-file .secrets/docker/endpoints.env \
  -v "$PWD/data/gateway:/data:ro" \
  -v "$PWD/data/proxy/runtime/certs:/secrets/certs:ro" \
  -v "$PWD/data/staging:/staging" -v "$PWD/data/remote-cache:/remote-cache" \
  -v "$PWD/data/camera:/camera-state" -v "$PWD/data/local-status:/local-status" \
  -v "$PWD/data/endpoint-state:/endpoint-state" \
  --entrypoint setpriv bambu-cloud-lan-proxy:local \
  --reuid="$proxy_uid" --regid="$proxy_gid" --clear-groups \
  --inh-caps=+net_bind_service --ambient-caps=+net_bind_service \
  --bounding-set=-setpcap,-setuid,-setgid \
  bambu-cloud-lan-proxy endpoint-manager
```

The startup `setpriv` invocation drops to your runtime UID while retaining only each worker's necessary network capability.

## 6. Connect and maintain

Complete [first-printer setup](/docs/first-printer/). After saving virtual IPs, stop `bambu-endpoints` and `bambu-mqtt`, repeat the bootstrap command, then start those two containers again. The address manager applies registry changes automatically.

These commands do not define Compose's dependency health checks. You own startup readiness, environment changes, upgrades, and recreation of stopped containers. Do not rerun the creation commands over existing names. To apply a changed env file, back up and recreate only the affected container with the same persistent mounts; `docker restart` does not reload env files.

Keep all published internal ports on loopback and all virtual ports on trusted networks. See [operations](/docs/operations/) for backup, certificate pinning, and unknown write outcomes.
