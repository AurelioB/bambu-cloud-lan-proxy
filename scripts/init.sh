#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

if [ ! -f .env ]; then
  cp .env.example .env
fi

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

mkdir -p data/gateway data/proxy
docker compose run --rm --build bootstrap

echo "Initialization complete. Secrets are in .env and data/ (both ignored by Git)."
echo "Public proxy CA: data/proxy/export/bambu-cloud-lan-proxy-ca.crt"
