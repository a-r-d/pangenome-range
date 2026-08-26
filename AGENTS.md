# AGENTS.md

## Mission

`pangenome-range` is a systems-research project for building and measuring a
cloud-native, range-addressable representation of pangenome graphs.

The intended deployment is a static immutable `.pngr` object hosted on an
ordinary HTTP origin, object store, or CDN. A browser should answer a genomic
region query with a small bootstrap, a small directory lookup, and one parallel
payload round. No custom query server should be required.

The project has four product surfaces:

1. **Native Rust encoder and research CLI** — converts GBZ input into `.pngr`,
   verifies semantics, and records construction/query evidence.
2. **Browser TypeScript package** — opens local files or remote static objects,
   performs HTTP range reads, decodes `.pngr`, and returns typed query results.
3. **Viewer library** — renders decoded regional graph data without owning the
   storage or transport layer.
4. **Documentation/demo site** — a GitHub Pages site under `docs/` that imports
   the public browser package and proves real range access.

The primary npm package distributes the portable reader/viewer and an isolated
Node executable shim. Exact-version optional platform packages contain the
native Rust encoder/CLI; ordinary reader and viewer imports must never load the
shim or native code. Standalone GitHub Release binaries and a future Cargo
installation remain additional distribution forms.

---

## Read before changing code

Read these files before touching encoder, format, query, or benchmark code:

- `README.md`
- `docs/ARCHITECTURE.md`
- `docs/FORMAT_GOALS.md`
- `docs/FIXED_WINDOW_ARCHIVE.md`
- `docs/BENCHMARKS.md`
- `docs/OPTIMIZATION_LOG.md`
- `docs/UPSTREAM.md`
- the most relevant retained `results/*/REPORT.md`

The optimization log is not optional. It records failed designs that must not be
reintroduced under another name.

---

## Current status and known failure

The current pre-release file-format v1 implementation demonstrates:

- a 64-byte header;
- a small bootstrap/root;
- arithmetic lookup into fixed 4 KiB leaf pages;
- independently compressed regional payloads;
- adaptive splitting;
- exact packed local GBWT records reconstructed on read;
- a reusable range reader and directory cache.

The former global SQLite occurrence table is a rejected architecture. On the
recorded HPRC v2.1 run it reached roughly 157 GB after 47 minutes before the
first payload. It is not part of the current encoder and must not return.

The upstream `gbz`/`simple-sds` path also currently deserializes the complete GBZ
into memory. That is a separate problem. Remove the global occurrence table
first; investigate lazy/memory-mapped source access independently.

---

## Non-negotiable invariants

### Correctness

- A performance result is invalid unless its declared semantics match an
  independent source oracle.
- Never improve speed by silently dropping node sequences, edges, orientation,
  path multiplicity, reference coordinates, weights, or provenance.
- When semantics change, change the canonical model, format version, docs, and
  tests explicitly.

### Scale

- Do not create a row, object, or fixed-width tuple for every global haplotype
  node visit. Population-scale graphs can contain trillions of visits.
- Do not expand all haplotypes into explicit full sequences.
- Normal archive construction must not perform a source-global path scan before
  the first payload solely to build an auxiliary occurrence index.
- Additional encoder memory must be bounded by active source state, one or a
  small bounded number of regional chunks, compression buffers, and compact
  metadata.

### Static-object architecture

- The served representation must work through standard HTTP byte ranges.
- No database server, GraphQL service, bespoke query daemon, or required proxy.
- All archive offsets and lengths are unsigned 64-bit values.
- JavaScript must represent file offsets internally as `bigint`; never truncate
  them to 32-bit values or unsafe IEEE-754 integers.

### Determinism

- The same input, options, dependency lockfiles, and encoder version must produce
  byte-identical output unless a documented format change says otherwise.
- Parallel execution must not make archive ordering nondeterministic.
- Retained benchmark query sets use fixed seeds.

### Data hygiene

- Do not commit multi-gigabyte GBZ, SQLite, `.pngr`, scratch, or benchmark cache
  files.
- Large source and output objects belong outside the repository.
- `results/` may retain compact configuration, raw query tables, summaries, and
  reports. Do not rewrite old retained evidence in place.
- Record source URL, byte length, checksum, data-use constraints, and upstream
  version for every published benchmark.

---

## Haplotype semantics

This is the most important modeling boundary in the project.

