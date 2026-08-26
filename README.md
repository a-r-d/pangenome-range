# pangenome-range

`pangenome-range` is a systems-research project for testing cloud-native,
range-addressable layouts for pangenome graphs. The target is a static immutable
`.pngr` object on an HTTP origin, object store, or CDN that can answer
interactive genomic-region queries with few byte-range reads and no custom
query server.

The repository contains two separately distributed products under one name:

- the native Rust `pangenome-range` CLI for encoding, verification, and
  research measurements;
- the private TypeScript `pangenome-range` workspace package for browser/Node
  range reading and framework-neutral viewing.

The Rust binary is not shipped through npm. See
[`docs/DISTRIBUTION.md`](docs/DISTRIBUTION.md) for the intended release boundary.

The only current on-disk format is pre-release file-format v1: archive magic
`PNGRNG01` and regional magic `PNGRGN01`. Its complete normative contract is
[`docs/FILE_FORMAT_V1.md`](docs/FILE_FORMAT_V1.md). Historical research objects
are intentionally unsupported and must be regenerated with the current
encoder; npm/Cargo package versions are independent of the file-format number.


## Quick start

```bash
./scripts/fetch-test-data.sh
cargo run --release -- inspect test-data/micb-kir3dl1.gbz
cargo run --release -- benchmark-source test-data/micb-kir3dl1.gbz
cargo run --release -- encode test-data/mhc-10.gbz /tmp/mhc.pngr \
  --sample MHC-GRCh38 --contig MHC --progress plain
cargo run --release -- verify /tmp/mhc.pngr \
  --against test-data/mhc-10.gbz --sample MHC-GRCh38 --contig MHC \
  --start 100000 --end 200000
cargo run --release -- validate /tmp/mhc.pngr
cargo run --release -- fixtures export test-data/conformance
```

Development checks:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The TypeScript workspace uses Node.js 24 LTS and pnpm:

```bash
pnpm install --frozen-lockfile
pnpm check
pnpm check:rust
pnpm build
pnpm docs:dev
```

`pnpm check` uses Biome for lint and format verification, then runs strict
TypeScript checking and tests. `pnpm format` applies Biome formatting.

`pnpm test:browser` builds the public reader and exercises real cross-origin
HTTP `206` bootstrap, directory, zstd payload, and decode paths in Chromium,
Firefox, and WebKit against a deterministic transport fixture.

## TypeScript reader

The package is private while naming, ownership, and release policy remain under
review. Browser code can open a URL or a local `File`/`Blob` directly:

```ts
import { openPangenome } from "pangenome-range";

const archive = await openPangenome({
  source: "https://example.test/graph.pngr",
  directoryCacheBytes: 1024 * 1024,
  payloadCacheBytes: 32 * 1024 * 1024,
});

const result = await archive.query({
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
  context: 100,
  trace: true,
});

console.log(result.graph.nodes.ids, result.tiles[0]?.haplotypes);
console.log(result.trace?.requestRanges, result.trace?.canonicalHash);
await archive.close();
```

`queryTiles()` is the streaming primitive. It preserves anonymous haplotype
evidence per source tile; `query()` merges only globally mergeable graph
topology and keeps those tiles alongside the merged graph.

```ts
for await (const tile of archive.queryTiles({
  sample: "GRCh38",
  contig: "chr19",
  start: 54_816_468,
  end: 54_830_778,
})) {
  renderProgressively(tile);
}
```

Use the isolated Node subpath for positioned file reads:

```ts
import { openPangenome } from "pangenome-range/reader";
import { FileRangeSource } from "pangenome-range/node";

const source = await FileRangeSource.open("graph.pngr");
const archive = await openPangenome(source);
```

Run measurements with `--release`. Large data and benchmarks will remain
opt-in; the default test suite uses synthetic bytes and stays fast.

## Workspace

- `pangenome-range-format`: `RangeSource`, local positioned reads, exact trace metrics,
  and an idealized network-cost model. It deliberately defines no archive.
- `pangenome-range-build`: an interface for replaceable candidate-layout experiments.
- `pangenome-range-query`: storage-independent query/correctness types and a canonical
  BLAKE3 comparison hash for local graph semantics.
- `pangenome-range-cli`: the direct-write v1 encoder, GBZ inspection, source
  tracing, and retained research benchmarks.
- `packages/browser`: private ESM package with isolated reader, viewer, and Node
  exports, strict HTTP/Blob/memory/file range sources, v1-only archive and
  record-preserving regional decoding, canonical graph assembly, and optional
  query traces. Rendering is not implemented in this tranche.
- `packages/benchmark`: private browser/Node benchmark tools with a real
  cross-origin range-origin smoke test; a full latency corpus remains future
  work.
- `docs`: the existing research Markdown plus a VitePress shell and demo
  placeholder deployed under `/pangenome-range/`.

`encode INPUT.gbz OUTPUT.pngr` is implemented with sample/contig/interval and
`--max-chunks` pilot guards, bounded deterministic parallel tile construction,
exact compressed local GBWT records, direct writing, JSON build reports,
periodic coordinate/base/chunk/percent/throughput/ETA progress, archive
validation, and atomic rename. It defaults to available parallelism capped at
eight workers and a 256 MiB raw/compressed queue. Progress defaults to
five-second snapshots and is configurable with
`--progress-interval-seconds`. Run
`pangenome-range help` for the complete option list. `verify` compares an
archive query against an independently extracted GBZ source oracle, including
tile-local weighted traversal evidence; `--workload` reuses one source load for
a retained JSON query array. `validate` rereads every directory page and
decompresses and structurally checks every physical archive payload. The
planned CLI names `build`, `query`, and `benchmark` remain reserved until there
is a real experiment behind each one.

## Data tiers

1. Tier 0: tiny synthetic graphs/byte sources generated by tests.
2. Tier 1: the 73,920-byte HPRC-derived MICB/KIR3DL1 GBZ fixture fetched from a
   pinned `gbz-base` commit with SHA-256 verification.
3. Tier 2: one HPRC chromosome (opt-in, not bootstrapped yet).
4. Tier 3: whole-genome HPRC v2.1 (opt-in).
5. Tier 4: the 5,008-haplotype 1000GP graph from the GBZ paper (future stress
   target, not a routine development input).

See [`docs/FILE_FORMAT_V1.md`](docs/FILE_FORMAT_V1.md),
[`docs/RESEARCH.md`](docs/RESEARCH.md),
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), and
[`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) before adding a layout experiment.
