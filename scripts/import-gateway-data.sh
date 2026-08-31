#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_dir=${1:-}
destination="$project_dir/data/gateway"

if [ -z "$source_dir" ] || [ ! -d "$source_dir" ]; then
  echo "Usage: $0 /path/to/existing/gateway-data" >&2
  exit 2
fi

mkdir -p "$destination"
if find "$destination" -mindepth 1 -print -quit | grep -q .; then
  echo "Destination is not empty: $destination" >&2
  echo "Refusing to merge cloud credentials automatically." >&2
  exit 1
fi

cp -a "$source_dir/." "$destination/"
echo "Copied the existing Gateway session into $destination"
