#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

./scripts/init.sh
docker compose up -d --build
docker compose ps
