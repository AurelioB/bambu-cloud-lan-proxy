#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
. ./.env
host=${PROXY_BIND_IP:-127.0.0.2}
curl --fail --silent --show-error --insecure --user "bblp:${PROXY_ACCESS_CODE}" --ftp-ssl "ftps://${host}:990/" >/dev/null
echo "FTPS login and listing succeeded"
