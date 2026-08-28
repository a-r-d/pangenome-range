# PPanG rice chromosome 6 acquisition and conversion

Status: complete research corpus; the derived anonymous `.pngr` is publicly
hosted under the confirmed MIT redistribution status.

## Scope

- Official object: `chr06_mc.xg`, discovered from the rendered PPanG page.
- Graph construction: Minigraph-Cactus v2.2.2 as stated by PPanG.
- Coordinate anchor: `NATELBORO.chr06`.
- Curated locus: `Xa7`, reported at `28873554-28874897`.
- `.pngr` semantics: unchanged
  `anonymous-distinct-weighted-tile-paths` production encoder output.
- Published “16 of 113” presence count: external comparison target, not yet a
  correctness assertion for this downloaded path set.
- Large-file workspace:
  `/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/runs/2026-08-28-ppang-rice-chr06`.

No `.pngr` format or production encoder semantic change was made. The archive
uses `anonymous-distinct-weighted-tile-paths`.

## Acquisition

- Official page: <https://cgm.sjtu.edu.cn/PPanG/>
- Discovered object URL:
  <https://cgm.sjtu.edu.cn/PPanG/data/pangenome-graph/chr06_mc.xg>
- HTTP range probe: `206`, `Accept-Ranges: bytes`, ETag
  `"1f365dace-601ed535e1fd4"`, last modified `2023-08-02 09:31:17 GMT`.
- XG: 8,378,505,934 bytes; SHA-256
  `466744e1f7da8f30b34f13f773302d55464fc51ccfe1a53c74d3709228476dae`.
- Download: resumable transfer from `2026-08-28T09:57:56Z` through
  `2026-08-28T11:28:50Z`; checksum recorded at `11:31:13Z`.
- PPanG repository inputs are pinned to commit
  `64f092fb94ef5bf0e16faf131537fac93163c2c8`; exact checksums are in the
  canonical [`data/rice/sources.json`](../../data/rice/sources.json).
- Tool: official vg v1.76.1 release, 55,374,080 bytes, SHA-256
  `87b457fdda6801c9580f79a53c3c0aa502261420abf222920c7222a703fd856b`.

## How the archive was generated

The whole conversion is automated. It needs Linux, Python 3, Node.js 24,
pnpm, the Rust toolchain, and at least 50 GiB free on a scratch volume. From the
repository root:

```bash
export PPANG_RICE_DATA_DIR=/path/to/large/scratch/rice-chr06
export PPANG_RICE_GBWT_BUFFER_MILLIONS=20

scripts/rice/fetch-vg.sh
scripts/rice/fetch-ppang-rice.sh
scripts/rice/build-rice-corpus.sh
```

Those commands perform these steps:

1. Download and checksum the pinned vg v1.76.1 binary.
2. Discover `chr06_mc.xg` from the official PPanG page, download it resumably,
   and verify its byte length and SHA-256.
3. Audit every XG path name, then export the graph to GFA without changing node
   IDs or path names.
4. Split the GFA into 113 biological accession paths and 104,846 verbatim
   `_MINIGRAPH_` generic paths. No synthetic sample identity is assigned to the
   generic paths.
5. Build the two GBWT components with a bounded 20-million-node insertion
   buffer. Accession paths receive their source names plus technical haplotype
   0; `IRGSP-1.0` and `NATELBORO` are marked as references.
6. Merge the GBWT components, combine the result with the original XG to make
   the GBZ, and build its r-index.
7. Compare XG and GBZ graph statistics and SHA-256 hashes for every named path
   sequence. Both comparisons must be empty before continuing.
8. Encode `NATELBORO#0#chr06` with the unchanged production encoder, the
   `anonymous-distinct-weighted-tile-paths` semantics, and the curated Xa7 GFF3
   annotation in `data/rice/`.
9. Validate every archive payload, verify the Xa7 interval against the source
   GBZ oracle, and generate the fixed browser workload.
10. Run the workload in Chromium, Firefox, and WebKit, publish the immutable
    checksum-addressed `.pngr`, and verify the public origin with exact range,
    checksum, ETag, CORS, and cache-policy checks.

`commands.sh` is the exact top-level recipe used for the retained run. The
implementation details and exact encoder flags live in
`scripts/rice/build-rice-corpus.sh`, so this report does not duplicate a long
shell transcript.

## Path and graph audit

The XG contains 20,539,106 nodes, 30,221,815 edges, 210,803,459 bp of
node sequence, and 104,959 named paths:

- 113 biological accession paths match `<accession>.chr06`;
- 104,846 reviewed Minigraph-generated generic paths match
  `_MINIGRAPH_.s<number>`;
- zero names remain unhandled;
- `IRGSP-1.0.chr06` and `NATELBORO.chr06` are both present.

The 6,984,618,682-byte `vg convert -fW` GFA retained node IDs 1 through
20,539,106, all edges, and all 104,959 P-lines, with no W-lines, duplicates,
missing paths, or extra paths.

