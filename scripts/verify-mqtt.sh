#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

serial=${1:-}
if [ -z "$serial" ]; then
  echo "Usage: $0 PRINTER_SERIAL" >&2
  exit 2
fi

ca_file="$project_dir/data/proxy/export/bambu-cloud-lan-proxy-ca.crt"
if [ ! -s "$ca_file" ]; then
  echo "Missing proxy CA; run ./scripts/init.sh first." >&2
  exit 1
fi

echo "Waiting for one cloud-backed report from $serial..."
docker run --rm --network host \
  --env-file .env \
  -e PROXY_TEST_SERIAL="$serial" \
  -v "$ca_file:/tmp/proxy-ca.crt:ro" \
  --entrypoint sh \
  eclipse-mosquitto:2 \
  -c 'exec mosquitto_sub \
    -h "$PROXY_BIND_IP" -p 8883 \
    -u bblp -P "$PROXY_ACCESS_CODE" \
    --cafile /tmp/proxy-ca.crt \
    -t "device/$PROXY_TEST_SERIAL/report" -C 1 -W 30'
