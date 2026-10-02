#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
if [ -f release.env ]; then
  # Keep operators' .env secrets separate from versioned image references.
  exec docker compose --env-file .env --env-file release.env \
    -f compose.yaml -f compose.release.yaml "$@"
fi
exec docker compose "$@"
