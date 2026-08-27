# Optimization log

This log records failed or incomplete performance experiments as first-class
evidence. A run belongs here when it changes what we believe about the encoder,
even if it never produces a candidate archive.

## 2026-08-25: HPRC v2.1 whole-genome occurrence-index blow-up

### Goal

Exercise the v3 16 KiB/zstd-3 encoder on a real multi-gigabyte source without
building GBZ-base or repeating the layout sweep.

The source was the HPRC v2.1 Minigraph-Cactus GRCh38 GBZ:

- URL: <https://s3-us-west-2.amazonaws.com/human-pangenomics/pangenomes/freeze/release2/minigraph-cactus/v2.1/hprc-v2.1-mc-grch38/hprc-v2.1-mc-grch38.gbz>
- Size: 5,492,627,216 bytes
- SHA-256: `11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`
- Graph: 139,520,381 nodes, 53,150 paths, 464 haplotypes, and 292
  reference-path fragments

The process had a 18,000,000 KiB virtual-memory limit. Source and scratch were
on `/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d`.

### Observed result

The run was intentionally stopped after 47m27s because preprocessing was still
growing and the final archive had not received a single payload byte.

| Measurement | Observed value |
|---|---:|
| Source GBZ | 5,492,627,216 bytes (5.12 GiB) |
| Temporary SQLite occurrence table at stop | 157,105,246,208 bytes (146.32 GiB) |
| Temporary/source ratio at stop | 28.60x |
| Final payload spool | 0 bytes |
| Encoder RSS near stop | 8,417,564 KiB (8.03 GiB) |
| Elapsed time | 2,847 seconds |
| Average temporary-file growth | 52.63 MiB/s |

These are lower bounds on the old design's total cost. The path scan had not
finished, and SQLite had not yet built `visits_by_node`, the second B-tree over
the already enormous `visits` table.

The retained evidence is in
[`results/2026-08-25-hprc-v2.1-grch38-encoder-scale`](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-hprc-v2.1-grch38-encoder-scale/REPORT.md).
The 157 GB temporary table and empty spool were deleted after the measurements
were recorded. The downloaded source GBZ was retained outside the repository.

### Attribution

There are two independent scale costs:

1. **Upstream source loading.** A standalone `inspect` invocation, which does
   not run our encoder, peaked at 8,775,216 KiB RSS while
   `simple_sds::serialize::load_from::<GBZ>` deserialized the complete source.
   This is an upstream `simple-sds`/`gbz` full-load behavior that our encoder
   currently chooses to use.
2. **Our occurrence-index design.** `PathOccurrenceIndex::build` scans every
   path and inserts one SQLite row for every node visit. Each row repeats
   `node_id`, `path_id`, `visit_index`, orientation, start, and end; start/end
   are retained even though only reference visits use them. It is
   single-threaded and does all of this before the encoder writes a chunk.

The v3 spool made our additional memory bounded, but the global row-per-visit
SQLite representation made temporary disk and startup time unacceptable.

### Replacement direction

The next prototype must remove `PathOccurrenceIndex` from the normal archive
build. `gbz-base::Subgraph` already decompresses the selected GBWT node records
and can extract local traversals with `HaplotypeOutput::All`. We currently ask
for `HaplotypeOutput::None`, discard that local information, and reconstruct
named paths using the global SQLite table.

The replacement should:

1. Extract topology and haplotype traversals only for the active chunk.
2. Encode those local traversals immediately and release them before advancing.
3. Keep named reference identity and coordinates in the arithmetic manifest.
4. Avoid any source-global occurrence table or sort before the first chunk.

There is a semantics decision to make explicit. The upstream local extractor
preserves traversal multiplicity but exposes non-reference paths anonymously.
The pinned `gbz` 0.7.0 code and current upstream Rust implementation retain
the GBWT document-array samples as opaque bytes and do not expose a locate API,
so a local GBWT occurrence cannot currently be mapped back to its true path ID
without an auxiliary structure.
If browser rendering only needs local haplotype shapes and counts, that is the
preferred compact representation. If per-sample identity is required, add a
separate compact identity sidecar: dictionary-code names once, store integer
identifiers only, delta-code visits, and keep coordinates only for reference
paths. Do not return to a general SQLite row per visit.

### Acceptance gates before another whole-genome run

- MHC canonical correctness remains true for 1 kb, 10 kb, 100 kb, and 1 Mb
  queries.
- The encoder writes its first payload chunk without a source-global path scan.
- Temporary occurrence-index bytes are exactly zero.
- Peak additional encoder RSS remains bounded by the configured raw chunk,
  compression buffers, and small lookup caches; report source-load RSS
  separately.
- Progress reports distinguish source load, path-index construction, chunk
  selection, path extraction, encoding, compression, and final assembly.
- The full HPRC run completes under an explicit scratch budget and preserves
  the selected path-identity semantics.

