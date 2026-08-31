#!/usr/bin/env bash
set -euo pipefail

: "${HPRC_DATA_DIR:?set HPRC_DATA_DIR to the external pangenome-range-data directory}"

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pngr="$repo_root/target/release/pangenome-range"
source_gbz="$HPRC_DATA_DIR/sources/hprc-v2.1-mc-grch38.gbz"
annotations="$HPRC_DATA_DIR/annotations/gencode-v50/gencode.v50.annotation.gff3"
run_dir="${HPRC_RUN_DIR:-$HPRC_DATA_DIR/runs/2026-08-30-hprc-v2.1-named-membership}"
cache="${HPRC_CACHE_DIR:-$HPRC_DATA_DIR/cache/hprc-source-cache-v2}"
scratch="$run_dir/scratch"
archive_prefix="$run_dir/hprc-v2.1-gencode-v50-named-membership"
archive_candidate="$archive_prefix.candidate.pngr"
archive_path_file="$run_dir/archive.path"
report="$run_dir/encode-report.json"
oracle="$run_dir/oracle-report.json"
workload="$repo_root/experiments/hprc-v2.1-named-membership-workload.json"
stage="${HPRC_STAGE:-all}"
encode_timeout="${HPRC_ENCODE_TIMEOUT:-30m}"
encode_threads="${HPRC_ENCODE_THREADS:-32}"
validation_threads="${HPRC_VALIDATION_THREADS:-32}"
validation_max_queued_bytes="${HPRC_VALIDATION_MAX_QUEUED_BYTES:-12884901888}"
active_unit=''
capped_run_counter=0

source_sha256='11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b'
annotations_sha256='d97bc25b7d4d4aa9614c4cf6fa4748c083b1531cb60c52c0612c9ff7ec4813eb'

[[ -x "$pngr" ]] || {
  printf 'error: build the release CLI first: cargo build --release -p pangenome-range-cli\n' >&2
  exit 1
}
[[ -f "$source_gbz" && -f "$annotations" && -f "$workload" ]] || {
  printf 'error: missing HPRC source, GENCODE annotation, or workload\n' >&2
  exit 1
}
command -v systemd-run >/dev/null

case "$stage" in
  cache | encode | validate | oracle | all) ;;
  *)
    printf 'error: HPRC_STAGE must be cache, encode, validate, oracle, or all\n' >&2
    exit 1
    ;;
esac

cleanup_active_unit() {
  if [[ -n "$active_unit" ]]; then
    local unit=$active_unit
    active_unit=''
    systemctl --user stop "$unit.service" >/dev/null 2>&1 || true
  fi
}

trap cleanup_active_unit EXIT INT TERM

run_capped() {
  local high=$1
  local maximum=$2
  shift 2
  capped_run_counter=$((capped_run_counter + 1))
  active_unit="pangenome-range-hprc-${stage}-$$-${capped_run_counter}"
  local status
  if systemd-run --user --unit="$active_unit" --wait --collect --pipe --quiet \
    --property="MemoryHigh=$high" \
    --property="MemoryMax=$maximum" \
    --property=MemorySwapMax=0 \
    --property=ManagedOOMPreference=avoid \
    --property=TasksMax=128 \
    -- "$@"; then
    status=0
  else
    status=$?
  fi
  active_unit=''
  return "$status"
}

verify_sha256() {
  local path=$1
  local expected=$2
  [[ "$(sha256sum "$path" | awk '{print $1}')" == "$expected" ]] || {
    printf 'error: checksum mismatch for %s\n' "$path" >&2
    exit 1
  }
}

mkdir -p "$run_dir" "$scratch" "$(dirname "$cache")"

if [[ "$stage" == cache || "$stage" == encode || "$stage" == oracle || "$stage" == all ]]; then
  verify_sha256 "$source_gbz" "$source_sha256"
fi
if [[ "$stage" == encode || "$stage" == all ]]; then
  verify_sha256 "$annotations" "$annotations_sha256"
fi

