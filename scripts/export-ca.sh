#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_file="$project_dir/data/proxy/export/bambu-cloud-lan-proxy-ca.crt"
destination=${1:-"$project_dir/bambu-cloud-lan-proxy-ca.crt"}

if [ ! -s "$source_file" ]; then
  echo "No generated CA found. Run ./scripts/init.sh first." >&2
  exit 1
fi

cp "$source_file" "$destination"
chmod 644 "$destination"
echo "Exported public CA to $destination"
