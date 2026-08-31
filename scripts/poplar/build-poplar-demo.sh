#!/usr/bin/env bash
set -euo pipefail

: "${POPLAR_DATA_DIR:?set POPLAR_DATA_DIR to an external data directory}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
binary="$repo_root/target/release/pangenome-range"
input="$POPLAR_DATA_DIR/Chr19.gbz"
cache="$POPLAR_DATA_DIR/chr19-source-cache"
output="$POPLAR_DATA_DIR/poplar-chr19-named.pngr"
report="$POPLAR_DATA_DIR/poplar-chr19-named-report.json"
verification="$POPLAR_DATA_DIR/poplar-chr19-demo-verification.json"

[[ -x "$binary" ]] || {
  printf 'build the release CLI first: cargo build --release -p pangenome-range-cli\n' >&2
  exit 1
}
[[ -f "$input" ]] || {
  printf 'fetch Chr19.gbz first with scripts/poplar/fetch-poplar.sh\n' >&2
  exit 1
}
command -v systemd-run >/dev/null

run_capped() {
  systemd-run --user --wait --collect --quiet \
    --property=MemoryMax=4G \
    --property=MemorySwapMax=0 \
    --property=TasksMax=256 \
    -- "$@"
}

mkdir -p "$POPLAR_DATA_DIR/scratch"
run_capped "$binary" source-cache build "$input" "$cache"
run_capped "$binary" encode "$input" "$output" \
  --sample Nisqually-1 \
  --contig Chr19 \
  --window-size 16384 \
  --codec zstd-3 \
  --threads 2 \
  --max-queued-bytes 134217728 \
  --scratch-dir "$POPLAR_DATA_DIR/scratch" \
  --source-cache "$cache" \
  --path-membership \
  --path-locate-max-lf-steps 8192 \
  --reference-assembly 'Populus trichocarpa Nisqually-1 v4' \
  --dataset-title 'Populus trichocarpa chromosome 19 pangenome' \
  --dataset-description 'ORNL chromosome 19 Minigraph-Cactus graph for 88 haplotype-resolved assemblies from 44 diverse genotypes, anchored on Nisqually-1' \
  --source-uri 'https://labkey.ornl.gov/labkey/_webdav/CBI/Jacobson/PUBLIC_DATA/poplar_pangenome/pan_genome_graph/%40files/ptricho_pangenome/doi/Chr19/Chr19.gbz' \
  --report "$report" \
  --progress plain
run_capped "$binary" validate "$output" \
  --mode full \
  --workers 2 \
  --max-queued-bytes 536870912 \
  --progress plain
run_capped "$binary" verify "$output" \
  --against "$input" \
  --sample Nisqually-1 \
  --contig Chr19 \
  --start 6291456 \
  --end 6324224 \
  --report "$verification"

sha256sum "$input" "$output"
printf 'local-only archive built at %s; do not publish before data-use review\n' "$output"