Reducing the 8.4 GiB source-load RSS is a separate track. Candidate approaches
are a range/memory-mapped GBZ reader, consuming an already-built GBZ-base
database, or upstream locate/cached-GBWT support. None of those should block
removing our 157 GB temporary occurrence table first.

## 2026-08-25: v4 tile-local replacement accepted on MHC

Archive v4 removes `PathOccurrenceIndex` and the normal `rusqlite` dependency.
Each active interval is extracted with `HaplotypeOutput::Distinct`, converted
to one real reference walk plus anonymous weighted local traversals, encoded,
and released. Occurrence-index bytes and wall time are exactly zero.

The focused 1 kb adapter comparison found that `All` emitted 9 anonymous
traversals / 27 node visits / 3,343 JSON bytes, while `Distinct` emitted 2
traversals with total weight 9 / 27 weighted visits / 2,193 bytes. Aggregating
`All` by exact oriented traversal equaled `Distinct`. The production-shaped
first 16 KiB tile showed the same equivalence with 9 versus 6 emitted
traversals and 504 weighted visits.

The retained MHC v4 smoke wrote its first payload in 11.282 ms, completed a
3,194,336-byte archive in 1,660.006 ms, used 3,153,203 bytes of payload-spool
scratch, and passed 40 graph-oracle queries plus fresh per-tile weighted checks.
The archive selected multiple chunks for the 1 Mb workload. The whole-process
peak was 916,932 KiB, but includes source load, GBZ-base construction, and query
oracles; it is not an encoder-only RSS attribution.

The next highest-information optimization is removing the payload spool by
reserving/backfilling the directory and appending compressed payloads directly
to the temporary final archive. Source-wide GBZ deserialization remains a
separate upstream limitation.

## 2026-08-25: direct writer and bounded pilots accepted

Archive v4 now reserves its fixed directory pages in a temporary final archive,
appends compressed payloads directly, backfills pages/header, validates every
physical payload through the Rust decoder, fsyncs, and atomically renames. The
payload spool, second full-file copy, and global pending-entry sort are gone.
Failure cleanup is the default; `--keep-partial` is explicit.

The release MHC archive remained exactly 3,194,336 bytes with SHA-256
`119f8e15a0681bd4418ba0eba71c590ca2b6dfc79c1625243bde05fa1358d89a`
for both one and four compression threads. A regression test also exercises a
four-chunk build at both thread counts and requires byte identity. The retained
single-config smoke passed all 40 graph-oracle queries at six coalescing gaps
(240 measurements) and freshly checked 4,338 selected tile payloads.
The preceding spool smoke used the same fixed-seed queries and passed the same
source-oracle gates, establishing decoded equivalence; its archive hash was not
retained, so old/new byte identity is not claimed retroactively.

Release standalone construction measurements were:

| Phase | legacy occurrence-index + spool | local haplotype + spool | local haplotype + direct writer | direct writer + 4 threads |
|---|---:|---:|---:|---:|
| Construction wall | >2,847,000 ms; stopped | 1,660.006 ms | 1,754.494 ms | 1,808.041 ms |
| First payload after build start | none | 11.282 ms | 11.222 ms | 14.686 ms |
| Occurrence scratch | 157,105,246,208 B and growing | 0 B | 0 B | 0 B |
| Payload-spool scratch | 0 B at stop | 3,153,203 B | 0 B | 0 B |
| Final-copy phase | never reached | present, not separately timed | 0 ms | 0 ms |
| Compression wall | never reached | 50.895 ms | 54.149 ms | 26.521 ms |
| Peak queued raw / compressed | not bounded | 565,982 / 51,526 B | 565,982 / 51,526 B | 1,579,650 / 157,258 B |
| Archive SHA-256 | none | not retained | `119f8e15...d89a` | `119f8e15...d89a` |

Four compression workers halved compression wall time but made total MHC
construction 3.1% slower in the paired final run because ordered GBZ extraction/materialization
dominates. `--threads 1` therefore remains the default; bounded parallelism is
available for inputs where compression is material, not claimed as a universal
speedup.

A controlled 32-chunk release experiment compared a new `Subgraph` per interval
with sliding reuse. Exact emitted JSON hashes matched for every interval; reuse
took 33.001 ms versus 38.784 ms (1.175x). Sequential extraction now reuses the
record allocation. A safety-limited topology-only preflight runs once per fixed
directory bucket, splits clearly oversized parents early, and retains exact
post-materialization size checks for every candidate.

The retained HPRC source was present, so a bounded `GRCh38#chr6 --max-chunks 2`
pilot was run rather than a whole genome. It produced a 4,644-byte archive with
the same SHA-256 across the before/after filtering runs. Moving the sample and
contig filter ahead of reference-length traversal reduced manifest discovery
and time to first payload after build start from 15,722.862 ms to 445.932 ms.
The final run used zero occurrence bytes, zero spool/scratch bytes, a 4,266-byte
provisional temp prefix, and peak queued raw/compressed bytes of 19,282 / 194.

