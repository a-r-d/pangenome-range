#!/usr/bin/env bash
set -euo pipefail

: "${CHICKEN_DATA_DIR:?set CHICKEN_DATA_DIR to an external data directory}"

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pngr="$repo_root/target/release/pangenome-range"
node_binary=$(command -v node)
vg="$CHICKEN_DATA_DIR/tools/vg-v1.76.1"
compressed_gfa="$CHICKEN_DATA_DIR/pangenome.gfa.gz"
gfa="$CHICKEN_DATA_DIR/pangenome.gfa"
component="$CHICKEN_DATA_DIR/chicken-chr15.gfa"
extractor="$CHICKEN_DATA_DIR/tools/extract-gfa-component"
gbz="$CHICKEN_DATA_DIR/chicken-chr15-gbz-v1.gbz"
annotation_source="$CHICKEN_DATA_DIR/GCF_016699485.2_genomic.gff.gz"
annotations="$CHICKEN_DATA_DIR/chicken-chr15-genes.gff3"
cache="$CHICKEN_DATA_DIR/chicken-chr15-source-cache"
archive="$CHICKEN_DATA_DIR/chicken-chr15-named.pngr"
report="$CHICKEN_DATA_DIR/chicken-chr15-named-report.json"
oracle="$CHICKEN_DATA_DIR/chicken-chr15-oracle-probe.json"

graph_sha256='609230aa36071690bfde02445a7d0693f75e512a79276f60a3b5d04361c2600d'
component_sha256='8d3ae4d2d6091e4ceaa3e9e973dfbf97baa79238d4360c15f6417e725cbba87b'
gbz_sha256='3c9f21afa2d57909a3bd8b0d48c947e770faab514223dbbb2a0869717ecf0cf7'
annotations_sha256='3ab30066097b88a9c31f4f24c64f2b875903d6016508db2a91bf09f1e91d87f9'
archive_sha256='fcb19b2c6e850c16e7e831613f34d27feef477331064cde5b16137492e6d1b43'

[[ -x "$pngr" ]] || {
  printf 'error: build the release CLI first: cargo build --release -p pangenome-range-cli\n' >&2
  exit 1
}
[[ -x "$vg" && -f "$compressed_gfa" && -f "$annotation_source" ]] || {
  printf 'error: run scripts/chicken/fetch-chicken.sh first\n' >&2
  exit 1
}
command -v systemd-run >/dev/null

run_capped() {
  local memory=$1
  shift
  systemd-run --user --wait --collect --pipe --quiet \
    --property="MemoryMax=$memory" \
    --property=MemorySwapMax=0 \
    --property=TasksMax=128 \
    -- "$@"
}

verify_sha256() {
  local path=$1
  local expected=$2
  [[ "$(sha256sum "$path" | awk '{print $1}')" == "$expected" ]] || {
    printf 'error: checksum mismatch for %s\n' "$path" >&2
    exit 1
  }
}

# The source checksum gates the dataset-specific component bounds below. This
# bounded chromosome recipe intentionally does not run stock vg on the complete
# GFA. The separate whole-corpus recipe requires the measured move-fixed vg
# binary and enforces a 42 GiB, zero-swap ceiling.
verify_sha256 "$compressed_gfa" "$graph_sha256"
if [[ ! -f "$gfa" ]]; then
  [[ ! -e "$gfa.part" ]] || { printf 'error: review incomplete %s.part\n' "$gfa" >&2; exit 1; }
  run_capped 512M bash -c 'gzip -dc "$1" > "$2"' bash "$compressed_gfa" "$gfa.part"
  mv "$gfa.part" "$gfa"
fi
[[ "$(stat --format=%s "$gfa")" == 12630299872 ]]

if [[ ! -x "$extractor" ]]; then
  run_capped 512M c++ -std=c++20 -O2 -Wall -Wextra -Werror \
    "$repo_root/scripts/chicken/extract-gfa-component.cpp" -o "$extractor.part"
  mv "$extractor.part" "$extractor"
fi
if [[ ! -f "$component" ]]; then
  [[ ! -e "$component.part" ]] || { printf 'error: review incomplete %s.part\n' "$component" >&2; exit 1; }
  run_capped 384M "$extractor" "$gfa" "$component" 14724903 15245596
fi
verify_sha256 "$component" "$component_sha256"

if [[ ! -f "$gbz" ]]; then
  [[ ! -e "$gbz.part" ]] || { printf 'error: review incomplete %s.part\n' "$gbz" >&2; exit 1; }
  mkdir -p "$CHICKEN_DATA_DIR/vg-tmp"
  run_capped 2G "$vg" gbwt --gfa-input --gbz-v1 --max-node 0 \
    --num-jobs 1 --buffer-size 20 --temp-dir "$CHICKEN_DATA_DIR/vg-tmp" \
    --graph-name "$gbz.part" "$component"
  mv "$gbz.part" "$gbz"
fi
verify_sha256 "$gbz" "$gbz_sha256"

if [[ ! -f "$annotations" ]]; then
  [[ ! -e "$annotations.part" ]] || { printf 'error: review incomplete %s.part\n' "$annotations" >&2; exit 1; }
  run_capped 256M bash -c \
    'printf "%s\n" "##gff-version 3" "##sequence-region chr15 1 12703657" > "$2"; gzip -dc "$1" | awk -F "\t" '\''BEGIN { OFS="\t" } $1 == "NC_052546.1" && $3 == "gene" { $1="chr15"; print }'\'' >> "$2"' \
    bash "$annotation_source" "$annotations.part"
  mv "$annotations.part" "$annotations"
fi
verify_sha256 "$annotations" "$annotations_sha256"

mkdir -p "$CHICKEN_DATA_DIR/scratch"
if [[ ! -d "$cache" ]]; then
  run_capped 1G "$pngr" source-cache build "$gbz" "$cache"
fi
if [[ ! -f "$archive" ]]; then
  run_capped 1G "$pngr" encode "$gbz" "$archive" \
    --sample bGalGal1b --contig chr15 \
    --window-size 16384 --codec zstd-3 --threads 1 --max-queued-bytes 67108864 \
    --scratch-dir "$CHICKEN_DATA_DIR/scratch" --source-cache "$cache" \
    --path-membership --path-locate-max-lf-steps 8192 \
    --annotations "$annotations" --annotation-sample bGalGal1b \
    --annotation-feature-type gene --annotation-release 106 \
    --annotation-assembly GCF_016699485.2 \
    --reference-assembly GCF_016699485.2 \
    --dataset-title 'Chicken pangenome chromosome 15' \
    --dataset-description 'Chromosome 15 component from the 30-haplotype Gallus gallus pangenome graph published by Rice et al., including named source-path membership and GRCg7b gene annotations' \
    --source-uri 'https://zenodo.org/records/10018222' \
    --report "$report" --progress plain
fi
verify_sha256 "$archive" "$archive_sha256"

run_capped 1G "$pngr" validate "$archive" \
  --mode full --workers 1 --max-queued-bytes 536870912 --progress plain
if [[ -f "$oracle" ]]; then
  run_capped 1G "$pngr" verify "$archive" --against "$gbz" \
    --workload "$repo_root/results/named-membership/chicken/workload.json"
else
  run_capped 1G "$pngr" verify "$archive" --against "$gbz" \
    --workload "$repo_root/results/named-membership/chicken/workload.json" \
    --report "$oracle"
fi
run_capped 512M "$node_binary" "$repo_root/scripts/chicken/query-chicken.mjs" "$archive"

printf 'verified archive: %s\n' "$archive"
