#!/usr/bin/env bash
set -euo pipefail

: "${CHICKEN_DATA_DIR:?set CHICKEN_DATA_DIR to an external data directory}"

graph_url='https://zenodo.org/api/records/10018222/files/pangenome.gfa.gz/content'
graph_bytes=2358369558
graph_sha256='609230aa36071690bfde02445a7d0693f75e512a79276f60a3b5d04361c2600d'
annotation_url='https://ftp.ncbi.nlm.nih.gov/genomes/all/GCF/016/699/485/GCF_016699485.2_bGalGal1.mat.broiler.GRCg7b/GCF_016699485.2_bGalGal1.mat.broiler.GRCg7b_genomic.gff.gz'
annotation_bytes=25161571
annotation_sha256='d298f52807d16a467068a18dcf384b331b823d0861837731cc77a331e07fef72'
vg_url='https://github.com/vgteam/vg/releases/download/v1.76.1/vg'
vg_bytes=55374080
vg_sha256='87b457fdda6801c9580f79a53c3c0aa502261420abf222920c7222a703fd856b'

fetch_verified() {
  local url=$1
  local destination=$2
  local expected_bytes=$3
  local expected_sha256=$4
  local part="$destination.part"
  mkdir -p "$(dirname "$destination")"
  if [[ ! -f "$destination" ]]; then
    [[ ! -e "$part" ]] || {
      printf 'error: review or remove incomplete download %s\n' "$part" >&2
      exit 1
    }
    curl --fail --location --show-error --output "$part" "$url"
    [[ "$(stat --format=%s "$part")" == "$expected_bytes" ]]
    [[ "$(sha256sum "$part" | awk '{print $1}')" == "$expected_sha256" ]]
    mv "$part" "$destination"
  fi
  [[ "$(stat --format=%s "$destination")" == "$expected_bytes" ]]
  [[ "$(sha256sum "$destination" | awk '{print $1}')" == "$expected_sha256" ]]
}

fetch_verified "$graph_url" "$CHICKEN_DATA_DIR/pangenome.gfa.gz" "$graph_bytes" "$graph_sha256"
fetch_verified "$annotation_url" "$CHICKEN_DATA_DIR/GCF_016699485.2_genomic.gff.gz" "$annotation_bytes" "$annotation_sha256"
fetch_verified "$vg_url" "$CHICKEN_DATA_DIR/tools/vg-v1.76.1" "$vg_bytes" "$vg_sha256"
chmod +x "$CHICKEN_DATA_DIR/tools/vg-v1.76.1"

printf 'verified chicken inputs in %s\n' "$CHICKEN_DATA_DIR"