The final report hashed the 5,492,627,216-byte source in 20,824.260 ms, loaded it
in 15,304.074 ms, built the compact reference index in 15,533.393 ms, and peaked
at 8,775,512 KiB RSS. Total encode-start to first payload was 52,107.791 ms.
This confirms that
the next scale bottleneck is lazy/memory-mapped source access; it is not evidence
of remaining encoder scratch or a reason to recreate a global occurrence index.

## 2026-08-25: record-preserving regional payload accepted at whole-source scale

The monitored materialized-path writer later reached only 70,148 bp/s at
0.2148% and projected 84,556 seconds (23.49 hours). A larger interval did not
amortize the work: a 4 Mb upstream supertile exceeded 12.5 GiB RSS and was
stopped after 82 seconds, while a 256 KiB supertile took roughly 17 seconds.
Large intervals increase the number and length of distinct traversals that must
be materialized and sorted. Supertile batching is rejected.

The accepted replacement treats the GBWT records as the compressed local
haplotype evidence already present in the source. Each tile selects topology
with `HaplotypeOutput::None`, copies exact compressed records and sequences,
stores topology edges separately, and retains one real reference occurrence
anchor. The reader reconstructs and weights paths only for queried tiles. An
eight-worker full-source record-copy kernel covered 5,944,255,022 reference
bases in 156.733 seconds before archive compression/writing was implemented.

The completed product run used the then-current record-preserving regional
payload, 16 KiB base windows,
zstd-3, eight bounded workers, and a 256 MiB queue cap. It wrote and validated
an 8,828,856,533-byte archive in 475.810 seconds; the whole command including
source and output SHA-256 passes took 557.55 seconds. Payload processing reached
23,008,155 bp/s. This is 151.65x faster end-to-end and 177.71x faster for
construction than the stopped 23.49-hour estimate, clearing the requested 100x
large-input gate.

The run retained 363,105 payloads, 79 adaptive splits, a 47,376,617-byte index,
35,562,073 peak pending raw+compressed bytes, 8,776,260 KiB peak RSS, and zero
occurrence-index, spool, or scratch bytes. Archive validation was the largest
remaining construction phase at 202.109 seconds. Source load remains the cause
of the high RSS; validation and archive size are now higher-information targets
than GPU/SIMD path materialization. Exact commands, provenance, checksums, and
limitations are retained in
`results/2026-08-25-record-preserving-v4/REPORT.md`.

A separate post-build source-oracle query checked seven archive tiles for
`CHM13#chr1:1,000,000-1,100,000`, including weighted anonymous traversals, and
matched canonical hash
`cbf983e845fcd6adcb1504089aba3c80fae85cd0c3998bcc90ba02f8fac8c5b4`.
That semantic check is intentionally distinct from the full structural payload
validation performed before rename.

The 100x change came from changing the unit of work, not from a faster sort.
The rejected writer repeatedly expanded every selected GBWT occurrence into
explicit local paths and then sorted/deduplicated those materialized paths for
each tile. Population-scale occurrence count and path length made that work
explode, and larger supertiles made it worse. The accepted record-preserving
design instead copies the source's already compressed GBWT record bytes,
forward sequences, and canonical topology, plus one real reference occurrence
anchor. Path reconstruction and
weight aggregation move to the few tiles selected by a query. Eight bounded
workers construct deterministic ordered tile batches and compress them while
the direct writer appends to the atomic temporary archive. No global visit
index, payload spool, second copy, or source-global sort remains.

## 2026-08-25: full validation and browser HTTP range path accepted

The completed archive was reread independently after construction. Its
SHA-256 remained
`f9966387ae140607017d45d5c9a2923ac428682a1a1331865773b87729709066`.
`pangenome-range validate` checked 292 manifests, 11,559 directory pages, and
all 363,105 physical payloads (35,747,140,299 uncompressed bytes) in 169.418
seconds. A retained nine-query source-oracle workload then passed 9/9 canonical
graph comparisons and 58/58 exact weighted tile comparisons across CHM13,
GRCh38, chromosomes 1/6/19/X, and fragment/terminal boundaries.

The TypeScript product path at that stage included strict `HttpRangeSource`,
bootstrap/root parsing, arithmetic fixed-page lookup, byte-bounded caches,
pure-JavaScript zstd decompression, and regional-v4 decoding. A synthetic
cross-origin origin passed in Chromium, Firefox, and WebKit. Chromium then
queried the unchanged 8.23 GiB archive over real HTTP `206` responses: 11
requests, 294,190 fetched bytes, seven tiles, 8,592 tile nodes, and 2,196
weighted traversals in 1.238 seconds on loopback. At that stage every response
carried exact range headers and a stable ETag, and the reader rejected every
`200` fallback. This was accepted functional range evidence, not a
public-network latency benchmark.

## 2026-08-25: TypeScript range reader and conformance matrix accepted

