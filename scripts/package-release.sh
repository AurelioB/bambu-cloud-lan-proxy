#!/bin/sh
# Package only a clean source revision and explicitly selected image binaries.
# Does not download the Bambu plugin, connect to printers, or publish anything.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
if [ "$#" -ne 3 ]; then
  echo 'Usage: package-release.sh LOCAL_IMAGE ghcr.io/owner/project NEW_OUTPUT_DIR' >&2
  exit 2
fi
image=$1
registry=$2
output=$3
printf '%s\n' "$registry" | grep -Eq '^ghcr\.io/[a-z0-9._-]+/[a-z0-9._-]+$' || {
  echo 'Registry must be a lowercase GHCR owner/project reference.' >&2; exit 2;
}
git diff --quiet && git diff --cached --quiet || {
  echo 'Commit tracked changes before packaging a release.' >&2; exit 2;
}
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -n 1)
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.-]+)?$'
test "$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")" = linux/amd64
test "$(docker run --rm --network none "$image" --version)" = "bambu-cloud-lan-proxy $version"
# Refuse an existing target, rather than risking previous artifacts or user data.
mkdir "$output"
output=$(CDPATH= cd -- "$output" && pwd)
package_dir=$(mktemp -d "${TMPDIR:-/tmp}/bambu-release-package.XXXXXX")
native_name=bambu-cloud-lan-proxy-$version-linux-x86_64
deploy_name=bambu-cloud-lan-proxy-$version-compose
mkdir "$package_dir/$native_name" "$package_dir/$native_name/bin"
container=$(docker create --network none "$image" --version)
trap 'docker rm "$container" >/dev/null' EXIT HUP INT TERM
for binary in bambu-cloud-lan-proxy bambu-proxy-native mediamtx; do
  docker cp "$container:/usr/local/bin/$binary" "$package_dir/$native_name/bin/$binary"
done
docker cp "$container:/usr/share/licenses" "$package_dir/$native_name/licenses"
git archive HEAD examples/standalone docker/bootstrap.sh docker/proxy.acl \
  LICENSE THIRD_PARTY_NOTICES docs/standalone-binary.md \
  | tar -x -C "$package_dir/$native_name"
git archive --prefix="$deploy_name/" HEAD compose.yaml compose.release.yaml \
  .env.example scripts/init.sh scripts/deploy.sh scripts/compose.sh \
  scripts/apply-endpoints.sh scripts/export-ca.sh LICENSE THIRD_PARTY_NOTICES \
  SECURITY.md README.md CHANGELOG.md docs examples/standalone \
  website/README.md website/public/openapi.json website/src/content/docs \
  | tar -x -C "$package_dir"
printf 'PROXY_IMAGE=%s:v%s\nPROXY_BOOTSTRAP_IMAGE=%s-bootstrap:v%s\nPROXY_MQTT_IMAGE=%s-mqtt:v%s\n' \
  "$registry" "$version" "$registry" "$version" "$registry" "$version" \
  > "$package_dir/$deploy_name/release.env"
# Rendering validates the merged configuration without using a real local .env.
env -i PATH="$PATH" docker compose --env-file "$project_dir/.env.example" \
  --env-file "$package_dir/$deploy_name/release.env" \
  -f "$package_dir/$deploy_name/compose.yaml" \
  -f "$package_dir/$deploy_name/compose.release.yaml" \
  --profile legacy-single-endpoint config --format json \
  | jq -e 'all(.services[]; (.build == null) and (.image | startswith("ghcr.io/")))' >/dev/null
tar -czf "$output/$native_name.tar.gz" -C "$package_dir" "$native_name"
tar -czf "$output/$deploy_name.tar.gz" -C "$package_dir" "$deploy_name"
git archive --format=tar.gz --prefix="bambu-cloud-lan-proxy-$version-source/" HEAD \
  > "$output/bambu-cloud-lan-proxy-$version-source.tar.gz"
jq -n --arg version "$version" --arg revision "$(git rev-parse HEAD)" \
  --arg image_id "$(docker image inspect --format '{{.Id}}' "$image")" \
  '{version:$version, revision:$revision, platform:"linux/amd64", image_id:$image_id,
    proprietary_plugin_bundled:false, hardware_validation:"H2C on v0.6.0; A1/P1 unverified"}' \
  > "$output/build.json"
(cd "$output" && sha256sum ./*.tar.gz build.json > SHA256SUMS)
printf 'Prepared %s in %s. Nothing was published.\n' "$version" "$output"
