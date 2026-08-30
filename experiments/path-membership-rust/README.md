# Named path membership Rust experiment

This isolated crate is the historical precursor to the production implementation
recorded in `docs/adr/ADR-path-membership-production.md`. Its prepared summary and
catalog can still feed the encoder's paired research-oracle flags. The registered
production extension and public TypeScript API now live in the workspace crates;
this harness is retained to reproduce the earlier codec and oracle comparisons.

`prepare` reads unchanged v1 payloads and emits only the local GBWT start positions.
`analyze` joins oracle locate results, subtracts exactly the real reference occurrence,
checks every grouped multiplicity against the production anonymous reconstruction, and
measures six reversible membership choices. `answer` is the experimental biological
API for traversal-group or oriented-node carrier queries.

Run `cargo run --manifest-path experiments/path-membership-rust/Cargo.toml -- help`
for the exact `prepare`, `analyze`, and `answer` arguments. For example:

```bash
cargo run --manifest-path experiments/path-membership-rust/Cargo.toml -- \
  answer --summary summary.json --catalog catalog.ndjson --group-index 0
```

The adaptive corpus is benchmarked in real headless Chromium with:

```bash
node experiments/path-membership-rust/browser-benchmark.mjs adaptive.bin 1000
```

The browser decoder intentionally implements the experiment's delta, interval,
dense, and Roaring-style tags directly. Complement remains legal only for a proven
tile partition; it is excluded from the standalone corpus because decoding it also
requires the tile-universe object.

## Rust locate prototype

The historical `locate-rust` command implements ordinary GBWT locate without an `.ri`.
It decodes the four
Simple-SDS structures in the document-array sample option already embedded in a
GBWT, checks the structure invariants, and follows LF until it reaches a sample. A
required safety limit prevents an unbounded walk. The locate source remains behind
the `unpublished-fork-locate` feature because it calls APIs from the unpublished
`a-r-d/gbwt-rs` experiment branch. The default experiment build uses released
`gbz 0.7.0` and remains portable; production does not use the fork.

To reproduce only the historical fork-backed commands, point Cargo's `gbz` patch at
a checkout containing that experiment and enable the feature. The checkout path is
deliberately supplied at invocation time rather than committed into this repository:

```bash
cargo run --release \
  --manifest-path experiments/path-membership-rust/Cargo.toml \
  --features unpublished-fork-locate \
  --config 'patch.crates-io.gbz.path="/path/to/gbwt-rs"' -- help
```

Verify the complete synthetic table:

```bash
cargo run --release --manifest-path experiments/path-membership-rust/Cargo.toml -- \
  verify-locate-tsv --gbwt synthetic.gbwt \
  --expected experiments/path-membership-oracle/tests/expected-node-da.tsv \
  --stats synthetic-locate.json --max-lf-steps 10000
```

Locate a fixed binary workload and emit the same `PMLO0001` format as the C++
oracle:

```bash
cargo run --release --manifest-path experiments/path-membership-rust/Cargo.toml -- \
  locate-rust --gbwt graph.gbwt --positions positions.bin \
  --output located-rust.bin --stats locate.json --max-lf-steps 10000
cmp located-rust.bin located-cpp.bin
```

`sample-positions` produces a deterministic bounded workload from a seed. It has a
hard one-million-position count limit and does not enumerate paths.

`verify-sampled-sequences` is the scale gate that does not require a C++ `.ri`. It
chooses deterministic stored sequence IDs, walks forward from each known sequence
start to obtain expected `(position, sequence ID)` pairs, and then requires ordinary
locate to recover every sequence ID in the same single GBWT load. Both the sample
count and forward/LF walk lengths are explicitly bounded.

## Paged catalog prototype

`export-catalog` reads path metadata through the ordinary Rust GBWT load. The rice
output was normalized against the independent C++ exporter before the paged format
was measured.

```bash
cargo run --release --manifest-path experiments/path-membership-rust/Cargo.toml -- \
  export-catalog --gbwt graph.gbwt --output catalog.ndjson

cargo run --release --manifest-path experiments/path-membership-rust/Cargo.toml -- \
  build-paged-catalog --catalog catalog.ndjson --output catalog.pmpc \
  --records-per-page 1024 --stats build.json

cargo run --release --manifest-path experiments/path-membership-rust/Cargo.toml -- \
  verify-paged-catalog --catalog catalog.ndjson --paged catalog.pmpc \
  --query-ids path-ids.txt --max-data-ranges 3 --stats verify.json
```

`PMPC0001` is an isolated experiment, not a committed `.pngr` version. It uses a
64-byte header, fixed 48-byte page directory entries, arithmetic path-ID lookup,
front-coded strings, page-local zstd-3, BLAKE3-128 integrity, and unsigned 64-bit
offsets. The verifier exhaustively compares every decoded record with the NDJSON
source before reporting query bytes. The reader rejects malformed dimensions,
noncontiguous or out-of-file ranges, unknown codecs, digest failures, truncation, and
trailing bytes.

The independent browser proof runs the same query through a strict local `206`
server in all requested Playwright engines:

```bash
PANGENOME_RANGE_CATALOG_BROWSERS=chromium,firefox,webkit \
  node experiments/path-membership-rust/catalog-browser-benchmark.mjs \
  catalog.pmpc build.json path-ids.txt catalog.ndjson browser.json
```

The JavaScript runtime validates `206`, `Content-Range`, `Accept-Ranges`, response
length, file identity, the root digest, and every selected page digest. Loopback
timings prove functional browser decoding only; they are not network benchmarks.

## Integrated encoder proof

Package a prepared bounded summary and complete catalog into one `.pngr`:

```bash
pangenome-range encode input.gbz output.pngr \
  --sample GRCh38 --contig chr5 --start 1245184 --end 1310720 \
  --experimental-path-membership-summary summary.json \
  --experimental-path-catalog catalog.ndjson
```

Both flags are required. The encoder does not run locate itself yet. The standard
pre-rename validator decodes the extension and reconciles every traversal digest and
weight against the regional payload.

Exercise the extension through a strict local range origin:

```bash
PANGENOME_RANGE_MEMBERSHIP_BROWSERS=chromium,firefox,webkit \
  node experiments/path-membership-rust/integrated-browser-benchmark.mjs \
  output.pngr GRCh38 chr5 1261568 1277952 catalog.ndjson browser.json
```
