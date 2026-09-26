#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
access_code=$(sed -n 's/^PROXY_ACCESS_CODE=//p' .env | tail -n 1)
host=$(sed -n 's/^PROXY_BIND_IP=//p' .env | tail -n 1)
host=${host:-127.0.0.2}
if [ -z "$access_code" ]; then
  echo "Missing PROXY_ACCESS_CODE; run ./scripts/init.sh first." >&2
  exit 1
fi
curl --fail --silent --show-error --insecure --user "bblp:${access_code}" --ftp-ssl "ftps://${host}:990/" >/dev/null
echo "FTPS login and listing succeeded"
