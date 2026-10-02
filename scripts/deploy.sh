#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

./scripts/init.sh
if [ -f release.env ]; then
  ./scripts/compose.sh up -d --no-build
else
  ./scripts/compose.sh up -d --build
fi
./scripts/compose.sh ps
