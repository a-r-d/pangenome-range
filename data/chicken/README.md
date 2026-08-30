# Chicken demo inputs

Large inputs and generated archives stay outside this repository. Set
`CHICKEN_DATA_DIR` to an external directory, then run:

```bash
scripts/chicken/fetch-chicken.sh
scripts/chicken/build-chicken-demo.sh
```

The build deliberately extracts the closed chromosome 15 component before
invoking `vg`. It never asks `vg` to index the complete 12.6 GB uncompressed
GFA. Every source and derived boundary is checksum-gated, and each expensive
stage runs in a zero-swap cgroup with a stage-specific memory ceiling.

The retained result and exact commands are documented in
`results/named-membership/chicken/REPORT.md`.