That historical public-reader tranche implemented the complete then-retained
format matrix, typed-array decoding, exact canonical merge/hash behavior,
byte-bounded directory and compressed-payload caches, and one parallel
coalesced payload round. Anonymous traversals remained tile-local.

The Node MICB/KIR3DL1 integration records exact request plans. MICB requires two
GETs and 37,797 bytes; KIR3DL1 requires three GETs and 68,619 bytes. Both match
the Rust oracle hashes through HTTP, Blob, and positioned file sources. A
synthetic archive requires one `HEAD` plus three GETs / 20,604 bytes in each of
Chromium, Firefox, and WebKit. The reader bundle is 138,924 bytes raw and 31,195
bytes gzip in the final gate, below the explicit 160 KiB / 50 KiB budgets.

During the three-engine gate, native browser `fetch` exposed a receiver-binding
bug in the optional `HEAD` path that Node did not reproduce. Calling the stored
fetch implementation as an unbound function restored `HEAD` and `If-Range` in
all engines; a receiver-sensitive unit test retains the regression. Incorrect
`200` responses are still rejected by default, with whole-object acceptance
available only below an explicit caller-provided byte cap. This tranche is
functional and conformance evidence, not a public-network latency result.

## 2026-08-25: pre-release format identity reset to v1

The project remained unreleased, so the active record-preserving layout was
reset to the single public identity `PNGRNG01` / `PNGRGN01`. Named-path and
materialized-weighted compatibility encoders, decoders, public types, fixtures,
and dispatch branches were deleted. Rust and TypeScript now accept only the
current v1 bytes and fail closed for other magic/version pairs.

This was a format/specification cleanup, not a performance optimization. The
record-preserving field order, bounded direct writer, arithmetic directory,
and tile-local reconstruction algorithm did not change. Existing benchmark
sections above retain the identifiers used when those measurements were
captured. Their objects are historical and must be regenerated before use with
the current readers. The authoritative current contract is
[`FILE_FORMAT_V1.md`](FILE_FORMAT_V1.md), and the conformance directory now
contains only current v1 fixtures.

## 2026-08-25: optional browser WASM decoder lifecycle

The first full browser benchmark implementation initialized
`@bokuweb/zstd-wasm` separately for each archive-reader instance in one page.
The library owns singleton module state: initial queries decoded correctly, but
later warm/reused scenarios failed with zstd error code `-72`. Reinitialization
is rejected.

The accepted benchmark adapter initializes the WASM module once per page and
creates lightweight `ChunkDecompressor` wrappers over that initialized module.
Separate archive readers and cache scenarios remain isolated without resetting
the decoder runtime. The default reader is still pure-JavaScript `fzstd`; WASM
is optional and must be evaluated with initialization, asset, memory,
per-chunk, whole-query, and correctness evidence rather than steady-state speed
alone.

## 2026-08-26: validation progress closes the silent-tail gap

The current-v1 whole-source build made the CLI ergonomics failure measurable:
coordinate-based payload progress reached 100%, then the full structural
validation ran for 240.736 seconds after emitting only one phase marker. The
encoder was healthy, but a human operator had no evidence that it was still
advancing.

The accepted CLI progress contract now covers input/output checksum passes,
opaque GBZ-load/path-index heartbeats, coordinate-based payload construction,
and entry-based structural validation. Validation snapshots report directory
entries/pages, unique physical payloads, compressed bytes reread, percentage,
rate, elapsed time, and ETA at the configured cadence. Interactive terminals
select readable plain progress automatically; newline-delimited JSON remains
available for monitors. This is an observability repair, not an encoder-speed
claim, and it does not change archive bytes or validation semantics.

## 2026-08-26: current-v1 whole-genome encode and local viewer path accepted

The HPRC v2.1 Minigraph-Cactus GRCh38 source was regenerated with the current
`PNGRNG01` / `PNGRGN01` encoder so the TypeScript reader and viewer could be
tested against supported bytes rather than the incompatible historical
research object. The release binary used 16 KiB windows, zstd-3, eight bounded
workers, a 256 MiB queue cap, and
`anonymous-distinct-weighted-tile-paths` semantics:

```text
pangenome-range encode hprc-v2.1-mc-grch38.gbz \
  hprc-v2.1-mc-grch38-v1-t8-zstd3.pngr \
  --window-size 16384 --codec zstd-3 \
  --haplotypes anonymous-distinct-weighted-tile-paths \
  --threads 8 --max-queued-bytes 268435456 \
  --progress json --progress-interval-seconds 5 --report REPORT.json
```

### Current-v1 whole-source encoder result

