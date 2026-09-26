#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
host=$(sed -n 's/^PROXY_BIND_IP=//p' .env | tail -n 1)
host=${host:-127.0.0.2}
docker compose exec -T -e NATIVE_PROBE_HOST="$host" endpoint-manager \
  bambu-cloud-lan-proxy native-probe
