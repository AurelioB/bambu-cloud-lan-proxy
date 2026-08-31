#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"

python -m pytest
python -m ruff check src tests
docker compose config --quiet
