#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
cargo fmt --check
npm --prefix web ci
npm --prefix web run check
npm --prefix web run build
cargo test --locked
docker compose config --quiet
