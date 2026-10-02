#!/bin/sh
# Offline tests of the actual packaged executables and deployment configuration.
set -eu
artifacts=$1
image=$2
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -1)
artifacts=$(CDPATH= cd -- "$artifacts" && pwd)
(cd "$artifacts" && sha256sum --check SHA256SUMS)
unpack_dir=$(mktemp -d "${TMPDIR:-/tmp}/bambu-release-smoke.XXXXXX")
for archive in "$artifacts"/*.tar.gz; do
  if tar -tzf "$archive" | grep -Eq '(^|/)(data|\.git|node_modules)/|(^|/)\.env$|libbambu_networking\.so|libBambuSource\.so'; then
    echo 'Unexpected private state or proprietary plugin in release archive.' >&2; exit 1
  fi
done
tar -xzf "$artifacts/bambu-cloud-lan-proxy-$version-linux-x86_64.tar.gz" -C "$unpack_dir"
tar -xzf "$artifacts/bambu-cloud-lan-proxy-$version-compose.tar.gz" -C "$unpack_dir"
native_dir=$unpack_dir/bambu-cloud-lan-proxy-$version-linux-x86_64
test "$(docker run --rm --network none --entrypoint /bundle/bin/bambu-cloud-lan-proxy \
  -v "$native_dir:/bundle:ro" "$image" --version)" = "bambu-cloud-lan-proxy $version"
printf '%s\n' '{"id":1,"method":"echo","params":{"offline":true}}' \
  | docker run --rm -i --network none --entrypoint /bundle/bin/bambu-proxy-native \
    -v "$native_dir:/bundle:ro" "$image" \
  | jq -e '.id == 1 and .result.offline == true' >/dev/null
docker run --rm --network none --entrypoint /bundle/bin/mediamtx \
  -v "$native_dir:/bundle:ro" "$image" --version
deploy_dir=$unpack_dir/bambu-cloud-lan-proxy-$version-compose
cp "$deploy_dir/.env.example" "$deploy_dir/.env"
chmod 600 "$deploy_dir/.env"
"$deploy_dir/scripts/compose.sh" --profile legacy-single-endpoint config --format json \
  | jq -e 'all(.services[]; .build == null)' >/dev/null
for script in "$deploy_dir/scripts/"*.sh; do sh -n "$script"; done
if ./scripts/package-release.sh "$image" ghcr.io/example/proxy "$artifacts"; then
  echo 'Packaging unexpectedly overwrote an existing output directory.' >&2; exit 1
fi
echo 'Native executables, immutable checksums, source exclusions, and prebuilt Compose configuration passed.'
