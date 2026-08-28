#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
data_dir=${PPANG_RICE_DATA_DIR:-"$repo_root/data/rice"}
provenance_dir="$data_dir/provenance"
source_path="$data_dir/chr06_mc.xg"
part_path="$source_path.part"
ppang_commit=64f092fb94ef5bf0e16faf131537fac93163c2c8
mkdir -p "$data_dir" "$provenance_dir" "$data_dir/upstream/PPanG-$ppang_commit"

url=$(python3 "$repo_root/scripts/rice/discover-ppang-assets.py" --output-dir "$provenance_dir")
probe_headers="$provenance_dir/chr06_mc.probe.headers"
curl --fail --silent --show-error --location \
  --header 'Range: bytes=0-0' --dump-header "$probe_headers" \
  --output "$provenance_dir/chr06_mc.probe.byte" "$url"

readarray -t probe < <(python3 - "$probe_headers" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text(errors="replace")
blocks = [block for block in re.split(r"\r?\n\r?\n", text) if block.startswith("HTTP/")]
if not blocks:
    raise SystemExit("probe returned no HTTP response")
block = blocks[-1]
status = block.splitlines()[0].split()[1]
headers = {}
for line in block.splitlines()[1:]:
    if ":" in line:
        key, value = line.split(":", 1)
        headers[key.lower()] = value.strip()
content_range = headers.get("content-range", "")
match = re.fullmatch(r"bytes 0-0/(\d+)", content_range)
if status != "206" or not match:
    raise SystemExit(f"expected a 206 bytes 0-0/N probe, got status={status} Content-Range={content_range!r}")
print(match.group(1))
print(status)
print(headers.get("etag", ""))
print(headers.get("last-modified", ""))
print(headers.get("accept-ranges", ""))
PY
)
xg_bytes=${probe[0]}
required_bytes=$((xg_bytes * 6))
minimum_50_gib=$((50 * 1024 * 1024 * 1024))
if (( required_bytes < minimum_50_gib )); then
  required_bytes=$minimum_50_gib
fi
available_bytes=$(df -B1 --output=avail "$data_dir" | tail -1 | tr -d ' ')
if (( available_bytes < required_bytes )); then
  printf 'error: need at least %s free bytes, but only %s are available in %s\n' \
    "$required_bytes" "$available_bytes" "$data_dir" >&2
  exit 1
fi

for path in src/config.json src/builtin_genes.json src/reference_genes.json riceData/chr06_mc.bed README.md LICENSE.txt; do
  destination="$data_dir/upstream/PPanG-$ppang_commit/$path"
  mkdir -p "$(dirname "$destination")"
  curl --fail --silent --show-error --location \
    "https://raw.githubusercontent.com/SJTU-CGM/PPanG/$ppang_commit/$path" \
    --output "$destination"
done

download_started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
source_reused=false
if [[ -f "$source_path" ]]; then
  source_reused=true
  actual_bytes=$(stat --format=%s "$source_path")
  if (( actual_bytes != xg_bytes )); then
    printf 'error: existing %s has %s bytes, expected %s; refusing to overwrite\n' \
      "$source_path" "$actual_bytes" "$xg_bytes" >&2
    exit 1
  fi
else
  local_aria2="$data_dir/tools/aria2/usr/bin/aria2c"
  local_aria2_lib="$data_dir/tools/aria2/usr/lib/x86_64-linux-gnu"
  if command -v aria2c >/dev/null 2>&1; then
    aria2=(aria2c)
  elif [[ -x "$local_aria2" ]]; then
    aria2=(env "LD_LIBRARY_PATH=$local_aria2_lib" "$local_aria2")
  else
    aria2=()
  fi
  if (( ${#aria2[@]} > 0 )); then
    "${aria2[@]}" --continue=true --allow-overwrite=true --auto-file-renaming=false \
      --file-allocation=none --max-connection-per-server=16 --split=16 --min-split-size=16M \
      --dir="$data_dir" --out="$(basename "$part_path")" "$url" \
      >> "$provenance_dir/chr06_mc.download.log" 2>&1
  else
    wget --continue --server-response --output-file="$provenance_dir/chr06_mc.download.log" \
      --output-document="$part_path" "$url"
  fi
  actual_bytes=$(stat --format=%s "$part_path")
  if (( actual_bytes != xg_bytes )); then
    printf 'error: downloaded %s bytes, expected %s\n' "$actual_bytes" "$xg_bytes" >&2
    exit 1
  fi
  mv "$part_path" "$source_path"
fi
sha256=$(sha256sum "$source_path" | awk '{print $1}')
download_finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)

python3 - "$repo_root" "$data_dir" "$url" "$xg_bytes" "$sha256" "$download_started" "$download_finished" \
  "${probe[1]}" "${probe[2]}" "${probe[3]}" "${probe[4]}" "$ppang_commit" "$probe_headers" "$source_reused" <<'PY'
import datetime as dt, hashlib, json, pathlib, re, sys
root = pathlib.Path(sys.argv[1])
data_dir = pathlib.Path(sys.argv[2])
sources_path = root / "data/rice/sources.json"
data = json.loads(sources_path.read_text())
header_text = pathlib.Path(sys.argv[13]).read_text(errors="replace")
responses = []
for block in re.split(r"\r?\n\r?\n", header_text):
    if not block.startswith("HTTP/"):
        continue
    lines = block.splitlines()
    headers = {}
    for line in lines[1:]:
        if ":" in line:
            key, value = line.split(":", 1)
            headers[key.lower()] = value.strip()
    responses.append({
        "status": int(lines[0].split()[1]),
        "location": headers.get("location"),
    })
graph = data["graph"]
graph.update({
    "finalUrl": sys.argv[3], "bytes": int(sys.argv[4]), "sha256": sys.argv[5],
    "httpStatus": int(sys.argv[8]), "etag": sys.argv[9] or None,
    "lastModified": sys.argv[10] or None, "acceptRanges": sys.argv[11] or None,
    "httpResponses": responses,
    "checksumRecordedAt": sys.argv[7],
})
source_reused = sys.argv[14] == "true"
if not source_reused or not graph.get("downloadStarted") or not graph.get("downloadFinished"):
    graph.update({"downloadStarted": sys.argv[6], "downloadFinished": sys.argv[7]})
commit = sys.argv[12]
base = data_dir / f"upstream/PPanG-{commit}"
files = []
for path in ["src/config.json", "src/builtin_genes.json", "src/reference_genes.json", "riceData/chr06_mc.bed", "README.md", "LICENSE.txt"]:
    raw = (base / path).read_bytes()
    files.append({"path": path, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()})
data["ppangRepository"].update({"commit": commit, "files": files})
data["updatedAt"] = dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")
sources_path.write_text(json.dumps(data, indent=2) + "\n")
PY

printf 'downloaded %s (%s bytes, sha256 %s)\n' "$source_path" "$actual_bytes" "$sha256"