The current v1 model preserves a real reference path and reconstructs exact
anonymous tile-local traversal multiplicity from packed local GBWT records. It
does not identify arbitrary non-reference paths by a true global sample/path ID.

Therefore:

- **Reference paths** must retain real identity, coordinates, orientation, and
  fragment information.
- **Anonymous local haplotypes** must never be assigned fake sample names or fake
  global path IDs.
- A scalable archive may store distinct tile-local traversals with integer
  weights. Such traversals are local evidence, not globally stitchable samples.
- For a query spanning multiple chunks, graph topology may be merged globally,
  but anonymous haplotype evidence remains associated with its source tile unless
  the format contains a real, tested continuation identity.
- Do not double-count overlapping halo traversals across chunks.
- If stable sample identity is later added, it must be a separate explicit mode
  or sidecar with its own byte cost, construction cost, query cost, and format
  version. Do not reintroduce a row-per-visit index.

The only serialized semantic label is
`anonymous-distinct-weighted-tile-paths`. `anonymous-all-tile-paths` may exist
only as an internal source-oracle mode; it is not a second file-format payload.

A reader must expose the semantic label to callers.

---

## Repository ownership and dependency direction

Keep the Rust dependency direction acyclic:

```text
pangenome-range-cli
        -> pangenome-range-build
        -> pangenome-range-query
        -> pangenome-range-format
```

Suggested responsibilities:

- `pangenome-range-format`
  - byte-range source abstractions;
  - archive constants and versioned binary codecs;
  - header/root/directory/regional payload parsing;
  - format errors and corruption checks;
  - no GBZ dependency.
- `pangenome-range-query`
  - storage-independent query types;
  - canonical graph and tile-local haplotype semantics;
  - canonical hashing and mismatch reporting.
- `pangenome-range-build`
  - GBZ/GBZ-base adapters;
  - chunk selection/materialization;
  - encoder pipeline and build metrics;
  - experimental layouts.
- `pangenome-range-cli`
  - user commands, argument parsing, progress, reporting;
  - no binary format logic.

Do not perform a giant mechanical Rust refactor in the same change as a major
semantic or performance repair unless the refactor is required for correctness.

Target JavaScript layout:

```text
packages/
  browser/       # primary package: browser-safe exports + isolated CLI shim
  benchmark/     # private Node/Playwright benchmark tools
docs/            # existing Markdown + VitePress site/demo
```

---

## Native CLI contract

The binary name remains:

```text
pangenome-range
```

Target public commands:

```text
pangenome-range encode <input.gbz> <output.pngr> [options]
pangenome-range inspect <input.gbz|input.pngr>
pangenome-range query <input.pngr> --sample ... --contig ... --start ... --end ...
pangenome-range verify <input.pngr> --against <input.gbz> [workload]
pangenome-range benchmark ...
pangenome-range fixtures export ...
```

Research-only commands may remain, but clearly label them and do not make a
known-pathological mode the normal path.

Encoder output must be written to a temporary sibling and atomically renamed on
success. Partial output must not masquerade as a valid archive.

---

## Encoder implementation rules

- Remove `PathOccurrenceIndex` from the normal encoder path.
- Use local GBWT/GBZ subgraph extraction for the active chunk.
- Encode and release local traversal data before advancing.
- Preserve reference identity arithmetically/in metadata rather than repeating
  strings per visit.
- Prefer direct append to the final temporary archive with later directory
  backfill over a complete payload spool and second full copy.
- Bound worker queues. Never allow parallel chunk production to retain an
  unbounded corpus of raw payloads.
- Measure source load, path index, selection, haplotype extraction, encoding,
  compression, directory finalization, and total wall time separately.
- Report time to first payload byte and bytes of scratch written before it.
- Add filters such as sample/contig/interval/max-chunks so scale work can be
  tested without repeatedly encoding a whole genome.
- Use a safety limit and/or cheap estimate before fully materializing a huge
  parent region merely to discover it must be split.
- Keep legacy occurrence-index code, if retained at all, behind an explicit
  non-default feature or reproduction command.

Whole-genome runs require explicit disk, memory, and output paths. Do not launch
one as part of ordinary tests or CI.

---

## Format/versioning rules

Current research objects use `.pngr`.

While the project is unreleased and pre-v1, an incompatible change replaces
file-format v1 in place. It requires all of the following in the same tranche:

