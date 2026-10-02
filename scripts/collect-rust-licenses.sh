#!/bin/sh
# Run after cargo build inside the Rust builder; never installed as a service.
set -eu
output=$1
registry_dir=${CARGO_HOME:-/usr/local/cargo}/registry/src
mkdir -p "$output"
for notice in "$registry_dir"/*/*/LICENSE* "$registry_dir"/*/*/LICENCE* \
  "$registry_dir"/*/*/COPYING* "$registry_dir"/*/*/NOTICE*; do
  test -f "$notice" || continue
  crate=$(basename "$(dirname "$notice")")
  mkdir -p "$output/$crate"
  cp "$notice" "$output/$crate/"
done
cp Cargo.toml Cargo.lock "$output/"