| Measurement | Current-v1 result |
|---|---:|
| Source GBZ | 5,492,627,216 B (5.115 GiB) |
| Source SHA-256 | `11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b` |
| References / reference bases | 292 / 5,944,255,022 |
| Archive | 8,828,788,418 B (8.222 GiB; 1.607389x source) |
| Archive SHA-256 | `9dec2631107557bebc0cef671c72e2ee232f7ae8aa1cd6c7ec3ce3706176b80d` |
| Directory entries / pages / adaptive splits | 363,105 / 11,559 / 79 |
| Index / compressed payload | 47,376,617 / 8,781,411,801 B |
| Source checksum / load / compact path index | 20.828 / 15.445 / 14.842 s |
| Time from encode start to first payload | 66.740 s |
| Payload pipeline | 296.047 s |
| Construction including validation | 552.565 s |
| Structural validation inside construction | 240.736 s |
| Terminal-observed whole command | approximately 641.0 s (10m41s) |
| Processing throughput over construction | 10,757,563 reference bp/s; 657 chunks/s |
| Peak RSS | 8,776,204 KiB (8.370 GiB) |
| Peak queued raw / compressed / total | 29,440,657 / 6,121,417 / 35,562,074 B |
| Occurrence index / payload spool / scratch | 0 / 0 / 0 B |

The JSON report directly accounts for 603.680 seconds through the end of
construction when source checksum, source load, and compact path-index time are
included. The terminal session spanned about 641.0 seconds; the approximately
37.3-second remainder includes the output SHA-256 pass and CLI overhead, which
this report schema does not time separately. The 800.032-second subgraph
selection and 222.236-second materialization counters are aggregate worker
milliseconds, not additional elapsed phases.

After validation-progress reporting was added, an independent `validate` pass
reread all 11,559 directory pages and all 363,105 physical payloads, including
35,747,140,299 uncompressed bytes, in 180.879 seconds. A separate source-oracle
verification for `CHM13#chr1:1,000,000-1,100,000` passed graph correctness and
all 7/7 tile-local haplotype comparisons with canonical hash
`b191be02fc2a9556349d8b5b97b268c90c579b1c275cc600355bfaae5b499473`.
Structural validation and semantic verification remain separate gates.

### Real local HTTP range and viewer result

The same 8.222 GiB archive was served from the external SSD with the benchmark
package's strict path-backed range origin and opened by the VitePress
development viewer through its configured external-archive URL. `origin-check`
passed size, stable content-addressed ETag, identity encoding, `no-transform`,
CORS/preflight, exposed headers, exact `Content-Range`, and multiple sampled
`206 Partial Content` reads.

The first viewer load queried
`CHM13#chr1:1,000,000-1,100,000` with 100 bp context. The application made a
one-byte `0-0` size-discovery GET followed by this traced query plan:

| Layer | Inclusive byte range | Bytes | Local origin elapsed |
|---|---:|---:|---:|
| Bootstrap | `0-16383` | 16,384 | 0.6 ms |
| Root tail | `16384-30952` | 14,569 | 0.5 ms |
| Directory | `35049-43240` | 8,192 | 0.4 ms |
| Coalesced payload | `50914188-51169240` | 255,053 | 1.3 ms |

The query trace therefore contains four reads, four dependency rounds, and
294,198 unique bytes with zero duplicate bytes. Including size discovery, the
browser fetched 294,199 bytes, or 0.00333227% of the archive. Every application
GET was an exact `206`; there was no full-object `200 GET`. Origin elapsed times
measure the local Node/file response path with uncontrolled OS cache state, not
public-network latency or a cold-SSD benchmark.

The configured-archive open took 27.3 ms, query wall time was 1,678.3 ms, and
the complete UI action took 1,717.3 ms. The reader selected seven tiles and the
viewer observed 8,592 decoded tile-node occurrences, 11,908 decoded edges, and
2,196 weighted local traversals. Rendering correctly applied its explicit
budget: 2,000 nodes, 2,759 edges, and 24 traversal lanes. The browser result
produced the same canonical hash as the Rust source-oracle verification, and
the page reported no console warnings or errors.

The trace also reported 1,352.8 ms of regional decode work and 3,696.4 ms for
decompression. The decompression value is not an elapsed phase and must not be
added to query wall time: current per-tile timers span suspended promises while
other tile work advances, so their accumulated durations overlap and can exceed
the 1,678.3 ms end-to-end wall clock. Until that instrumentation is repaired,
the defensible conclusion is that range transfer is already small and local
origin service is sub-millisecond to low-millisecond, while browser
decompression/decode/reconstruction dominates the remaining query wall. A
public origin benchmark and corrected non-overlapping phase timings are the
next gates before choosing JavaScript, WASM, SIMD, or worker optimizations.

## 2026-08-26: release-candidate integrity and bounded validation accepted

This pre-stable tranche changed v1 directory entries from 40 to 56 bytes by
placing BLAKE3-128 over each exact encoded regional payload in the arithmetic
directory. It also assigned header bytes 48..63 to an optional bounded extension
directory. Both decisions have ADRs; there is no old-research-archive decoder.

The integrity placement study scanned the accepted whole-HPRC archive:

