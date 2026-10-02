#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

if [ ! -f .env ]; then
  umask 077
  cp .env.example .env
fi
chmod 600 .env

current_code=$(sed -n 's/^PROXY_ACCESS_CODE=//p' .env | tail -n 1)
if [ -z "$current_code" ] || [ "$current_code" = "CHANGE_ME" ]; then
  if command -v openssl >/dev/null 2>&1; then
    access_code=$(openssl rand -hex 4 | tr 'a-f' 'A-F')
  else
    access_code=$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n' | tr 'a-f' 'A-F')
  fi
  sed -i "s/^PROXY_ACCESS_CODE=.*/PROXY_ACCESS_CODE=$access_code/" .env
fi

if grep -q '^PUID=1000$' .env; then
  sed -i "s/^PUID=1000$/PUID=$(id -u)/" .env
fi
if grep -q '^PGID=1000$' .env; then
  sed -i "s/^PGID=1000$/PGID=$(id -g)/" .env
fi

mkdir -p data/gateway data/proxy data/camera
if [ ! -e data/gateway/printers.json ]; then
  printf '[]\n' > data/gateway/printers.json
  chmod 600 data/gateway/printers.json
fi
if [ ! -f data/gateway/printers.json ]; then
  echo 'data/gateway/printers.json must be a regular JSON file, not a directory' >&2
  exit 1
fi
if [ -f release.env ]; then
  ./scripts/compose.sh pull bootstrap mqtt gateway endpoint-manager endpoint-addresses
  ./scripts/compose.sh run --rm --no-build bootstrap
else
  ./scripts/compose.sh run --rm --build bootstrap
fi

echo "Initialization complete. Secrets are in .env and data/ (both ignored by Git)."
echo "Public proxy CA: data/proxy/export/bambu-cloud-lan-proxy-ca.crt"
