# pangenome-range

`pangenome-range` is a systems-research project for testing cloud-native,
range-addressable layouts for pangenome graphs. The target is a static immutable
`.pngr` object on an HTTP origin, object store, or CDN that can answer
interactive genomic-region queries with few byte-range reads and no custom
query server.

The primary public npm package combines browser-safe TypeScript libraries with
an isolated launcher for the native Rust CLI:

- `pangenome-range` and `/reader` provide portable range reading;
- `/viewer` provides the framework-neutral viewer;
- `/node` provides positioned local-file reads;
- `npx pangenome-range` launches an exact-version, platform-specific optional
  native package containing the encoder and research CLI.

The browser exports do not import the launcher, Node built-ins, or native code.
Standalone native archives remain a second installation form through GitHub
Releases, and a future crates.io route is prepared separately. See
[`docs/DISTRIBUTION.md`](docs/DISTRIBUTION.md) for the release topology.

The only current on-disk format is pre-release file-format v1: archive magic
`PNGRNG01` and regional magic `PNGRGN01`. Its complete normative contract is
[`docs/FILE_FORMAT_V1.md`](docs/FILE_FORMAT_V1.md). Historical research objects
are intentionally unsupported and must be regenerated with the current
encoder; npm/Cargo package versions are independent of the file-format number.


## Quick start

Install the complete npm product when consuming a release:

```bash
npm install pangenome-range
npx pangenome-range --version
npx pangenome-range encode input.gbz output.pngr
```

Application imports remain reader/viewer focused:

```ts
import { openPangenome } from "pangenome-range";
import { createPangenomeViewer } from "pangenome-range/viewer";
```

Installing with `npm install --omit=optional` intentionally installs only the
JavaScript libraries. If the CLI is then invoked, its shim names the exact
missing native package and explains how to reinstall it.

From a source checkout:

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

`encode` uses the project-owned disk-backed GBZ reader by default. GBZ record
and sequence sections are streamed into an ephemeral indexed cache below
`--scratch-dir` (or beside the output), read through a fixed 64 MiB block-cache
budget, and removed when the command exits. The final whole-HPRC encode used
11.92 GB of scratch for a 5.49 GB source and peaked at 608,060 KiB RSS instead
of 8,775,928 KiB for the fully loaded baseline. Use
`--source-access loaded` only for the fully deserialized correctness baseline.
Plan scratch capacity before large runs; this cache is separate from the `.pngr`
temporary sibling and does not contain a global occurrence table.

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
Firefox, and WebKit. The private harness runs six explicit cold/warm/query
scenarios with both the default pure-JavaScript zstd decoder and an optional
WASM decoder. CI keeps a Chromium/pure-JS subset; the full matrix remains a
manual gate.

Generate a checksum-bound shared workload and collect Node or real-browser
evidence without overwriting an existing run ID:

```bash
pnpm bench -- workload --file graph.pngr --output workload.json
pnpm bench -- archive --file graph.pngr --workload workload.json --run-id node-run
pnpm bench -- browser --file graph.pngr --workload workload.json --run-id browser-run
pnpm bench -- origin-check --url "$PANGENOME_RANGE_ARCHIVE_URL" \
  --origin "$PANGENOME_RANGE_CORS_ORIGIN"
pnpm bench -- compare --runs node-run browser-run \
  --rust-summary results/browser-run/rust-verification.json
```

Each benchmark run writes configuration, environment, raw requests, per-query
CSV, a machine-readable summary, and `REPORT.md` under `results/<run-id>/`.
Loopback browser timings are local functional evidence, not public-network or
CDN performance.

## TypeScript reader

Browser code can open a URL or a local `File`/`Blob` directly:

```ts
import { openPangenome } from "pangenome-range";

const archive = await openPangenome({
  source: "https://example.test/graph.pngr",
  directoryCacheBytes: 1024 * 1024,
  payloadCacheBytes: 32 * 1024 * 1024,
  extensionCacheBytes: 8 * 1024 * 1024,
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

const genes = await archive.searchLoci({ name: "BRCA", mode: "prefix" });
const overview = await archive.summary({
  sample: "GRCh38",
  contig: "chr6",
  start: 0,
  end: 170_000_000,
  maxBins: 512,
});
console.log(genes.hits, overview.bins);
await archive.close();
```

