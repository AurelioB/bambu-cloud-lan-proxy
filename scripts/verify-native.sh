#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
host=$(sed -n 's/^PROXY_BIND_IP=//p' .env | tail -n 1)
host=${host:-127.0.0.2}
timeout 5 openssl s_client -connect "${host}:6000" </dev/null >/dev/null 2>&1
echo "native-file TLS handshake succeeded"
