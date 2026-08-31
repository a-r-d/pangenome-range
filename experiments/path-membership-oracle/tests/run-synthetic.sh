#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  printf 'usage: %s PATH_MEMBERSHIP_ORACLE VG\n' "$0" >&2
  exit 2
fi

oracle=$1
vg=$2
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf -- "$work_dir"' EXIT

"$vg" gbwt -G "$script_dir/synthetic.gfa" \
  --path-regex '([^#]+)#([0-9]+)#(.+)' --path-fields _SHC \
  --set-reference REF -g "$work_dir/synthetic.gbz" -r "$work_dir/synthetic.ri"
"$vg" gbwt -Z "$work_dir/synthetic.gbz" -o "$work_dir/synthetic.gbwt"

"$oracle" metadata --gbwt "$work_dir/synthetic.gbwt" > "$work_dir/metadata.ndjson"
"$oracle" verify-brute-force --gbwt "$work_dir/synthetic.gbwt" \
  --r-index "$work_dir/synthetic.ri" > "$work_dir/verification.json"
{
  printf 'oriented_node\trecord_offset\tsequence_id\tcanonical_path_id\tsequence_orientation\n'
  for node in 1+ 1- 2+ 2- 3+ 3- 4+ 4- 5+ 5- 6+ 6- 7+ 7-; do
    "$oracle" node-da --gbwt "$work_dir/synthetic.gbwt" \
      --r-index "$work_dir/synthetic.ri" --node "$node" |
      python3 -c 'import json,sys
for line in sys.stdin:
 row=json.loads(line)
 print(sys.argv[1], row["record_offset"], row["sequence_id"], row["canonical_path_id"], row["sequence_orientation"], sep="\t")' "$node"
  done
} > "$work_dir/node-da.tsv"
diff -u "$script_dir/expected-node-da.tsv" "$work_dir/node-da.tsv"

python3 - "$work_dir/metadata.ndjson" "$work_dir/verification.json" <<'PY'
import json
import sys

metadata = [json.loads(line) for line in open(sys.argv[1])]
catalog = [row for row in metadata if row.get("type") == "path"]
assert [row["raw_name"] for row in catalog] == [
    "REF#0#chr1",
    "SAMPLE_A#0#chr1",
    "SAMPLE_B#0#chr1",
    "SAMPLE_C#0#chr1",
    "SAMPLE_D#0#chr1",
]
assert catalog[0]["path_sense"] == "reference"
verification = json.load(open(sys.argv[2]))
assert verification["equal"] is True
assert verification["paths"] == 5
assert verification["sequences"] == 10
assert verification["occurrences"] > 0
PY
