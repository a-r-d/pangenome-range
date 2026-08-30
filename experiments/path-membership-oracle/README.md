# Path-membership C++ oracle

Status: research-only. This executable is an independent correctness oracle; it is not
linked into the Rust encoder, npm package, browser reader, or `.pngr` format.

## Pinned upstream

The experiment was written and tested against:

- GBWT `c2e0199694fe41ec46c61c201c6ae0cd7dd08783` (version 1.6.0);
- vgteam SDSL `a4c77d4ee040344ee8c0cd185b1d26a3c3a95436`;
- the PPanG corpus built with official vg 1.76.1.

At the pinned GBWT commit, `FastLocate::decompressDA(node)` returns one source
sequence ID for every offset in the oriented node's record. In a bidirectional GBWT,
`Path::id(sequence_id)` maps both stored orientations to one canonical path ID and
`Path::is_reverse(sequence_id)` preserves the source orientation. The oracle does not
deduplicate that document array.

The pinned `gbwt-rs` dependency (`gbz` 0.7.0, upstream commit
`a0d72bc3bd261fc433e59de64b2706f5f45708ad`) can deserialize the GBWT document-array
samples but does not implement locate queries. That gap is evaluated separately in the
Rust experiment.

## Build

Build GBWT and SDSL outside this repository, then point CMake at their install roots.
For example:

```bash
cmake -S experiments/path-membership-oracle \
  -B /tmp/path-membership-oracle-build \
  -DGBWT_ROOT=/tmp/path-membership-gbwt-prefix \
  -DSDSL_ROOT=/tmp/path-membership-sdsl-prefix
cmake --build /tmp/path-membership-oracle-build -j 8
ctest --test-dir /tmp/path-membership-oracle-build --output-on-failure
```

The tool loads the GBWT through its official Simple-SDS loader and loads `.ri` through
the official SDSL loader before calling `FastLocate::setGBWT()`.

## Commands

```bash
path-membership-oracle metadata --gbwt graph.gbwt
path-membership-oracle node-da --gbwt graph.gbwt --r-index graph.ri --node 42+
path-membership-oracle locate --gbwt graph.gbwt --r-index graph.ri --node 42+ --offset 7
path-membership-oracle batch-locate --gbwt graph.gbwt --r-index graph.ri \
  --input positions.bin --output located.bin
path-membership-oracle brute-force --gbwt graph.gbwt --output occurrences.bin
path-membership-oracle verify-brute-force --gbwt graph.gbwt --r-index graph.ri
```

`metadata`, `node-da`, `locate`, and `verify-brute-force` write JSON or NDJSON.
Metadata names are reconstructed as `sample#haplotype#contig`; nonzero fragment IDs
are appended as `#fragment=N`. `path_sense` is derived from the GBWT
`reference_samples` tag, with `_gbwt_ref` reported as `generic`.

`verify-brute-force` independently iterates every stored GBWT sequence with `start()`
and `LF()`, records the exact `(node, record offset, sequence ID)` assignment, and
requires equality with every `FastLocate::decompressDA(node)` entry. Its flat expected
document array uses eight bytes per non-endmarker occurrence and refuses allocations
over 16 GiB by default; change that only with an explicit `--max-bytes` value. Do not
run it on full HPRC or whole 1000G.

## Binary interfaces

All integers are unsigned little-endian.

`positions.bin`:

```text
8 bytes  "PMPO0001"
u64      record count
repeat count times:
  u64    encoded oriented node (GBWT Node::encode)
  u64    node-record offset
```

`located.bin`:

```text
8 bytes  "PMLO0001"
u64      record count
repeat count times (40 bytes):
  u64    encoded oriented node
  u64    node-record offset
  u64    source sequence ID
  u64    canonical path ID
  u8     source sequence orientation (0 forward, 1 reverse)
  u8[7]  zero padding
```

The path catalog from `metadata` supplies biological metadata for the canonical path
ID. Keeping the high-volume batch output fixed-width avoids repeating names per
occurrence.

`occurrences.bin`:

```text
8 bytes  "PMBF0001"
u64      occurrence count
repeat count times (32 bytes):
  u64    encoded oriented node
  u64    node-record offset
  u64    source sequence ID
  u64    position within that stored GBWT sequence
```

## Synthetic fixture

`tests/synthetic.gfa` contains exactly one reference and four named haplotypes:
an insertion, a deletion-like skip, a reverse traversal, and a path that exits and
revisits nodes 2-3. The revisit creates two local fragments for a tile restricted to
nodes 2-3. Generate the GBWT/GBZ/r-index and run the equality gate with:

```bash
bash experiments/path-membership-oracle/tests/run-synthetic.sh \
  /tmp/path-membership-oracle-build/path-membership-oracle \
  /path/to/vg
```

The test builds the GBZ and r-index together, then extracts a standalone GBWT from
the GBZ. Current vg intentionally ignores `-o` when `-g` writes a GBZ, so those are
separate commands.
