#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
data_dir=${PPANG_RICE_DATA_DIR:-"$repo_root/data/rice"}
metadata_dir="$repo_root/data/rice"
results_dir=${PPANG_RICE_RESULTS_DIR:-"$repo_root/results/rice-acquisition"}
vg=${PPANG_RICE_VG:-"$data_dir/tools/vg-v1.76.1"}
threads=${PPANG_RICE_THREADS:-8}
gbwt_buffer_millions=${PPANG_RICE_GBWT_BUFFER_MILLIONS:-20}
cargo_target_dir=${CARGO_TARGET_DIR:-"$data_dir/cargo-target"}
xg="$data_dir/chr06_mc.xg"
gfa="$data_dir/chr06_mc.paths.gfa"
gbz="$data_dir/chr06_mc.named.gbz"
gbwt="$data_dir/chr06_mc.named.gbwt"
rindex="$data_dir/chr06_mc.named.ri"
archive="$data_dir/rice-chr06-mc-xa7-anonymous.pngr"
paths="$results_dir/chr06_mc.xg.paths.txt"
split_dir="$data_dir/tmp/gfa-split"
accession_gfa="$split_dir/chr06_mc.accessions.gfa"
internal_gfa="$split_dir/chr06_mc.minigraph-internal.gfa"
component_dir="$data_dir/tmp/gbwt-components"
accession_gbwt="$component_dir/chr06_mc.accessions.gbwt"
accession_refs_gbwt="$component_dir/chr06_mc.accessions.refs.gbwt"
internal_gbwt="$component_dir/chr06_mc.minigraph-internal.gbwt"

mkdir -p "$results_dir" "$data_dir/tmp/system" "$cargo_target_dir"
export TMPDIR="$data_dir/tmp/system"
[[ -x "$vg" ]] || { printf 'error: vg not found at %s\n' "$vg" >&2; exit 1; }
[[ -f "$xg" ]] || { printf 'error: run scripts/rice/fetch-ppang-rice.sh first\n' >&2; exit 1; }

run_measured() {
  local name=$1
  shift
  python3 "$repo_root/scripts/rice/run-measured.py" \
    --json "$results_dir/$name.measurement.json" \
    --stdout "$results_dir/$name.stdout.txt" \
    --stderr "$results_dir/$name.stderr.txt" \
    --temp-dir "$data_dir/tmp/$name" -- "$@"
}

# Audit the official XG before conversion.
"$vg" stats -F "$xg" > "$results_dir/xg-stats-format.txt"
"$vg" stats -z "$xg" > "$results_dir/xg-stats-size.txt"
"$vg" stats -N -E -l -r "$xg" > "$results_dir/xg-graph-stats.txt"
"$vg" paths -L -x "$xg" > "$paths"
"$vg" paths -M -x "$xg" > "$results_dir/xg-path-metadata.txt"
python3 "$repo_root/scripts/rice/audit-rice-paths.py" xg \
  --names "$paths" --tsv "$results_dir/xg-paths.tsv" --json "$results_dir/xg-audit.json" \
  --format "$results_dir/xg-stats-format.txt" --size "$results_dir/xg-stats-size.txt" \
  --graph "$xg"

# Export P-lines while preserving original numeric node IDs and path names.
if [[ ! -f "$gfa" ]]; then
  gfa_part="$gfa.part"
  [[ ! -e "$gfa_part" ]] || { printf 'error: remove or review incomplete %s\n' "$gfa_part" >&2; exit 1; }
  python3 "$repo_root/scripts/rice/run-measured.py" \
    --json "$results_dir/gfa-export.measurement.json" \
    --stdout "$gfa_part" --stderr "$results_dir/gfa-export.stderr.txt" \
    --temp-dir "$data_dir/tmp/gfa-export" -- "$vg" convert -fW "$xg"
  python3 "$repo_root/scripts/rice/audit-rice-paths.py" gfa \
    --gfa "$gfa_part" --names "$paths" --json "$results_dir/gfa-audit.json"
  mv "$gfa_part" "$gfa"
fi
awk -F '\t' '$1 ~ /^(H|P|W|S|L)$/ { print; count++; if (count == 20) exit }' \
  "$gfa" > "$results_dir/gfa-head.txt"