| Measurement | Result |
| --- | ---: |
| Physical payloads | 363,105 |
| Encoded payload bytes | 8,781,411,801 |
| Maximum entries in one 4 KiB page | 52 |
| Directory scan | 81.316 ms |
| Payload reads | 3,915.411 ms |
| BLAKE3 | 2,334.885 ms / 3,586.7 MiB/s |

The 128-bit directory placement reduced theoretical page capacity from 102 to
72, but no observed page exceeded 52. It therefore modeled zero index/archive
growth and zero extra page reads, while detecting corruption before
decompression. Header placement could not do that; an extension table added
5.8-8.7 MB and at least one uncached lookup.

The default atomic gate is now `standard`: validate directory/offset structure,
deduplicate exact physical ranges, verify BLAKE3-128, decompress exactly, and
decode the regional structure. `full` additionally reconstructs every physical
tile traversal. Both use byte-bounded workers; aggregate worker milliseconds
are separate from wall time.

On MHC, standard validation measured 85.550, 44.236, 22.534, and 12.049 ms at
1, 2, 4, and 8 workers. A new same-source/options/host whole-HPRC run then
produced exactly the prior archive and index byte lengths:

| Measurement | Previous v1 | Release candidate |
| --- | ---: | ---: |
| Archive | 8,828,788,418 B | 8,828,788,418 B |
| Index | 47,376,617 B | 47,376,617 B |
| Construction including validation | 552.565 s | 354.481 s |
| Pre-rename validation | 240.736 s | 28.123 s |
| Final output SHA-256 | not separated | 32.520 s |
| Whole command | about 641 s | 438.720 s |
| Peak RSS | 8,776,204 KiB | 8,775,928 KiB |

The candidate archive SHA-256 is
`76ae6616d296af1c270420ecbaa1fdb1dfa80f28645d72df589a31d2f0f0121e`.
All nine retained source-oracle queries passed graph and tile-local haplotype
comparison. One- and four-worker MHC encodes remained byte-identical at SHA-256
`164d18c254cae1e52bfed5a6cd53ea9d48c8d14ab50dbcc85d5e3b54f5569c70`.

Source access remains the independent limitation. The new `PangenomeSource`
seam and memory preflight report `fully-loaded-gbz` / unbounded access. A
two-chunk HPRC pilot still peaked at 8,776,080 KiB. This rejects any claim that
filtering makes source memory bounded and leaves a whole 1000GP attempt
unauthorized pending lazy/mmap upstream work.

## 2026-08-26: project-owned disk-backed GBZ source accepted for bounded pilots

The production encoder no longer uses `gbz-base::PathIndex`,
`gbz-base::Subgraph`, or `gbz-base::GBZRecord`. Those remain research/oracle
tools only. `SourcePathIndex` now owns sparse real-reference samples and
`LocalSubgraph` owns interval walking, context expansion, topology, and exact
packed-record retention. The loaded adapter and disk adapter produced identical
MHC archives at one and four workers:

```text
164d18c254cae1e52bfed5a6cd53ea9d48c8d14ab50dbcc85d5e3b54f5569c70
```

GBZ/simple-sds stores the complete record and sequence bodies as large
serialized sections. `DiskGbzSource` parses their indices, streams their bodies
to four temporary files, and performs arithmetic offset lookup through four
16 MiB block caches. It never creates a row per haplotype visit or expands full
haplotypes. `--source-access disk` is now the default; `loaded` is retained as
the correctness baseline.

The exact retained HPRC pilot used the same source, filters, archive options,
host, and correctness gate as the release-candidate source pilot:

```text
source:       5,492,627,216 B / sha256 11d6047f...e2b
filter:       GRCh38 chr6, first two 16 KiB chunks
archive:      4,807 B
archive sha:  23342b919d933a19002ac97aac8d8ce3495ed5f4d3fcb27c9e194aeba76f70ca
```

| Measurement | Fully loaded baseline | Disk-backed + 64 MiB cache |
| --- | ---: | ---: |
| Peak RSS | 8,776,080 KiB | 408,376 KiB |
| Whole pilot wall | 51.37 s | 104.68 s |
| Source checksum | included in wall | 20.822 s |
| Source preparation | full load | 26.123 s cache build |
| Reference index | included in wall | 57.647 s |
| Encoder construction | negligible | 9.484 ms |
| Source scratch | 0 B | 11,921,858,427 B |
| Explicit read-cache limit | n/a | 67,108,864 B |

Peak process memory fell by 95.35%, from 8.37 GiB to 0.389 GiB. The cache is
2.171x source size and is removed on exit. The first uncached reader prototype
had the same 407,756 KiB memory peak and exact archive hash, but its one-seek-
per-access reference index took 378.746 s. The fixed 64 MiB block cache reduced
that phase to 57.647 s. Both runs rebuilt fresh cache files; whole-wall
comparison is still affected by ordinary OS source-cache warmth, so no claim is
made from the 48.040 s versus 26.123 s cache-build difference.

