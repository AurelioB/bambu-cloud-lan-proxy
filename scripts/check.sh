#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
cargo fmt --check
cargo test --locked
npm --prefix web ci
npm --prefix web run check
npm --prefix web run build
docker compose config --quiet
