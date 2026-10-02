#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

if [ ! -f .env ]; then
  echo "Run ./scripts/init.sh first" >&2
  exit 1
fi

if [ -f release.env ]; then
  ./scripts/compose.sh run --rm --no-build bootstrap
  ./scripts/compose.sh up -d --no-build --force-recreate endpoint-addresses
  ./scripts/compose.sh up -d --no-build --force-recreate mqtt endpoint-manager
else
  ./scripts/compose.sh run --rm --build bootstrap
  ./scripts/compose.sh up -d --build --force-recreate endpoint-addresses
  ./scripts/compose.sh up -d --build --force-recreate mqtt endpoint-manager
fi
./scripts/compose.sh ps mqtt gateway endpoint-addresses endpoint-manager

echo "Printer endpoint addresses, certificates, and listeners are applied."
