#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

if [ ! -s .env ]; then
  echo "Missing .env; run ./scripts/init.sh first." >&2
  exit 1
fi

exec python3 "$project_dir/scripts/verify_ftps.py" "$@"
