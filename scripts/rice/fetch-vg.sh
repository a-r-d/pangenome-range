#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
data_dir=${PPANG_RICE_DATA_DIR:-"$repo_root/data/rice"}
version=v1.76.1
url="https://github.com/vgteam/vg/releases/download/$version/vg"
expected_bytes=55374080
expected_sha256=87b457fdda6801c9580f79a53c3c0aa502261420abf222920c7222a703fd856b
destination="$data_dir/tools/vg-$version"
part="$destination.part"
mkdir -p "$(dirname "$destination")"

if [[ ! -f "$destination" ]]; then
  curl --fail --location --show-error --output "$part" "$url"
  actual_bytes=$(stat --format=%s "$part")
  actual_sha256=$(sha256sum "$part" | awk '{print $1}')
  [[ "$actual_bytes" == "$expected_bytes" ]] || {
    printf 'error: vg bytes %s != expected %s\n' "$actual_bytes" "$expected_bytes" >&2
    exit 1
  }
  [[ "$actual_sha256" == "$expected_sha256" ]] || {
    printf 'error: vg sha256 %s != expected %s\n' "$actual_sha256" "$expected_sha256" >&2
    exit 1
  }
  mv "$part" "$destination"
  chmod +x "$destination"
fi

[[ "$(stat --format=%s "$destination")" == "$expected_bytes" ]]
[[ "$(sha256sum "$destination" | awk '{print $1}')" == "$expected_sha256" ]]
"$destination" version
