#!/usr/bin/env bash
set -euo pipefail

# Run from the repository root. Large outputs and temporary files stay on the
# user-designated scratch drive; only compact evidence stays in results/.
export PPANG_RICE_DATA_DIR=/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/runs/2026-08-28-ppang-rice-chr06
export PPANG_RICE_GBWT_BUFFER_MILLIONS=20

scripts/rice/fetch-vg.sh
scripts/rice/fetch-ppang-rice.sh
scripts/rice/build-rice-corpus.sh

# Browser proof after the Node reader has produced the checksum-bound Xa7 workload.
pnpm bench -- browser \
  --file "$PPANG_RICE_DATA_DIR/rice-chr06-mc-xa7-anonymous.pngr" \
  --workload results/rice-acquisition/xa7-browser-workload.json \
  --run-id xa7-browser \
  --results-dir results/rice-acquisition/browser \
  --decoder pure-js
