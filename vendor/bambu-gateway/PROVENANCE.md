# Vendored Bambu Gateway components

This directory contains the portions of
[LeoLobato/bambu-gateway](https://github.com/LeoLobato/bambu-gateway) needed by
Bambu Cloud LAN Proxy. They are vendored so this project can be built without
downloading Bambu Gateway source or relying on its published container image.

- Upstream commit: `79db09fff4929e9a7537453078031a51b3834c5f`
- Upstream version at import: `3.0.0`
- Source archive SHA-256:
  `791bfab95edfbc801466434ca014ca4aa8495af9f21993ffe04bb9e595ef81f0`
- Imported: 2026-08-31
- License: MIT; the original copyright and license text are retained in
  [LICENSE](LICENSE).

The imported source is intentionally not updated automatically. Local changes
to the native plugin host are applied by `docker/gateway-host/upstream.patch`
and `docker/gateway-host/storage_bridge.*` during the image build. Any future
upstream update must be an explicit, reviewed import.