`queryTiles()` is the streaming primitive. It preserves anonymous haplotype
evidence per source tile; `query()` merges only globally mergeable graph
topology and keeps those tiles alongside the merged graph.

Every newly encoded archive includes a multiscale summary pyramid and a
named-locus index. Summaries are populated from exact tile-local counters.
Named loci are populated only when the encoder receives an explicit GFF3:

```bash
pangenome-range encode graph.gbz graph.pngr \
  --annotations genes.gff3 --annotation-sample GRCh38
```

Without `--annotations`, the named-locus index is present but empty. The
encoder never downloads or guesses an annotation assembly. Both features are
skippable by readers that only need regional graph queries.

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

Create the optional Canvas 2D viewer from its isolated DOM entry point. It
streams `queryTiles()`, cancels stale region requests, and applies node, edge,
and traversal budgets before layout:

```ts
import { openPangenome } from "pangenome-range/reader";
import { createPangenomeViewer } from "pangenome-range/viewer";

const archive = await openPangenome("https://example.test/graph.pngr");
const viewer = createPangenomeViewer(document.querySelector("#viewer")!, {
  archive,
  maxRenderedNodes: 2_000,
  maxRenderedEdges: 4_000,
  maxHaplotypeLanes: 24,
  showRequestTrace: true,
});

await viewer.setRegion({
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
  context: 100,
});
viewer.destroy();
await archive.close();
```

The viewer renders weighted anonymous traversals as tile-local evidence, never
as named individuals or globally stitchable samples. See the [live demo](docs/demo.md)
and [archive hosting contract](docs/HOSTING.md).

Run measurements with `--release`. Large data and benchmarks will remain
opt-in; the default test suite uses synthetic bytes and stays fast.

## Workspace

- `pangenome-range-format`: normative v1 header/root/directory and regional
  codecs, corruption checks, archive validation, `RangeSource`, local
  positioned reads, exact trace metrics, and the network-cost model.
- `pangenome-range-build`: GBZ source adapters, reference anchoring, tile
  selection, the bounded encoder pipeline, build metrics, and candidate-layout
  experiments.
- `pangenome-range-query`: storage-independent graph/tile semantics, comparison,
  and canonical BLAKE3 hashes.
- `pangenome-range-cli`: the direct-write v1 encoder, GBZ inspection, source
  tracing, and retained research benchmarks.
- `packages/browser`: primary public ESM package with isolated reader, viewer,
  Node, and native-CLI launcher surfaces, strict HTTP/Blob/memory/file range
  sources, v1-only archive and record-preserving regional decoding, canonical
  graph assembly, and optional query traces. The launcher lives only under
  `bin/`; browser bundles do not reach it. The bounded progressive Canvas 2D
  viewer lives only under the `/viewer` export.
- `packages/benchmark`: private Node/Playwright benchmark CLI, strict local
  range origin with controlled faults, versioned shared workloads, immutable
  result writer, origin validator, Rust-vs-runtime comparison report, and
  pure-JS/WASM decoder measurement.
- `docs`: the existing research Markdown plus a VitePress site and reader/viewer
  demo deployed under `/pangenome-range/`.

`encode INPUT.gbz OUTPUT.pngr` is implemented with sample/contig/interval and
`--max-chunks` pilot guards, bounded deterministic parallel tile construction,
exact compressed local GBWT records, direct writing, JSON build reports,
periodic progress through input/output checksumming, opaque source-loading
heartbeats, coordinate-based encoding, and full archive validation. Validation
snapshots include entry/page/payload counts, percent, throughput, elapsed time,
and ETA before the atomic rename. It defaults to available parallelism capped
at eight workers and a 256 MiB raw/compressed queue. An interactive terminal
gets readable five-second progress by default; redirected output stays quiet
unless `--progress plain` or `--progress json` is selected. The cadence is
configurable with `--progress-interval-seconds`. Run
`pangenome-range help` for the complete option list. `verify` compares an
archive query against an independently extracted GBZ source oracle, including
tile-local weighted traversal evidence; `--workload` reuses one source load for
a retained JSON query array. `validate` rereads every directory page and
decompresses and structurally checks every physical archive payload with the
same periodic validation progress. The
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
