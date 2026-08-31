#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

python_bin=${PYTHON:-}
if [ -z "$python_bin" ]; then
  if command -v python >/dev/null 2>&1; then
    python_bin=python
  else
    python_bin=python3
  fi
fi

"$python_bin" -m pytest
"$python_bin" -m ruff check src tests scripts/verify_ftps.py
docker compose config --quiet
