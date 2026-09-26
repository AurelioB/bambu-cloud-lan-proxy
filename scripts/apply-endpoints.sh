#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

if [ ! -f .env ]; then
  echo "Run ./scripts/init.sh first" >&2
  exit 1
fi

docker compose run --rm --build bootstrap
docker compose up -d --build --force-recreate endpoint-addresses
docker compose up -d --build --force-recreate mqtt endpoint-manager
docker compose ps mqtt gateway endpoint-addresses endpoint-manager

echo "Printer endpoint addresses, certificates, and listeners are applied."