# Preserve the two observed source path classes with their original semantics:
# accession paths become structured sample/contig metadata, while
# _MINIGRAPH_.s<number> paths remain verbatim generic paths.
if [[ ! -f "$accession_gfa" || ! -f "$internal_gfa" ]]; then
  mkdir -p "$split_dir"
  accession_part="$accession_gfa.part"
  internal_part="$internal_gfa.part"
  [[ ! -e "$accession_part" && ! -e "$internal_part" ]] || {
    printf 'error: review incomplete split GFA files in %s\n' "$split_dir" >&2
    exit 1
  }
  run_measured gfa-split python3 "$repo_root/scripts/rice/split-rice-gfa.py" \
    --input "$gfa" --accession-output "$accession_part" --internal-output "$internal_part" \
    --json "$results_dir/gfa-split.json"
  mv "$accession_part" "$accession_gfa"
  mv "$internal_part" "$internal_gfa"
fi

mkdir -p "$component_dir"
if [[ ! -f "$accession_gbwt" ]]; then
  accession_part="$accession_gbwt.part"
  [[ ! -e "$accession_part" ]] || { printf 'error: review incomplete %s\n' "$accession_part" >&2; exit 1; }
  run_measured accession-gbwt-build "$vg" gbwt --progress \
    --temp-dir "$data_dir/tmp/accession-gbwt-build" --num-jobs "$threads" \
    --buffer-size "$gbwt_buffer_millions" \
    --max-node 0 --path-regex '^(.+)\.(chr[0-9]{2})\.([0-9]+)$' --path-fields _SCH \
    -G "$accession_gfa" -o "$accession_part"
  mv "$accession_part" "$accession_gbwt"
fi
if [[ ! -f "$accession_refs_gbwt" ]]; then
  accession_refs_part="$accession_refs_gbwt.part"
  [[ ! -e "$accession_refs_part" ]] || { printf 'error: review incomplete %s\n' "$accession_refs_part" >&2; exit 1; }
  run_measured accession-reference-setting "$vg" gbwt \
    --set-reference IRGSP-1.0 --set-reference NATELBORO \
    -o "$accession_refs_part" "$accession_gbwt"
  mv "$accession_refs_part" "$accession_refs_gbwt"
fi
if [[ ! -f "$internal_gbwt" ]]; then
  internal_part="$internal_gbwt.part"
  [[ ! -e "$internal_part" ]] || { printf 'error: review incomplete %s\n' "$internal_part" >&2; exit 1; }
  run_measured internal-gbwt-build "$vg" gbwt --progress \
    --temp-dir "$data_dir/tmp/internal-gbwt-build" --num-jobs "$threads" \
    --buffer-size "$gbwt_buffer_millions" \
    --max-node 0 -G "$internal_gfa" -o "$internal_part"
  mv "$internal_part" "$internal_gbwt"
fi
if [[ ! -f "$gbwt" ]]; then
  gbwt_part="$gbwt.part"
  [[ ! -e "$gbwt_part" ]] || { printf 'error: review incomplete %s\n' "$gbwt_part" >&2; exit 1; }
  run_measured gbwt-merge "$vg" gbwt --progress --temp-dir "$data_dir/tmp/gbwt-merge" \
    -m -o "$gbwt_part" "$accession_refs_gbwt" "$internal_gbwt"
  mv "$gbwt_part" "$gbwt"
fi

if [[ ! -f "$gbz" ]]; then
  gbz_part="$gbz.part"
  [[ ! -e "$gbz_part" ]] || { printf 'error: review incomplete %s\n' "$gbz_part" >&2; exit 1; }
  run_measured gbz-build "$vg" gbwt --progress --temp-dir "$data_dir/tmp/gbz-build" \
    -x "$xg" -g "$gbz_part" "$gbwt"
  mv "$gbz_part" "$gbz"
fi

"$vg" stats -F "$gbz" > "$results_dir/gbz-stats-format.txt"
"$vg" stats -z "$gbz" > "$results_dir/gbz-stats-size.txt"
"$vg" stats -N -E -l -r "$gbz" > "$results_dir/gbz-graph-stats.txt"
"$vg" paths -L -x "$gbz" > "$results_dir/gbz-paths.txt"
"$vg" paths -M -x "$gbz" > "$results_dir/gbz-path-metadata.txt"
"$vg" gbwt -M -Z "$gbz" > "$results_dir/gbwt-metadata-summary.txt"
"$vg" gbwt -S -Z "$gbz" > "$results_dir/gbwt-sample-count.txt"
"$vg" gbwt -S -L -Z "$gbz" > "$results_dir/gbwt-samples.txt"
"$vg" gbwt -C -Z "$gbz" > "$results_dir/gbwt-contig-count.txt"
"$vg" gbwt -C -L -Z "$gbz" > "$results_dir/gbwt-contigs.txt"
"$vg" gbwt -H -Z "$gbz" > "$results_dir/gbwt-haplotype-count.txt"
"$vg" gbwt -c -Z "$gbz" > "$results_dir/gbwt-path-count.txt"
"$vg" gbwt -T -Z "$gbz" > "$results_dir/gbwt-path-names.txt"
python3 "$repo_root/scripts/rice/audit-rice-paths.py" gbz \
  --metadata "$results_dir/gbz-path-metadata.txt" --names "$paths" \
  --tsv "$results_dir/path-metadata.tsv" --json "$results_dir/gbz-audit.json" \
  --format "$results_dir/gbz-stats-format.txt" --size "$results_dir/gbz-stats-size.txt" \
  --graph "$gbz"
