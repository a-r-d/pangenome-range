#!/usr/bin/env bash
set -euo pipefail

: "${CHICKEN_DATA_DIR:?set CHICKEN_DATA_DIR to an external data directory}"
: "${VG_BIN:?set VG_BIN to the verified binary produced by scripts/chicken/build-pinned-vg.sh}"

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pngr="$repo_root/target/release/pangenome-range"
vg="$VG_BIN"
compressed_gfa="$CHICKEN_DATA_DIR/pangenome.gfa.gz"
gfa="$CHICKEN_DATA_DIR/pangenome.gfa"
gbz="$CHICKEN_DATA_DIR/chicken-whole-patched-gbz-v1.gbz"
annotation_source="$CHICKEN_DATA_DIR/GCF_016699485.2_genomic.gff.gz"
assembly_report="$CHICKEN_DATA_DIR/GCF_016699485.2_assembly_report.txt"
path_names="$CHICKEN_DATA_DIR/chicken-whole-path-names.txt"
reference_contigs="$CHICKEN_DATA_DIR/chicken-whole-reference-contigs.txt"
annotations="$CHICKEN_DATA_DIR/chicken-whole-genes.gff3"
cache="$CHICKEN_DATA_DIR/chicken-whole-source-cache"
archive="$CHICKEN_DATA_DIR/chicken-whole-named.pngr"
report="$CHICKEN_DATA_DIR/chicken-whole-named-report.json"
oracle="$CHICKEN_DATA_DIR/chicken-whole-oracle.json"

graph_sha256='609230aa36071690bfde02445a7d0693f75e512a79276f60a3b5d04361c2600d'
vg_sha256='80562fb2bb1240b520a139bfb060592214c71e68846c0dd915592c07b352a763'
gbz_sha256='96c04d263e8af7cf0863cfda8a22bb5cc9b9c3aea387cdd59a05db7a7ab1ea7f'
annotations_sha256='0b9b65ba6db7cac55636d392638895faf0758dc2d0be0944347c3501c1d6ee27'
archive_sha256='93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e'

[[ -x "$pngr" ]] || {
  printf 'error: build the release CLI first: cargo build --release -p pangenome-range-cli\n' >&2
  exit 1
}
[[ -x "$vg" && -f "$compressed_gfa" && -f "$annotation_source" && -f "$assembly_report" ]] || {
  printf 'error: run scripts/chicken/fetch-chicken.sh and provide VG_BIN\n' >&2
  exit 1
}
command -v systemd-run >/dev/null

run_capped() {
  local high=$1
  local maximum=$2
  shift 2
  systemd-run --user --wait --collect --pipe --quiet \
    --property="MemoryHigh=$high" \
    --property="MemoryMax=$maximum" \
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

verify_sha256 "$compressed_gfa" "$graph_sha256"
verify_sha256 "$vg" "$vg_sha256"
if [[ ! -f "$gfa" ]]; then
  [[ ! -e "$gfa.part" ]] || { printf 'error: review incomplete %s.part\n' "$gfa" >&2; exit 1; }
  run_capped 384M 512M bash -c 'gzip -dc "$1" > "$2"' bash "$compressed_gfa" "$gfa.part"
  mv "$gfa.part" "$gfa"
fi
[[ "$(stat --format=%s "$gfa")" == 12630299872 ]]

# This is the only high-memory stage. The required vg build contains the
# GBWTGraph rvalue-move repair from patches/vg/; one job, a small buffer, and a
# zero-swap cgroup keep the full conversion bounded. Build the verified binary
# with scripts/chicken/build-pinned-vg.sh. Do not weaken the checksum gate.
if [[ ! -f "$gbz" ]]; then
  [[ ! -e "$gbz.part" ]] || { printf 'error: review incomplete %s.part\n' "$gbz" >&2; exit 1; }
  mkdir -p "$CHICKEN_DATA_DIR/vg-full-tmp"
  run_capped 40G 42G env OMP_NUM_THREADS=1 "$vg" gbwt --progress \
    --gfa-input --gbz-v1 --max-node 0 --num-jobs 1 --buffer-size 20 \
    --temp-dir "$CHICKEN_DATA_DIR/vg-full-tmp" --graph-name "$gbz.part" "$gfa"
  mv "$gbz.part" "$gbz"
fi
verify_sha256 "$gbz" "$gbz_sha256"

if [[ ! -f "$path_names" ]]; then
  run_capped 6G 8G "$vg" gbwt --path-names --gbz-input "$gbz" > "$path_names.part"
  mv "$path_names.part" "$path_names"
fi
if [[ ! -f "$reference_contigs" ]]; then
  awk -F '#' '$1 == "bGalGal1b" { print $3 }' "$path_names" | sort -u > "$reference_contigs.part"
  [[ "$(wc -l < "$reference_contigs.part")" == 207 ]]
  mv "$reference_contigs.part" "$reference_contigs"
fi
if [[ ! -f "$annotations" ]]; then
  gzip -dc "$annotation_source" | awk -f "$repo_root/scripts/chicken/map-gff-to-reference.awk" \
    "$reference_contigs" "$assembly_report" - > "$annotations.part"
  mv "$annotations.part" "$annotations"
fi
verify_sha256 "$annotations" "$annotations_sha256"

if [[ ! -d "$cache" ]]; then
  [[ ! -e "$cache.part" ]] || { printf 'error: review incomplete %s.part\n' "$cache" >&2; exit 1; }
  run_capped 1536M 2G "$pngr" source-cache build "$gbz" "$cache.part"
  mv "$cache.part" "$cache"
fi

mkdir -p "$CHICKEN_DATA_DIR/whole-scratch"
if [[ ! -f "$archive" ]]; then
  run_capped 1536M 2G "$pngr" encode "$gbz" "$archive" \
    --sample bGalGal1b --window-size 16384 --codec zstd-3 \
    --threads 1 --max-queued-bytes 67108864 \
    --scratch-dir "$CHICKEN_DATA_DIR/whole-scratch" --source-cache "$cache" \
    --path-membership --path-locate-max-lf-steps 8192 \
    --annotations "$annotations" --annotation-sample bGalGal1b \
    --annotation-feature-type gene --annotation-release 106 \
    --annotation-assembly GCF_016699485.2 \
    --reference-assembly GCF_016699485.2 \
    --dataset-title 'Chicken pangenome complete reference' \
    --dataset-description 'All 207 bGalGal1b reference paths from the complete 30-assembly Gallus gallus pangenome graph, with named source-path membership and GRCg7b gene annotations' \
    --source-uri 'https://zenodo.org/records/10018222' \
    --report "$report" --progress plain --progress-interval-seconds 30
fi
verify_sha256 "$archive" "$archive_sha256"

run_capped 1536M 2G "$pngr" validate "$archive" \
  --mode full --workers 1 --max-queued-bytes 536870912 --progress plain
run_capped 30G 32G "$pngr" verify "$archive" --against "$gbz" \
  --workload "$repo_root/results/named-membership/chicken-whole/workload.json" \
  --report "$oracle"

printf 'verified whole archive: %s\n' "$archive"