The named GBZ uses a two-index merge:

1. The 113 accessions become structured sample/`chr06` metadata. Because the
   source has one chr06 path per accession and no haplotype number, the temporary
   construction names append `.0` and explicitly record technical haplotype 0.
   This does not infer any additional individual identity.
2. The 104,846 `_MINIGRAPH_` paths remain verbatim `GENERIC` paths and are not
   represented as rice accessions.
3. `IRGSP-1.0` and `NATELBORO` are the two reference samples.

Final GBWT metadata reports 104,959 paths, 114 stored samples/haplotypes, and
104,847 stored contigs. The extra sample is vg's `_gbwt_ref` sentinel for the
reviewed generic paths; there are 113 biological accession samples. The GBWT
has 209,918 bidirectional sequences. The audit found no missing, unexpected,
duplicated, fragmented, or invented path-cover paths.

Both graph-stat and all-path sequence diffs between XG and GBZ are exactly zero
bytes. The sequence comparison covers 104,959 names and checks each normalized
name, sequence length, and SHA-256.

Examples:

| Source path | GBZ name | Sense | Sample | Haplotype | Contig |
|---|---|---|---|---:|---|
| `IRGSP-1.0.chr06` | `IRGSP-1.0#0#chr06` | REFERENCE | `IRGSP-1.0` | 0 | `chr06` |
| `NATELBORO.chr06` | `NATELBORO#0#chr06` | REFERENCE | `NATELBORO` | 0 | `chr06` |
| `Azucena.chr06` | `Azucena#0#chr06#0` | HAPLOTYPE | `Azucena` | 0 | `chr06` |
| `_MINIGRAPH_.s60277` | `_MINIGRAPH_.s60277` | GENERIC | none | none | verbatim generic locus |

## Final files

| Object | Bytes | SHA-256 |
|---|---:|---|
| XG | 8,378,505,934 | `466744e1f7da8f30b34f13f773302d55464fc51ccfe1a53c74d3709228476dae` |
| GFA | 6,984,618,682 | `bc2ee80275f3b1fd2ce830bacea52cf6189107e39dc35a6754f9f162bc1f3c98` |
| GBWT | 457,520,368 | `367ba437b73ff2ba59a37a0747171e9fcdf6b734d138b014c858c38f2ba3874d` |
| GBZ | 526,138,568 | `2cb5e040d35c11dbadc0af6ef73fd16c97dd9be0816b410b9a4c49efbf788fdb` |
| r-index | 1,054,507,727 | `7b3572f70b266fb5f2285c477145af66d555ef1e7c439af1c4d3e9d21bd1e6ea` |
| anonymous `.pngr` | 325,664,519 | `c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c` |

All objects above and all large temporary/failed artifacts remain outside the
repository on the designated scratch drive.

## Construction measurements

| Phase | Wall seconds | Peak process-tree RSS bytes | Temporary evidence |
|---|---:|---:|---|
| GFA export | 249.845 | 8,435,961,856 | direct `.part` output |
| semantic GFA split | 29.729 | 266,739,712 | 8,209,109,153 bytes of split outputs |
| accession GBWT | 583.774 | 18,924,969,984 | 20-million-node insertion buffer |
| reference tagging | 0.343 | 49,356,800 | none |
| generic GBWT | 36.741 | 9,907,048,448 | 20-million-node insertion buffer |
| GBWT merge | 30.303 | 10,658,041,856 | no retained vg spill |
| GBZ build | 28.304 | 9,957,945,344 | direct `.part` output |
| r-index | 28.397 | 5,040,758,784 | no retained vg spill |
| `.pngr` encode | 51.865 | 654,987,264 | 1,119,808,469-byte source cache; 325,664,519-byte temporary archive peak |

The GFA split measurement's observed directory total also included a retained
failed split, so the table reports the exact new output bytes rather than
mislabeling the directory total as incremental temporary growth. Likewise, the
old `.pngr` wrapper measurement watched the entire pre-populated run temp tree;
the per-encoder source-cache and archive figures above come from `pngr-build.json`.

The first default-buffer accession build was intentionally interrupted at 499
seconds after RSS reached 20,615,139,328 bytes while host swap was already full.
vg had selected a 175,650,560-node insertion batch. Its log and qualified abort
record are retained. The successful 20-million-node run stayed bounded. A
second retained failure showed that structured non-reference paths require an
explicit haplotype number; the final technical-0 mapping fixes this without
changing source identity or sequence.

## Xa7 evidence

The curated one-row GFF3 records `Xa7` at 1-based inclusive
`NATELBORO.chr06:28873554-28874897`, strand unknown. The reader exposes the
equivalent zero-based annotation start `28873553`.

- Exact `Xa7` search: one hit, 252 bytes over two extension ranges.
- Query `28873554-28874897` with 100 bp context: one tile, 61 nodes, 93 edges,
  51 anonymous tile-local traversals.