if [[ "$stage" == cache || "$stage" == all ]]; then
  if [[ ! -d "$cache" ]]; then
    [[ ! -e "$cache.part" ]] || {
      printf 'error: review incomplete cache %s\n' "$cache.part" >&2
      exit 1
    }
    run_capped 1536M 2G /usr/bin/time -v -o "$run_dir/source-cache-time-v.txt" \
      "$pngr" source-cache build "$source_gbz" "$cache.part"
    mv "$cache.part" "$cache"
  fi
  "$pngr" source-cache inspect "$cache" > "$run_dir/source-cache-inspect.json"
fi

if [[ "$stage" == encode || "$stage" == all ]]; then
  [[ -d "$cache" ]] || {
    printf 'error: build the source cache first: HPRC_STAGE=cache %s\n' "$0" >&2
    exit 1
  }
  [[ ! -e "$archive_candidate" ]] || {
    printf 'error: review incomplete archive %s\n' "$archive_candidate" >&2
    exit 1
  }
  [[ ! -e "$archive_path_file" ]] || {
    printf 'error: archive already recorded in %s\n' "$archive_path_file" >&2
    exit 1
  }
  run_capped 35G 35G /usr/bin/time -v -o "$run_dir/encode-time-v.txt" \
    timeout --signal=TERM --kill-after=60s "$encode_timeout" \
    "$pngr" encode "$source_gbz" "$archive_candidate" \
    --source-access disk --source-cache "$cache" --scratch-dir "$scratch" \
    --window-size 16384 --codec zstd-3 \
    --threads "$encode_threads" --max-queued-bytes 536870912 \
    --annotations "$annotations" --annotation-sample GRCh38 \
    --annotation-feature-type gene \
    --annotation-release 'GENCODE Human Release 50' \
    --annotation-assembly GRCh38.p14 \
    --reference-assembly GRCh38.p14 \
    --dataset-title 'HPRC Release 2 Minigraph-Cactus v2.1 GRCh38' \
    --dataset-description 'Whole HPRC v2.1 GRCh38/CHM13 reference archive with GENCODE v50 gene search and exact named GBWT source-path membership' \
    --source-uri 'https://s3-us-west-2.amazonaws.com/human-pangenomics/pangenomes/freeze/release2/minigraph-cactus/v2.1/hprc-v2.1-mc-grch38/hprc-v2.1-mc-grch38.gbz' \
    --path-membership --path-locate-max-lf-steps 8192 \
    --report "$report" --progress plain --progress-interval-seconds 30 \
    > >(tee "$run_dir/encode-progress.log") 2>&1

  archive_sha256=$(sha256sum "$archive_candidate" | awk '{print $1}')
  archive="$archive_prefix-${archive_sha256:0:16}.pngr"
  [[ ! -e "$archive" ]] || {
    printf 'error: refusing to overwrite content-addressed archive %s\n' "$archive" >&2
    exit 1
  }
  mv "$archive_candidate" "$archive"
  printf '%s\n' "$archive" > "$archive_path_file"
  printf '%s  %s\n' "$archive_sha256" "$archive" > "$run_dir/archive.sha256"
  stat --format='%s' "$archive" > "$run_dir/archive.bytes"
fi

if [[ "$stage" == validate || "$stage" == oracle || "$stage" == all ]]; then
  [[ -f "$archive_path_file" ]] || {
    printf 'error: encode the archive first: HPRC_STAGE=encode %s\n' "$0" >&2
    exit 1
  }
  archive=$(<"$archive_path_file")
  [[ "$archive" == "$run_dir"/* && -f "$archive" ]] || {
    printf 'error: invalid archive path recorded in %s\n' "$archive_path_file" >&2
    exit 1
  }
fi

if [[ "$stage" == validate || "$stage" == all ]]; then
  run_capped 35G 35G /usr/bin/time -v -o "$run_dir/validate-time-v.txt" \
    "$pngr" validate "$archive" --mode full --workers "$validation_threads" \
    --max-queued-bytes "$validation_max_queued_bytes" \
    --progress plain --progress-interval-seconds 30 \
    > >(tee "$run_dir/validate-report.json")
fi

if [[ "$stage" == oracle || "$stage" == all ]]; then
  run_capped 35G 35G /usr/bin/time -v -o "$run_dir/oracle-time-v.txt" \
    "$pngr" verify "$archive" --against "$source_gbz" \
    --workload "$workload" --report "$oracle"
fi

printf 'completed HPRC named-membership stage %s\n' "$stage"
