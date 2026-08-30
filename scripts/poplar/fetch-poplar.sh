#!/usr/bin/env bash
set -euo pipefail

: "${POPLAR_DATA_DIR:?set POPLAR_DATA_DIR to an external data directory}"

mkdir -p "$POPLAR_DATA_DIR"

fetch_checked() {
  local name="$1"
  local expected_bytes="$2"
  local expected_sha256="$3"
  local url="$4"
  local output="$POPLAR_DATA_DIR/$name"
  local partial="$output.part"

  if [[ -f "$output" ]] &&
    [[ "$(stat -c %s "$output")" == "$expected_bytes" ]] &&
    [[ "$(sha256sum "$output" | cut -d ' ' -f 1)" == "$expected_sha256" ]]; then
    printf '%s is already checksum-verified\n' "$output"
    return
  fi

  curl --fail --location --retry 3 --output "$partial" "$url"
  [[ "$(stat -c %s "$partial")" == "$expected_bytes" ]]
  printf '%s  %s\n' "$expected_sha256" "$partial" | sha256sum --check --status
  mv "$partial" "$output"
  printf 'verified %s\n' "$output"
}

fetch_checked \
  Chr16.gbz \
  81167536 \
  1d2c0d2d734e0bca0c05b7d880df498e86dcdcee8213644dafc549fcbc159dac \
  'https://labkey.ornl.gov/labkey/_webdav/CBI/Jacobson/PUBLIC_DATA/poplar_pangenome/pan_genome_graph/%40files/ptricho_pangenome/doi/Chr16/Chr16.gbz?contentDisposition=attachment'

fetch_checked \
  Chr19.gbz \
  125422184 \
  603ccf3a00d589950be95bb7eb65d0b517f837b35b64d6c95674bcff754adab9 \
  'https://labkey.ornl.gov/labkey/_webdav/CBI/Jacobson/PUBLIC_DATA/poplar_pangenome/pan_genome_graph/%40files/ptricho_pangenome/doi/Chr19/Chr19.gbz?contentDisposition=attachment'
