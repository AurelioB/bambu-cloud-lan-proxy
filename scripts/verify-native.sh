#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
. ./.env
host=${PROXY_BIND_IP:-127.0.0.2}
timeout 5 openssl s_client -connect "${host}:6000" </dev/null >/dev/null 2>&1
echo "native-file TLS handshake succeeded"