On the 4,511,832-byte MHC fixture, disk-backed one-worker construction was
611.312 ms versus 573.678 ms loaded (6.6% slower), while the archive bytes were
identical. This supports the intended trade: a modest construction penalty for
a source working set no longer proportional to the full GBZ body. The compact
simple-sds indices loaded while creating the disk cache still scale with record
and sequence count; the HPRC cache-build peak is the evidence bound, not a claim
of constant memory.

A 1,024-window HPRC chr6 extension then covered 16,777,216 bp, wrote and
validated 1,024 physical payloads in 976.111 ms construction wall, and remained
at 407,880 KiB peak RSS. Its archive was 26,156,797 bytes with SHA-256
`239242b09acc247601ff58829002697cfd74bcebfe1b88bf457512f611604f46`.
The sustained 0.953 ms per entry is close to the accepted whole-HPRC loaded
run's 0.976 ms per entry average. The measurable whole-command penalty is thus
currently concentrated in source preprocessing: 103.113 s for disk-backed
checksum/cache/index versus 51.260 s loaded, about 52 seconds on this host.

The final whole-HPRC run completed from the project-owned path after two more
bounded hill-climbing steps: 16 cache shards removed the single cache lock from
the worker critical path, and a length-only source operation stopped reference
indexing from materializing sequence bodies. It produced the exact retained
8,828,788,418-byte archive, 47,376,617-byte index, and SHA-256
`76ae6616d296af1c270420ecbaa1fdb1dfa80f28645d72df589a31d2f0f0121e`.

| Measurement | Fully loaded release candidate | Final disk-backed |
| --- | ---: | ---: |
| Whole command | 438.720 s | 499.340 s |
| Prebuild | 51.260 s | 95.034 s |
| Construction including validation | 354.481 s | 369.561 s |
| Pre-rename validation | 28.123 s | 32.221 s |
| Output SHA-256 | 32.520 s | 34.493 s |
| Peak RSS | 8,775,928 KiB | 608,060 KiB |
| Source cache | 0 B | 11,921,858,427 B |

This is a 93.07% RSS reduction for a 13.82% whole-wall penalty on the exact
same source, options, and host. Construction itself was 4.25% slower; most of
the remaining cost is the bounded source cache and reference-index prebuild.
The source cache was removed on exit, and occurrence scratch, payload spool,
and general encoder scratch remained zero.

The whole HPRC gate is now complete. A 1000GP whole-source run remains
unauthorized until a representative bounded pilot measures cache expansion,
compact-index RSS, construction throughput, and required disk headroom.

## 2026-08-26: default viewer indexes accepted at whole-HPRC scale

The named-locus and multiscale-summary extension tranche was rerun on the exact
retained HPRC source, host, disk-backed source adapter, eight-worker setting,
256 MiB queue bound, 16 KiB windows, and zstd-3 codec. Source, output, and the
fresh 11,921,858,427-byte ephemeral source cache were on `/dev/nvme0n1`; the
repository was on `/dev/nvme1n1`.

| Measurement | Before viewer indexes | With viewer indexes | Delta |
| --- | ---: | ---: | ---: |
| Archive | 8,828,788,418 B | 8,829,030,376 B | +241,958 B (+0.00274%) |
| Bootstrap/index | 47,376,617 B | 47,376,777 B | +160 B |
| Whole command | 499.340 s | 503.360 s | +4.020 s (+0.81%) |
| Construction including validation | 369.561 s | 357.660 s | -11.902 s |
| Pre-rename validation | 32.221 s | 28.743 s | -3.478 s |
| Peak RSS | 608,060 KiB | 642,220 KiB | +34,160 KiB |

The 160-byte bootstrap growth is exactly two extension-directory entries. The
extension bodies occupy 241,798 encoded bytes and contain 591 summary series
with 8,017 bins. The named-locus descriptor is empty because no GFF3 was
provided. Feature finalization took 188.823 ms. The regional payload version,
directory population, and 363,105 physical payloads did not change.

This single-run comparison does not support claims from the faster construction
or validation subphases; prebuild moved in the opposite direction and the
0.81% whole-wall delta is ordinary system/cache noise. It does show no material
feature regression. Mandatory validation passed all 11,559 directory pages and
363,105 payloads before rename. The retained independent source oracle passed
9/9 graph hashes and 58/58 checked tile-local haplotype sets.

The source oracle is not part of the encoder measurement: its separate process
fully loaded research/oracle structures, took 81.58 seconds, and peaked at
12,973,152 KiB. Encoder peak remained 642,220 KiB (627.2 MiB), 92.68% below the
fully loaded encoder baseline. Exact commands and raw-evidence paths are in
`results/2026-08-26-default-viewer-indexes-whole-hprc/`.

## 2026-08-26: unified rolling workers and source overlap accepted