1. updated v1 magic/layout implementation without a compatibility decoder;
2. updated normative format documentation;
3. a Rust encoder and reader;
4. TypeScript v1 decoding and clear unsupported-version errors;
5. golden binary fixtures;
6. expected decoded JSON/canonical hashes;
7. malformed/truncated/corrupt fixture tests;
8. explicit notes that old research archives must be regenerated.

Package semantic versions are independent of file-format v1. Do not introduce a
v2 compatibility stack before a deliberate stable-format release.

Keep index and payload parsing bounds-checked. Validate counts before allocating.
Reject integer overflow, out-of-file offsets, overlapping invalid sections,
unknown codecs, impossible record offsets, and decompressed-length mismatch.

---

## Primary npm package contract

Working npm package name:

```text
pangenome-range
```

The checked-in package metadata is public, but no workflow may publish until
npm ownership and trusted publishing are deliberately configured. Release
workflows must stage and verify tarballs without publishing by default.

Required subpath exports:

```text
pangenome-range
pangenome-range/reader
pangenome-range/viewer
pangenome-range/node
```

The root export should remain reader-focused. Importing the root or `/reader`
must not pull viewer code, DOM code, or Node built-ins into the bundle.

The package also exposes the `pangenome-range` executable from `bin/`. Its
Node-only shim selects an exact-version platform package, forwards arguments,
signals, and exit status, and reports unsupported or omitted optional packages
without affecting JavaScript imports. Platform packages contain only the
binary, license, README, and metadata; they use `os`/`cpu`/Linux `libc`
restrictions and never use lifecycle scripts, downloads, or local compilation.

Core source interface:

```ts
export interface RangeSource {
  size(signal?: AbortSignal): Promise<bigint>;
  read(
    offset: bigint,
    length: number,
    options?: { signal?: AbortSignal }
  ): Promise<Uint8Array>;
  close?(): void | Promise<void>;
}
```

Required implementations:

- `HttpRangeSource`
- `BlobRangeSource`
- `MemoryRangeSource`
- `TracingRangeSource`
- `FileRangeSource` only from `pangenome-range/node`

Browser code rules:

- ESM-first and tree-shakeable.
- No `Buffer`, `fs`, `path`, or other Node polyfills in browser exports.
- Parse with `Uint8Array`/`DataView` and explicit little-endian reads.
- Use `bigint` for archive offsets.
- Support `AbortSignal` throughout open/query operations.
- Validate `206`, `Content-Range`, response length, and stable object identity.
- If a server ignores `Range` and returns `200`, reject large objects with a
  useful error instead of silently downloading the entire archive.
- Use explicit byte-bounded bootstrap, directory, and optional payload caches.
- Browser HTTP cache behavior is not a substitute for library cache metrics.
- Return typed-array-oriented data, not millions of tiny JavaScript objects.
- Decompression is behind an interface. A pure-JS zstd decoder may be the
  default; optional WASM acceleration must not change semantics.

---

## Viewer rules

The viewer consumes decoded query/tile results. It does not issue raw range
requests or parse archive bytes itself.

- Framework-neutral public API.
- Canvas 2D first; do not create one DOM element per node, edge, or haplotype.
- Separate layout/model code from rendering.
- Support progressive tile arrival.
- Display the active reference interval, node/edge counts, local traversal
  semantics, weights, and request trace.
- Enforce rendering budgets and summarize/sampling behavior explicitly.
- Never imply anonymous tile-local paths correspond to named individuals.
- Provide `destroy()` and remove listeners/workers/resources.

---

## TypeScript and browser benchmark rules

The benchmark package is private and must test both Node and real browsers.

Measure at least:

- bootstrap requests/bytes;
- directory requests/bytes;
- payload requests/bytes;
- dependency rounds;
- unique/duplicate bytes;
- cache hits;
- decompression, decode, merge/layout, and total latency;
- correctness hash;
- cold library cache and warm library cache separately;
- Chromium, Firefox, and WebKit where practical.

A local test server must implement real `206 Partial Content`, CORS, exposed
range headers, ETag, and request logging. Simulated network cost remains useful
for layout search but is never described as browser performance.

Raw benchmark outputs belong under a run directory with config, environment,
queries, summary, and report. Keep source and archive checksums.

---

## Cross-language conformance

Rust is the reference encoder, but not the only source of truth. The documented
format and golden fixtures are the contract.

For every supported version, retain:

- a tiny deterministic `.pngr` archive;
- header/root/directory fixtures;
- at least one raw regional payload;
- its compressed representation;
- expected reference descriptors;
- expected query/tile JSON;
- expected canonical hashes.

Rust and TypeScript must decode the same fixtures and produce matching canonical
results. A format change is incomplete until both sides pass.

---

## Documentation and GitHub Pages

Use the existing `docs/` directory as the documentation source. Prefer
VitePress so the existing Markdown remains first-class and the demo can be
embedded as a component.

Do not commit generated Pages output. Deploy the build artifact with GitHub
Actions.

The demo must import the workspace package through its public exports. It may not
reach into `packages/browser/src` with relative paths.

Until a real large archive is available, use a tiny deterministic generated
fixture. Do not hardcode the future production archive URL.

Use runtime/build configuration such as:

```text
VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL
```

or a small `demo-config.json` containing versioned archive choices.

---

## External archive hosting and Cloudflare Tunnel

A real demo archive will later be hosted on the project owner's own server and
published through a Cloudflare Tunnel. Repository code must treat that URL as
configuration.

The external origin must be validated for:

- `Accept-Ranges: bytes`;
- correct `206 Partial Content` responses;
- correct `Content-Range` and `Content-Length`;
- CORS permission for the GitHub Pages origin (or `*` for public immutable
  archives);
- `Access-Control-Expose-Headers` for `Accept-Ranges`, `Content-Range`,
  `Content-Length`, and `ETag`;
- stable `ETag`;
- `Cache-Control: public, max-age=31536000, immutable` for content-addressed
  archives;
- no transparent compression or transformation of `.pngr` bytes;
- `Cache-Control: no-transform` where appropriate.

Provide a script that probes multiple ranges and fails if the public endpoint
returns the wrong bytes. Do not add a GitHub Pages proxy.

---

## Development commands

Run the complete Rust gate with:

```bash
pnpm check:rust
```

This runs rustfmt verification, all workspace tests, and clippy for all targets
and features with warnings denied.

Rust measurements use release builds:

```bash
cargo run --release -p pangenome-range-cli -- ...
```

Run the JavaScript/TypeScript pre-commit gate after workspace setup:

```bash
pnpm install --frozen-lockfile
pnpm check
pnpm build
pnpm test:browser
```

`pnpm check` (equivalently `npm run check`) runs Biome lint and format
verification, strict TypeScript checking, and the test suites. Use
`pnpm format` to apply Biome formatting.

Default CI must stay fast and hermetic. Large downloads and whole-genome
benchmarks are opt-in.

Use Node.js 24 LTS for the JavaScript workspace unless the repository deliberately
updates its support policy.

---

## Change workflow for agents

1. Inspect the current tree and `git status`.
2. Read the relevant docs and retained result.
3. State the exact hypothesis being tested.
4. Make the smallest coherent tranche that can validate that hypothesis.
5. Add or update correctness tests before claiming performance.
6. Run focused tests, then full Rust/TypeScript checks applicable to the change.
   Before committing, both `pnpm check` and `pnpm check:rust` must pass.
7. Record measurements with exact commands and environment.
8. Update format, architecture, benchmark, optimization, and hosting docs when
   their truth changes.
9. Do not overwrite or delete retained evidence without explicit instruction.
10. End with files changed, commands run, results, limitations, and the next
    highest-information experiment.

---

## Never do these things

- Recreate a global SQLite or flat row-per-visit occurrence table.
- Hide path-identity loss behind synthetic names.
- Merge anonymous paths across tiles as if they were the same individual.
- Use JavaScript `number` for raw file offsets.
- Download an entire multi-gigabyte archive because a range origin is broken.
- Call simulated latency a browser benchmark.
- Commit large research inputs or generated archives.
- Hardcode the future Cloudflare Tunnel URL.
- Change magic/version semantics without golden fixtures and both decoders.
- Add a backend simply because client-side indexing is inconvenient.
- Run a whole-genome experiment in normal CI.
- Optimize before preserving a reproducible before/after measurement.

---

## Definition of done

A tranche is done only when:

- declared semantics are explicit;
- correctness tests pass;
- format/version implications are handled;
- Rust and TypeScript contracts remain aligned when applicable;
- performance evidence is reproducible;
- memory, scratch, and request behavior are reported rather than guessed;
- docs describe the code that actually exists;
- no large untracked artifacts are left inside the repository;
- the final report names remaining limitations honestly.