diff -u "$results_dir/xg-graph-stats.txt" "$results_dir/gbz-graph-stats.txt" \
  > "$results_dir/xg-gbz-graph-stats.diff"

# Hash every named path sequence on both sides without retaining full FASTA copies.
"$vg" paths -F -x "$xg" | python3 "$repo_root/scripts/rice/hash-path-fasta.py" \
  --output "$results_dir/xg-path-sequences.tsv"
"$vg" paths -F -x "$gbz" | python3 "$repo_root/scripts/rice/hash-path-fasta.py" \
  --output "$results_dir/gbz-path-sequences.tsv"
diff -u "$results_dir/xg-path-sequences.tsv" "$results_dir/gbz-path-sequences.tsv" \
  > "$results_dir/xg-gbz-path-sequences.diff"

# Build the merged named/generic GBWT locate r-index with bounded threads.
if [[ ! -f "$rindex" ]]; then
  rindex_part="$rindex.part"
  [[ ! -e "$rindex_part" ]] || { printf 'error: remove or review incomplete %s\n' "$rindex_part" >&2; exit 1; }
  run_measured rindex-build "$vg" gbwt --progress --temp-dir "$data_dir/tmp/rindex-build" \
    --num-threads "$threads" \
    -r "$rindex_part" "$gbwt"
  mv "$rindex_part" "$rindex"
fi

# Use the unchanged production encoder for the anonymous baseline archive.
CARGO_TARGET_DIR="$cargo_target_dir" cargo build --release -p pangenome-range-cli
pngr="$cargo_target_dir/release/pangenome-range"
source_url=$(python3 - "$repo_root/data/rice/sources.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))["graph"]["finalUrl"])
PY
)
if [[ ! -f "$archive" ]]; then
  python3 "$repo_root/scripts/rice/run-measured.py" \
    --json "$results_dir/pngr-encode.measurement.json" \
    --stdout "$results_dir/pngr-encode.stdout.txt" \
    --stderr "$results_dir/pngr-encode.stderr.txt" \
    --temp-dir "$data_dir/tmp/pngr-source" -- \
    "$pngr" encode "$gbz" "$archive" \
      --sample NATELBORO --contig chr06 --annotations "$metadata_dir/rice-xa7-natelboro.gff3" \
      --annotation-sample NATELBORO --annotation-release PPanG-2024-Xa7-curated \
      --annotation-assembly NATELBORO-chr06 --reference-assembly PPanG-rice-MC-v2.2.2 \
      --dataset-title 'PPanG rice chromosome 6 Minigraph-Cactus graph' \
      --dataset-description 'Research conversion of the PPanG chromosome 6 rice graph, anchored on NATELBORO for the Xa7 locus' \
      --source-uri "$source_url" --threads "$threads" --source-access disk \
      --scratch-dir "$data_dir/tmp/pngr-source" --progress json \
      --report "$results_dir/pngr-build.json"
fi
"$pngr" validate "$archive" --mode standard --workers "$threads" --progress json \
  > "$results_dir/pngr-validation.json" 2> "$results_dir/pngr-validation.progress.ndjson"
"$pngr" verify "$archive" --against "$gbz" --sample NATELBORO --contig chr06 \
  --start 28873554 --end 28874897 --report "$results_dir/xa7-source-verification.json" \
  > "$results_dir/xa7-source-verification.stdout.json"
node "$repo_root/scripts/rice/query-xa7.mjs" "$archive" \
  "$results_dir/xa7-reader-evidence.json" "$results_dir/xa7-browser-workload.json"

sha256sum "$xg" "$gfa" "$gbwt" "$gbz" "$rindex" "$archive" \
  > "$results_dir/checksums.txt"
stat --printf='%n\t%s\n' "$xg" "$gfa" "$gbwt" "$gbz" "$rindex" "$archive" \
  > "$results_dir/file-sizes.tsv"
cp "$repo_root/data/rice/sources.json" "$results_dir/sources.json"