The committed current-v1 encoder created new scoped native threads for every
eight-tile construction batch and again for each compression batch. The first
attempt only made those workers persistent while retaining the barriers; its
8,192-tile payload phase regressed from 5.721 to 6.663 seconds. A two-pool
pseudo-rolling attempt improved the small pilot but failed the whole-source
gate: payload time rose to 349.205 seconds, whole wall to 530.114 seconds, and
peak RSS to 751,812 KiB. Persistent construction and compression pools were
competing for the same eight physical cores and retaining a second set of
thread-local allocator state.

The accepted design uses one bounded pool of exactly `--threads` workers for
both job types. Construction maintains a rolling window and may complete out
of order, but the coordinator processes results and adaptive splits in strict
coordinate order. Full compression batches enter the same FIFO pool, so no
second worker set exists. The sixteen-window determinism test crosses multiple
worker waves and remains byte-identical between one and four workers.

Source SHA-256 also now overlaps the already-required disk-cache build. Schema-6
reports retain both worker-wall times and add their non-overlapping combined
critical-path wall. Whole-output SHA-256 was not overlapped with validation: a
read-only experiment kept validation at 30.40 seconds but slowed the competing
hash to 61.49 seconds, providing no critical-path benefit.

The exact same retained source, archive options, host, and storage layout
produced:

| Measurement | Previous current-v1 | Unified workers | Delta |
| --- | ---: | ---: | ---: |
| Archive / index | 8,829,030,376 / 47,376,777 B | identical | 0 B |
| Archive SHA-256 | `0b033255...17293` | identical | byte-identical |
| Whole command | 503.191 s | 409.937 s | -93.254 s (-18.53%) |
| Prebuild | 112.899 s | 79.182 s | -33.717 s |
| Payload pipeline | 328.715 s | 267.234 s | -61.482 s (-18.70%) |
| Construction including validation | 357.660 s | 297.977 s | -59.683 s |
| Pre-rename validation | 28.743 s | 30.454 s | +1.711 s |
| Output SHA-256 | 32.557 s | 32.701 s | +0.143 s |
| Peak RSS | 642,220 KiB | 640,556 KiB | -1,664 KiB |
| Voluntary context switches | 29,711,742 | 19,652,973 | -33.85% |

On the accepted run, the 23.104-second source-checksum worker fit inside the
27.598-second cache-build worker. Path indexing also measured 10.028 seconds
faster, but that single-run movement is treated as ordinary cache/system noise.
The payload improvement reproduced in the bounded pilot (5.721 to 4.545
seconds) and whole source. Mandatory structural validation passed before
rename; occurrence scratch, payload spool, and general scratch remained zero.
Full evidence is retained in
`results/2026-08-26-unified-workers-whole-hprc/`.

## 2026-08-26: GENCODE v50 named-locus policy and whole-HPRC run accepted

The canonical annotation input is the 4,763,975,927-byte uncompressed GENCODE
v50 comprehensive GFF3 for GRCh38.p14 reference chromosomes. It contains
78,733 gene rows, 644,292 transcript rows, and 11.24 million feature rows in
total. The initial unmodified importer bound `GRCh38` and `chr6` correctly but
repeated `gene_name` and `gene_id` on every transcript, exon, CDS, codon, and
UTR. A 6 Mbp chr6 pilot therefore emitted 279,428 search records; exact
`HLA-B` search returned 511 mixed feature hits.

The accepted reference-encoder policy indexes only GFF3 records whose feature
type is exactly `gene`. The same pilot emitted 1,130 records in five pages;
`HLA-B`, its Ensembl stable ID, and `MICA` each returned one correct gene
interval. The file format remains generic and byte-compatible; this is an
encoder selection policy, not a format change.

On the exact whole-HPRC source, options, host, and storage used by the unified
worker baseline, the corrected run emitted 157,466 name/stable-ID records in
612 pages. All 78,733 CHR genes are represented by their GENCODE symbol and
stable ID. The archive grew by 3,719,573 bytes (+0.0421%), while the arithmetic
directory/index remained 47,376,777 bytes. Named-locus construction added
23.122 seconds to writer finalization. Whole wall measured 420.848 seconds
versus 409.937 seconds (+10.911 seconds); faster payload and validation phases
in the annotation run are treated as ordinary run-to-run variation rather than
annotation savings. Peak RSS was 695,816 KiB versus 640,556 KiB.

Cold exact and prefix searches read the 61,145-byte descriptor and one
6.9-8.0 KiB leaf in two dependency rounds. `TERT`, `HLA-B`, `MICA`, `BRCA2`,
`TP53`, `BRCA1`, and the BRCA1 stable ID matched their independent GFF3 rows;
the `BRCA` prefix returned BRCA1, BRCA1P1, and BRCA2. Five gene-region graph
hashes and all 23 selected tile-local haplotype hashes matched the prior
source-oracle-qualified archive. Mandatory structural validation passed before
rename. Full evidence is retained in
`results/2026-08-26-gencode-v50-whole-hprc/`.
