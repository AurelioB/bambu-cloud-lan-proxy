# Standalone binary bundle

The release workflow prepares a Linux x86-64 bundle with:

- `bin/bambu-cloud-lan-proxy`: Rust application with the admin UI embedded.
- `bin/bambu-proxy-native`: isolated C++ plugin/media adapter.
- `bin/mediamtx`: the camera dependency copied from the reviewed image.
- Native installation examples, bootstrap scripts, and license notices.

It does **not** contain credentials, certificates, plugin state, Mosquitto,
or Bambu's proprietary plugin. The plugin is downloaded and hash-checked at
first cloud-core startup. The bundle is not a statically linked universal binary.

Use Debian 12 x86-64 or a compatible newer glibc/OpenSSL 3 host, such as Ubuntu
24.04 x86-64. Install runtime dependencies and verify release checksums first:

```sh
sha256sum --check SHA256SUMS
sudo apt-get update
sudo apt-get install -y ca-certificates curl jq openssl libssl3 \
  mosquitto mosquitto-clients iproute2
```

On distributions using a renamed OpenSSL 3 runtime package, use the equivalent
provided by that distribution. The embedded native code additionally needs the
usual glibc/libstdc++ runtime. Release CI smoke-tests binaries in the Debian 12
runtime image; a full systemd/cloud installation on your host is not certified
by this test.

After extracting the archive, install the three executables into
`/usr/local/bin/` with mode 755, and follow the standalone documentation from
the dedicated-account step. Replace source-build binary paths with the bundle's
`bin/` paths. MediaMTX is included, so a separate MediaMTX download is unnecessary.

Do not run the core as root, expose the broker to the internet, or reuse another
running instance's writable plugin state. See the website's standalone guide
for the complete service, permission, certificate, and update instructions.
