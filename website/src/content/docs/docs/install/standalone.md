---
title: Install standalone on Linux
description: Build and supervise the proxy without Docker.
---
This is the advanced **Linux x86-64, systemd, source-build** path. It runs the same Rust core, native adapter, Mosquitto broker, and endpoint/address managers as Compose. “Standalone” does not mean one process with no dependencies.

The supplied configuration targets Debian/Ubuntu-style paths. Review it for your distribution. These instructions are not a prebuilt binary installer.

## 1. Build the application

Install Rust 1.96 (the CI toolchain), Node.js 24 with npm, and native build tools. On a suitable Debian/Ubuntu host:

```sh
sudo apt-get update
sudo apt-get install -y build-essential cmake pkg-config libssl-dev \
  nlohmann-json3-dev ca-certificates curl git jq openssl \
  mosquitto mosquitto-clients iproute2
git clone https://github.com/AurelioB/bambu-cloud-lan-proxy.git
cd bambu-cloud-lan-proxy
npm --prefix web ci
npm --prefix web run check
npm --prefix web run build
cmake -S native -B build/native -DCMAKE_BUILD_TYPE=Release
cmake --build build/native --parallel
cargo build --locked --release
cargo test --locked
```

Rust embeds `web/dist`; build the admin UI **before** compiling Rust. Install Rust and Node through their official distribution instructions if not already present.

The distro Mosquitto package may auto-start a default broker. Check `sudo ss -ltnp` for port 1883. If that newly installed default instance is unused, stop/disable it before running the dedicated broker below. Do not stop a broker used by other applications; instead select free internal ports consistently in the example configuration.

## 2. Install under a dedicated account

For a fresh installation (do not recreate an existing service account):

```sh
sudo useradd --system --user-group --home-dir /var/lib/bambu-proxy \
  --no-create-home --shell /usr/sbin/nologin bambu-proxy
sudo install -d -m 750 -o bambu-proxy -g bambu-proxy \
  /var/lib/bambu-proxy /var/lib/bambu-proxy/cloud /var/lib/bambu-proxy/tls
sudo install -d -m 700 /etc/bambu-proxy /usr/local/lib/bambu-proxy
sudo install -m 755 target/release/bambu-cloud-lan-proxy /usr/local/bin/
sudo install -m 755 build/native/bambu-proxy-native /usr/local/bin/
sudo install -m 755 docker/bootstrap.sh /usr/local/lib/bambu-proxy/bootstrap.sh
sudo install -m 755 examples/standalone/bootstrap.sh /usr/local/sbin/bambu-proxy-bootstrap
sudo install -m 600 examples/standalone/environment /etc/bambu-proxy/environment
sudo install -m 644 examples/standalone/mosquitto.conf /etc/bambu-proxy/mosquitto.conf
sudo install -m 644 docker/proxy.acl /etc/bambu-proxy/proxy.acl
sudo chown root:bambu-proxy /etc/bambu-proxy
sudo chmod 750 /etc/bambu-proxy
```

The service account needs to traverse the configuration directory to read the broker config. The environment remains root-only; systemd reads it before changing user.

Use `sudoedit /etc/bambu-proxy/environment`. Replace **both** `CHANGE_ME` values with the same random eight-character ASCII letter/digit code; `openssl rand -hex 4` generates an appropriate value. Set `ENDPOINT_INTERFACE` and your cloud region. Do not use the example code in production.

The [complete environment example](/examples/standalone/environment) maps every persistent directory and listener used here. Keep its paths consistent across core, endpoints, and bootstrap. It is shell-sourced by the root bootstrap helper, so keep it **root-owned, operator-controlled, and never writable by the service account**.

## 3. Bootstrap certificates and install services

```sh
sudo /usr/local/sbin/bambu-proxy-bootstrap
sudo install -m 644 examples/standalone/bambu-proxy-*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now bambu-proxy-broker.service bambu-proxy-core.service
sudo systemctl enable --now bambu-proxy-addresses.service bambu-proxy-endpoints.service
```

The native adapter obtains the reviewed plugin during core startup. A configured printer registry is written when cloud discovery succeeds; address/endpoints workers can wait for it.

The examples grant only `CAP_NET_BIND_SERVICE` to endpoints and `CAP_NET_ADMIN` to the address manager. The core/broker have neither. All run as `bambu-proxy`; this is not a fully isolated multi-user host security boundary.

## 4. Camera dependency

Camera support additionally requires a compatible **MediaMTX binary at `/usr/local/bin/mediamtx`**. Follow the [official MediaMTX installation guide](https://mediamtx.org/docs/kickoff/install), choose Linux amd64, verify the release integrity information, and install the reviewed binary at that exact path.

The current container build uses MediaMTX **v1.20.1**, with its image digest pinned in `Dockerfile.proxy`; native operators must manage their MediaMTX version. Without it, camera workers fail/retry while the core and other endpoint protocols can still operate. Do not configure physical camera credentials until this dependency is installed if you want to avoid those failures.

## 5. Configure and apply printers

Open the loopback console or use an SSH tunnel, then follow [first-printer setup](/docs/first-printer/).

After saving virtual IPs, refresh the certificate SANs and restart listeners:

```sh
sudo systemctl stop bambu-proxy-endpoints.service bambu-proxy-broker.service
sudo /usr/local/sbin/bambu-proxy-bootstrap
sudo systemctl start bambu-proxy-broker.service bambu-proxy-endpoints.service
```

The address manager reconciles saved addresses automatically. Export the public CA from:

```text
/var/lib/bambu-proxy/tls/export/bambu-cloud-lan-proxy-ca.crt
```

Do not grant users access to the CA signing key just to copy the public certificate.

## Check and maintain

```sh
systemctl status bambu-proxy-core.service bambu-proxy-endpoints.service
sudo journalctl -u bambu-proxy-core.service -n 100 --no-pager
curl --fail http://127.0.0.1:4844/api/health
```

Stop all four project services before a consistent backup of `/etc/bambu-proxy` and `/var/lib/bambu-proxy`. Preserve ownership and permissions. Build updated binaries separately, stop the services, install the binaries, then restart. Restart the core after changing admin authentication.

This recipe and its units are provided as reviewed source examples. Automated checks validate syntax and certificate/bootstrap behavior; a full cloud-connected native deployment still needs verification on your host.
