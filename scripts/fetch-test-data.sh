#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
destination="$repository_dir/test-data/micb-kir3dl1.gbz"
temporary="$destination.part.$$"

gbz_base_commit="a5ed1ff3ddc402e230d1187afa438e05c8b3654e"
expected_sha256="1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788"
url="https://raw.githubusercontent.com/jltsiren/gbz-base/$gbz_base_commit/test-data/micb-kir3dl1.gbz"

cleanup() {
    rm -f "$temporary"
}
trap cleanup EXIT HUP INT TERM

checksum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "error: sha256sum or shasum is required" >&2
        exit 1
    fi
}

if [ -f "$destination" ]; then
    actual_sha256=$(checksum "$destination")
    if [ "$actual_sha256" = "$expected_sha256" ]; then
        echo "Already verified: $destination"
        exit 0
    fi
    echo "error: existing fixture has unexpected SHA-256: $actual_sha256" >&2
    echo "remove it explicitly before fetching a replacement" >&2
    exit 1
fi

curl --fail --location --proto '=https' --tlsv1.2 "$url" --output "$temporary"
actual_sha256=$(checksum "$temporary")
if [ "$actual_sha256" != "$expected_sha256" ]; then
    echo "error: fixture SHA-256 mismatch: $actual_sha256" >&2
    exit 1
fi

mv "$temporary" "$destination"
trap - EXIT HUP INT TERM
echo "Fetched and verified: $destination"

