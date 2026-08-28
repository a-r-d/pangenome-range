# PPanG rice chromosome 6 research corpus

This directory describes a local, reproducible conversion of the official PPanG
Minigraph-Cactus chromosome 6 graph. Large fetched and intermediate objects
belong on a scratch volume, must be ignored, and must not be committed. The
accepted anonymous `.pngr` is published under the confirmed MIT redistribution
status recorded in `LICENSE_REVIEW.md`.

The primary locus is `Xa7` at
`NATELBORO.chr06:28873554-28874897`. The PPanG comparison count is external
context until it is reproduced against the exact downloaded path set.

## Acquisition

Requirements: Python 3, curl, Node.js 24, pnpm, installed workspace
dependencies, and Playwright Chromium. The downloader prefers `aria2c` for
bounded parallel ranges and falls back to `wget -c`.

```bash
export PPANG_RICE_DATA_DIR=/path/to/large/scratch/rice-chr06
scripts/rice/fetch-vg.sh
scripts/rice/fetch-ppang-rice.sh
scripts/rice/build-rice-corpus.sh
```

The script discovers the graph link from the official page, saves the original
HTML and Playwright log, verifies a one-byte HTTP range response, enforces free
space of at least `max(50 GiB, 6 * XG bytes)`, resumes an interrupted download,
and records checksums in `sources.json`.

`fetch-vg.sh` pins the official x86-64 vg v1.76.1 release asset by byte length
and GitHub-published SHA-256. It installs only into the selected data directory;
it does not modify the system.

For the exact measured scratch path, buffer bound, conversion sequence, and
three-engine Xa7 browser workload, run `results/rice-acquisition/commands.sh`.

Expected files below `$PPANG_RICE_DATA_DIR` after the full experiment include:

```text
chr06_mc.xg
chr06_mc.paths.gfa
chr06_mc.named.gbwt
chr06_mc.named.gbz
chr06_mc.named.ri
rice-chr06-mc-xa7-anonymous.pngr
```

The published checksum-addressed `.pngr` is:

<https://archives.ard.ninja/pangenome-range/sha256/c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c/rice-chr06-mc-xa7-anonymous.pngr>

The XG, GFA, GBWT, GBZ, and r-index remain local research inputs.
