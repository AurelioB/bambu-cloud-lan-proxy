#!/bin/sh
# Fetch reviewed, checksum-pinned audit tools without compiling them locally.
set -eu
case "${1:-}" in
  cargo-audit)
    archive=cargo-audit-x86_64-unknown-linux-gnu-v0.22.2.tgz
    url=https://github.com/RustSec/rustsec/releases/download/cargo-audit%2Fv0.22.2/$archive
    checksum=ab28a1bdb54db4d5d8ad5981cf1f959410370b3d28250dbd35f6a44248620e39
    executable=cargo-audit-x86_64-unknown-linux-gnu-v0.22.2/cargo-audit
    ;;
  gitleaks)
    archive=gitleaks_8.30.1_linux_x64.tar.gz
    url=https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/$archive
    checksum=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
    executable=gitleaks
    ;;
  actionlint)
    archive=actionlint_1.7.12_linux_amd64.tar.gz
    url=https://github.com/rhysd/actionlint/releases/download/v1.7.12/$archive
    checksum=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8
    executable=actionlint
    ;;
  *) echo 'Usage: audit-tool.sh gitleaks|actionlint|cargo-audit' >&2; exit 2 ;;
esac
test "$(uname -sm)" = 'Linux x86_64' || {
  echo 'These audit tool pins are for Linux x86-64 only.' >&2; exit 2;
}
tool_dir=$(mktemp -d "${TMPDIR:-/tmp}/bambu-audit-tool.XXXXXX")
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
  "$url" --output "$tool_dir/$archive"
printf '%s  %s\n' "$checksum" "$tool_dir/$archive" | sha256sum --check >&2
tar -xzf "$tool_dir/$archive" -C "$tool_dir" "$executable"
printf '%s\n' "$tool_dir/$executable"
