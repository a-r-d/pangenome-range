#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
temporary=""

cleanup() {
    if [ -n "$temporary" ]; then
        rm -f "$temporary"
    fi
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

fetch_fixture() {
    fixture_name=$1
    destination=$2
    expected_bytes=$3
    expected_sha256=$4
    url=$5
    temporary="$destination.part.$$"

    if [ -f "$destination" ]; then
        actual_sha256=$(checksum "$destination")
        if [ "$actual_sha256" = "$expected_sha256" ]; then
            echo "Already verified $fixture_name: $destination"
            temporary=""
            return
        fi
        echo "error: existing $fixture_name fixture has unexpected SHA-256: $actual_sha256" >&2
        echo "remove it explicitly before fetching a replacement" >&2
        exit 1
    fi

    curl --fail --location --proto '=https' --tlsv1.2 "$url" --output "$temporary"
    actual_bytes=$(wc -c < "$temporary" | tr -d ' ')
    if [ "$actual_bytes" != "$expected_bytes" ]; then
        echo "error: $fixture_name fixture size mismatch: $actual_bytes bytes" >&2
        exit 1
    fi
    actual_sha256=$(checksum "$temporary")
    if [ "$actual_sha256" != "$expected_sha256" ]; then
        echo "error: $fixture_name fixture SHA-256 mismatch: $actual_sha256" >&2
        exit 1
    fi

    mv "$temporary" "$destination"
    temporary=""
    echo "Fetched and verified $fixture_name: $destination ($actual_bytes bytes)"
}

fetch_tiny() {
    gbz_base_commit="a5ed1ff3ddc402e230d1187afa438e05c8b3654e"
    fetch_fixture \
        "tiny MICB/KIR3DL1" \
        "$repository_dir/test-data/micb-kir3dl1.gbz" \
        "73920" \
        "1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788" \
        "https://raw.githubusercontent.com/jltsiren/gbz-base/$gbz_base_commit/test-data/micb-kir3dl1.gbz"
}

fetch_mhc() {
    vg_snakemake_commit="d938c2035fa5ce16acd69147743762b513292173"
    fetch_fixture \
        "medium MHC" \
        "$repository_dir/test-data/mhc-10.gbz" \
        "4511832" \
        "a0b44236852d5659202a6855308020df05efd7c2be90645d341d94fb775df685" \
        "https://raw.githubusercontent.com/vgteam/vg_snakemake/$vg_snakemake_commit/testdata/mhc.gbz"
}

if [ "$#" -gt 1 ]; then
    echo "usage: $0 [tiny|mhc|all]" >&2
    exit 2
fi

case "${1:-tiny}" in
    tiny)
        fetch_tiny
        ;;
    mhc)
        fetch_mhc
        ;;
    all)
        fetch_tiny
        fetch_mhc
        ;;
    *)
        echo "usage: $0 [tiny|mhc|all]" >&2
        exit 2
        ;;
esac

trap - EXIT HUP INT TERM