- Canonical hash:
  `2165bc15eaf7e237c50a0fce92c2d4ba7453a7461fa46bf32d78757695b712f1`.
- Source-oracle verification: correct, including haplotype-tile evidence.
- Cold local read: 3 requests/rounds and 145,864 unique bytes: 16,384 bootstrap,
  4,096 directory, and 125,384 payload bytes.

Real-browser loopback evidence passed 18/18 scenarios with request/origin
reconciliation and the same canonical hash. Cold transport-no-store results:

| Browser | Requests | Bytes | Rounds | Total ms |
|---|---:|---:|---:|---:|
| Chromium | 3 | 145,864 | 3 | 182.2 |
| Firefox | 3 | 145,864 | 3 | 212.0 |
| WebKit | 3 | 145,864 | 3 | 221.0 |

These are functional loopback timings, not public-network or CDN performance.
The retained browser run used Node.js 24.16.0, pnpm 11.24.0, Playwright 1.62.1,
Chromium 151.0.7922.34, Firefox 153.0, and WebKit 26.5 on Linux 6.17.

PPanG's published denominator of 113 agrees with the 113 accession paths found
here. The reported 16 Xa7-aligned genomes remains an external comparison target:
this task did not perform r-index membership inference, and the anonymous
`.pngr` must not be used to infer named individuals.

## Licensing verdict

`PUBLIC_DERIVED_REDISTRIBUTION_CONFIRMED`. On 2026-08-28, the project owner
explicitly confirmed that the PPanG graph data and derived objects are covered
by the MIT license and authorized publication of this rice `.pngr`.

The earlier permission-request draft was not needed and is not retained.

## Public archive

The checksum-addressed archive is published at:

<https://archives.ard.ninja/pangenome-range/sha256/c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c/rice-chr06-mc-xa7-anonymous.pngr>

It is a 325,664,519-byte, read-only file on the miniserver at
`/srv/data/public-archives`. The remote SHA-256 matches the local accepted
artifact. `pnpm bench -- origin-check` passed exact local/remote byte checks at
four ranges plus `206`, `Content-Range`, `Content-Length`, stable ETag, CORS and
preflight, exposed range headers, identity encoding, and
`public, max-age=31536000, immutable, no-transform` caching.

A real public Node reader workload passed 4/4 cold/warm pure-JS/WASM Xa7
queries. Every result matched canonical hash
`2165bc15eaf7e237c50a0fce92c2d4ba7453a7461fa46bf32d78757695b712f1`.
The retained machine-readable hosting proof is `public-origin-check.json`; the
reader result is summarized here because its per-query logs are reproducible.

## Reproduction

Run `results/rice-acquisition/commands.sh` from the repository root after
changing its scratch path if needed. It pins the 20-million-node GBWT buffer,
then runs acquisition, conversion, and the three-engine browser benchmark.

The repository intentionally retains only these result records:

- `REPORT.md` and `commands.sh`: human explanation and executable recipe;
- `pngr-build.json` and `pngr-validation.json`: final encoder configuration,
  checksum, measurements, and all-payload structural validation;
- `xa7-source-verification.json`, `xa7-reader-evidence.json`, and
  `xa7-browser-workload.json`: source-oracle and reader correctness;
- `browser/xa7-browser/summary.json`: the three-engine browser result;
- `public-origin-check.json`: published-byte and HTTP Range proof;
- `accession-gbwt-build.default-buffer.aborted.json`: the qualified failed
  baseline that established the bounded GBWT buffer requirement.

Source URLs, checksums, licensing review, and the curated annotation are kept
once under `data/rice/`. Per-stage timing JSON, full path and sequence tables,
zero-byte diffs, stdout/stderr, progress streams, environment captures, query
tables, and raw request logs are generated locally and intentionally ignored.
The original run's 93 such files were moved, without deletion, to
`$PPANG_RICE_DATA_DIR/repository-raw-results` on the designated scratch drive.

## Validation gates

After the implementation and evidence files were complete, the tree passed the
following gates. The only subsequent changes were this validation note and the
acquisition README's pointer to the already-tested reproduction script.

- `pnpm check` (Biome, strict TypeScript, 54 browser-package tests, 7 launcher
  tests, 6 documentation tests, and 5 benchmark tests);
- `pnpm check:rust` (rustfmt, 74 workspace tests, and Clippy with warnings
  denied);
- `pnpm build` (package bundle budgets, benchmark build, VitePress build, and
  public-export smoke test);
- `pnpm test:browser` (Chromium, Firefox, and WebKit; pure-JS and WASM decode,
  36 measurements with zero failures);
- the Xa7-specific browser workload (18/18 scenarios across the same three
  engines).

## Remaining limitation

The `.ri` is retained as a local identity-oracle input, but the published 16/113
Xa7 presence result has not been reproduced. That is the next highest-information
experiment and must preserve real accession identity rather than assigning
anonymous tile-local traversals to individuals.
