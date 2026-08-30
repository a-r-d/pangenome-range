# Poplar pangenome source record

This directory contains metadata only. GBZ inputs, source caches, scratch files, and
generated `.pngr` archives must stay outside the repository.

The canary is chromosome 16. The demo candidate is chromosome 19 from the ORNL
Populus trichocarpa pangenome graph release. Both files use GBZ serialization v1 and
contain embedded GBWT document-array samples required by the bounded named-membership
encoder.

Fetch with `scripts/poplar/fetch-poplar.sh` and build with
`scripts/poplar/build-poplar-demo.sh`. Both scripts require `POPLAR_DATA_DIR` to name
an external data directory. The build runs inside a 4 GiB, zero-swap systemd user
unit and never runs in CI.

Do not publish or redistribute the derived archive yet. The public project page and
dataset README do not state a data redistribution license; see `LICENSE_REVIEW.md`.
