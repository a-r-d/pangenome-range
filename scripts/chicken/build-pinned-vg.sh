#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
vg_commit='32cadf3d3ee45d04c532767158c7dee6243f5713'
gbwtgraph_commit='7fac0af212b0502d40cca3d2b2c3a5fd7d85c540'
expected_binary_sha256='80562fb2bb1240b520a139bfb060592214c71e68846c0dd915592c07b352a763'
patch_path="$repo_root/patches/vg/gbwtgraph-move-gbwt-index.patch"
repro_bin="$repo_root/scripts/chicken/repro-bin"

usage() {
  printf 'usage: %s OUTPUT_DIR [--jobs N] [--check-only VG_CHECKOUT]\n' "$0" >&2
}

[[ $# -ge 1 ]] || { usage; exit 2; }
output_dir=$1
shift
jobs=8
check_checkout=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --jobs)
      jobs=${2:?missing job count}
      shift 2
      ;;
    --check-only)
      check_checkout=${2:?missing checkout}
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

verify_checkout() {
  local checkout=$1
  [[ "$(git -C "$checkout" rev-parse HEAD)" == "$vg_commit" ]] || {
    printf 'error: vg checkout is not pinned commit %s\n' "$vg_commit" >&2
    exit 1
  }
  [[ "$(git -C "$checkout/deps/gbwtgraph" rev-parse HEAD)" == "$gbwtgraph_commit" ]] || {
    printf 'error: gbwtgraph submodule is not pinned commit %s\n' "$gbwtgraph_commit" >&2
    exit 1
  }
  if git -C "$checkout/deps/gbwtgraph" apply --check "$patch_path"; then
    printf 'clean\n'
  elif git -C "$checkout/deps/gbwtgraph" apply --reverse --check "$patch_path"; then
    printf 'patched\n'
  else
    printf 'error: local patch neither applies nor matches %s\n' "$checkout" >&2
    exit 1
  fi
}

if [[ -n "$check_checkout" ]]; then
  patch_state=$(verify_checkout "$check_checkout")
  [[ "$patch_state" == clean ]] || {
    printf 'error: --check-only requires an unpatched checkout\n' >&2
    exit 1
  }
  printf 'patch applies to vg %s / gbwtgraph %s\n' "$vg_commit" "$gbwtgraph_commit"
  exit 0
fi

command -v git >/dev/null
command -v make >/dev/null
command -v gcc >/dev/null
command -v g++ >/dev/null
command -v jq >/dev/null
[[ "$(g++ --version | head -n 1)" == 'g++ (Ubuntu 15.2.0-4ubuntu4) 15.2.0' ]] || {
  printf 'error: the measured binary requires g++ (Ubuntu 15.2.0-4ubuntu4) 15.2.0\n' >&2
  exit 1
}
mkdir -p "$output_dir"
source_dir="$output_dir/vg-source"
manifest="$output_dir/vg-tool-manifest.json"

if [[ ! -d "$source_dir/.git" ]]; then
  git clone --filter=blob:none https://github.com/vgteam/vg.git "$source_dir"
fi
git -C "$source_dir" fetch origin "$vg_commit"
git -C "$source_dir" checkout --detach "$vg_commit"
git -C "$source_dir" submodule update --init --recursive
patch_state=$(verify_checkout "$source_dir")
if [[ "$patch_state" == clean ]]; then
  git -C "$source_dir/deps/gbwtgraph" apply "$patch_path"
fi

# vg embeds whoami/hostname in its binary. The two PATH shims make those inputs
# deterministic and reproduce the measured build identity.
PATH="$repro_bin:$PATH" make -C "$source_dir" -j "$jobs"
binary="$source_dir/bin/vg"
actual_sha256=$(sha256sum "$binary" | awk '{print $1}')
binary_bytes=$(stat -c '%s' "$binary")
[[ "$actual_sha256" == "$expected_binary_sha256" ]] || {
  printf 'error: built vg checksum %s does not match the measured binary %s\n' \
    "$actual_sha256" "$expected_binary_sha256" >&2
  exit 1
}

compiler=$(g++ --version | head -n 1)
patch_sha256=$(sha256sum "$patch_path" | awk '{print $1}')
whoami_sha256=$(sha256sum "$repro_bin/whoami" | awk '{print $1}')
hostname_sha256=$(sha256sum "$repro_bin/hostname" | awk '{print $1}')
version=$($binary version | head -n 1)
jq -n \
  --arg schema_version '1' \
  --arg vg_repository 'https://github.com/vgteam/vg.git' \
  --arg vg_commit "$vg_commit" \
  --arg gbwtgraph_repository 'https://github.com/jltsiren/gbwtgraph.git' \
  --arg gbwtgraph_commit "$gbwtgraph_commit" \
  --arg patch 'patches/vg/gbwtgraph-move-gbwt-index.patch' \
  --arg patch_sha256 "$patch_sha256" \
  --arg patch_reason 'Move the GBWT rvalue into GBZ instead of copying it during GFA-to-GBZ construction.' \
  --arg whoami_sha256 "$whoami_sha256" \
  --arg hostname_sha256 "$hostname_sha256" \
  --arg compiler "$compiler" \
  --arg build_command "PATH=scripts/chicken/repro-bin:\$PATH make -j $jobs" \
  --arg version "$version" \
  --argjson binary_bytes "$binary_bytes" \
  --arg binary_sha256 "$actual_sha256" \
  '{
    schemaVersion: ($schema_version | tonumber),
    vg: {repository: $vg_repository, commit: $vg_commit, upstream: true},
    gbwtgraph: {repository: $gbwtgraph_repository, commit: $gbwtgraph_commit},
    localPatch: {path: $patch, sha256: $patch_sha256, reason: $patch_reason},
    reproducibilityShims: {
      whoami: {path: "scripts/chicken/repro-bin/whoami", sha256: $whoami_sha256},
      hostname: {path: "scripts/chicken/repro-bin/hostname", sha256: $hostname_sha256}
    },
    compiler: $compiler,
    buildMode: "default optimized make build",
    buildCommand: $build_command,
    version: $version,
    binaryBytes: $binary_bytes,
    binarySha256: $binary_sha256,
    patchApplyCheck: {
      status: "passed",
      command: "git -C CLEAN_GBWTGRAPH_CHECKOUT apply --check patches/vg/gbwtgraph-move-gbwt-index.patch"
    }
  }' > "$manifest"

printf 'verified vg binary: %s\nmanifest: %s\n' "$binary" "$manifest"
