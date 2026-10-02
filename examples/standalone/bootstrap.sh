#!/bin/sh
# Run as root after installing the reviewed bootstrap and root-owned env file.
set -eu
if [ "$(id -u)" != 0 ]; then
  echo 'Run this bootstrap as root.' >&2
  exit 1
fi
set -a
. /etc/bambu-proxy/environment
PROXY_RUNTIME_UID=$(id -u bambu-proxy)
PROXY_RUNTIME_GID=$(id -g bambu-proxy)
PROXY_RUNTIME_CERT_GID=$PROXY_RUNTIME_GID
set +a
exec /usr/local/lib/bambu-proxy/bootstrap.sh
