# Release-hardening v1 evidence

## Verdict

**Release candidate but not publishable.** The stable-v1 format checklist has no
open release-critical row, local and remote clean-checkout gates pass, and the
current whole-HPRC bytes pass structural and independent source-oracle gates.
Publication remains blocked on the protected release workflow with six real
native artifacts plus the remaining npm and repository-protection setup.

No package, GitHub Release, or registry publication occurred.

## Inputs and configuration

The source is the exact 5,492,627,216-byte HPRC Release 2 Minigraph-Cactus v2.1
GRCh38 GBZ with SHA-256
`11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`.
The annotation is the 4,763,975,927-byte uncompressed GENCODE v50 comprehensive
GRCh38.p14 CHR GFF3 with SHA-256
`d97bc25b7d4d4aa9614c4cf6fa4748c083b1531cb60c52c0612c9ff7ec4813eb`.

The final encode used 16,384 bp windows, zstd-3, eight workers, a 256 MiB queue
limit, the persistent project-owned disk source, explicit `GRCh38.p14`
annotation/reference assembly metadata, and the conservative default exact
feature type `gene`. Source and generated objects were on the same MP510 NVMe;
the repository was on a separate Samsung NVMe. Full configuration and host
details are in `config.json` and `environment.json`.

## Whole-HPRC result

| Measurement | Previous populated GENCODE run | Current warm-cache RC |
| --- | ---: | ---: |
| Archive bytes | 8,832,749,949 | 8,832,750,626 |
| Index bytes | 47,376,777 | 47,376,841 |
| Whole wall | 420.848 s | 384.080 s |
| Payload pipeline | 261.548 s | 278.291 s |
| Writer finalization | 23.402 s | 23.582 s |
| Pre-rename validation | 26.827 s | 28.422 s |
| Output SHA-256 | 32.404 s | 31.982 s |
| Peak RSS | 695,816 KiB | 621,808 KiB |

The 677-byte archive growth is the deterministic provenance extension body and
its 64-byte extension-directory entry. Regional payloads and named/summary
schemas did not change. The current archive SHA-256 is
`d1308fce7c5811d8ca8566e0c3ede3dc1b908c77eaa948ae2663edde17435be4`.

The wall comparison is useful operational evidence but is not presented as a
pure encoder-speed claim: the earlier populated run built an ephemeral source
cache, whereas the current run reused a persistent cache and includes new
provenance options. The exact attributable cache benefit is the prebuild phase:
118.944 s cold versus 21.685 s warm, a 97.259 s saving. Other phase movement is
ordinary host/I/O variance.

The standard integrity gate validated all 11,559 directory pages and all
363,105 physical payloads before atomic rename. The final output SHA-256 remains
a separate 31.982-second pass because validation does not read every permitted
archive byte exactly once in physical order; combining the passes would weaken
or complicate the gate.

## Persistent source cache

Cold atomic persistent-cache creation took 130.040 s, peaked at 408,148 KiB,
and wrote 12,055,087,949 bytes. The manifest binds the source length/SHA,
GBZ/GBWT/sequence serialization versions, all component lengths/counts,
reference-metadata SHA, path-index interval, and the 132,492,865-byte serialized
path index SHA. Each raw component has per-256 KiB BLAKE3-128 integrity checked
lazily before use.

The exact cold ephemeral and warm persistent encodes used identical semantic
options and produced byte-identical archives:

| Measurement | Cold ephemeral | Warm persistent |
| --- | ---: | ---: |
| Whole wall | 612.870 s | 384.080 s |
| Prebuild | 118.944 s | 21.685 s |
| Time to first payload | 118.979 s | 21.725 s |
| Source cache build + fused SHA | 55.941 s | 0 s |
| Path-index build | 63.003 s | 0 s |
| Source authentication | fused above | 21.026 s |
| Cache open | n/a | 0.653 s |
| Path-index deserialize | n/a | 0.648 s |
| Peak RSS | 685,992 KiB | 621,808 KiB |
| SHA-256 | `d1308fce...17435be4` | identical |

The 228.79-second whole-wall difference must not all be attributed to reuse:
the cold run occurred after the 12.99 GiB loaded source oracle and had slower
payload, validation, and checksum I/O. The 97.259-second prebuild reduction is
the defensible directly measured cache saving.

A deterministic 512 MiB bounded layout experiment sampled 2,048 blocks and
4,096 warm random 4 KiB reads. Zstd-3 independent blocks occupied 36.52% of raw
bytes, but mean random access was 608.70 microseconds versus 3.22 microseconds
for current raw `pread` (about 189x slower). Warm mmap measured 5.13
microseconds. Neither alternative is a Pareto improvement, so the production
cache remains raw and explicitly byte-bounded.

## Named-locus evidence

The reference encoder omitted `named-loci-v1---` when no annotation was
supplied and generated it for this explicit GENCODE input. The measured
pipeline was:

| Stage | Result |
| --- | ---: |
| Accepted exact `gene` rows | 78,733 |
| Expanded searchable records | 157,466 |
| Records after exact deduplication | 157,466 |
| Leaf pages | 612 |
| Encoded / decoded leaf bytes | 3,658,492 / 23,595,176 |
| Encoded descriptor bytes | 61,145 |
| Annotation SHA-256 | 17.944 s |
| Parse + name expansion | 5.230 s |
| Sort + deduplication | 0.116 s |
| Page encode + zstd + append | 0.102 s |

Peak RSS remained 621,808 KiB, below the prior no-annotation 642,220 KiB
viewer-index baseline and far below 1 GiB. The real corpus therefore does not
justify an external-sort implementation. GENCODE v50 CHR contains no `Alias`
or `gene_synonym` attributes; alias behavior remains covered by conformance
fixtures, while the whole-corpus benchmark records this fact instead of
inventing an alias.

Cold exact `BRCA1` and its stable ID each required a descriptor plus one leaf
in two dependency rounds (68,729 and 68,987 bytes). Warm decoded-page-cache
repeats required zero reads and zero bytes. `BRCA` returned BRCA1, BRCA1P1, and
BRCA2. A broad one-character `E*` query with `limit=1` read the descriptor plus
four leaves, returned deterministic truncation, and avoided 446 candidate
pages. Missing, sample-filtered, contig-filtered, cold, and warm rows are in
`search-queries.csv`.

## Summaries, format, and conformance

The archive contains 591 summary series and 8,017 bins. Construction now derives
each next level from adjacent groups of four in linear emitted-bin work. Tests
prove exact preservation of all eight counters at every level and cover
fragmented manifests with different maximum levels, gaps, terminal clipping,
absent references, and warm decoded-page cache. Summary bytes remain unchanged
by the refactor.

Stable-v1 policy is now explicit: zstd content-checksum frames fail closed
because BLAKE3-128 already protects encoded payloads; edge tuples must be
canonical, strictly increasing, duplicate-free, and locally sourced. Shared
Rust/TypeScript corrupt fixtures cover both policies. Provenance uses registered
type ID `archive-meta-v1-`; its source/annotation hashes and deterministic user
metadata are protected by the extension integrity digest and exposed by Rust
and TypeScript archive-information APIs.

The independent loaded GBZ oracle ran separately in 84.27 s at 12,992,748 KiB
peak RSS. All 9 graph queries and 58 tile-local haplotype comparisons passed.
Because the final instrumented archive is byte-identical to the oracle-tested
archive, that evidence applies exactly. MHC adversarial tests also prove
ephemeral/persistent/loaded adapter equivalence, t1/t4 encoder byte identity,
forced reverse worker completion, opposite reader completion schedules, and
duplicate logical references to one physical payload.

## Repository and package gates

The exact local gate completed successfully:

```text
pnpm install --frozen-lockfile
pnpm check:rust
pnpm package:cargo
pnpm check
pnpm build
pnpm test:browser:ci
pnpm test:pages
```

This includes 70 Rust tests, Clippy with warnings denied, 28 reader/viewer tests,
7 launcher tests, strict local 206/CORS/ETag tests, docs tests, Node and browser
benchmark smoke, Cargo's seven-file package allowlist, export isolation, bundle
budgets, VitePress build, Pages artifact smoke, and Chromium range operation.
The tracked 73,920-byte MICB/KIR3DL1 fixture has SHA-256
`1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788`;
tests verify it before opening and require no network.

The main npm tarball is 167,274 bytes (759,125 unpacked, 15 allowed files). The
real Linux x64 glibc package is 2,851,867 bytes (6,641,832 unpacked, exactly
license, README, package metadata, and binary). The packed host install passed
`--version`, top-level/encode help, reader import, and viewer import. Reader,
Node, and viewer bundles are respectively 158,680/36,294, 1,634/638, and
36,856/9,369 raw/gzip bytes.

All six platform package shapes, exact versions, allowlists, absence of
lifecycle scripts, and dry-run packs were exercised locally. Non-host package
shape tests used the host binary as an explicitly synthetic stand-in; they are
not native-target evidence and their tarball sizes are not retained as release
measurements. Only `x86_64-unknown-linux-gnu` is installed locally.

## Remote status and remaining work

The clean-checkout follow-up commit `4d59c31` passed all remote gates. CI run
`33036238653` passed Rust, Cargo packaging, the hermetic TypeScript/test gate,
build and bundle checks, Pages artifact smoke, and Chromium range tests. Pages
run `33036238701` built, browser-smoked, uploaded, configured, and deployed the
site successfully. CodeQL run `33036238242` passed Actions, Python, Rust, and
JavaScript/TypeScript analysis.

The deployed demo defaults to the 8,832,749,949-byte content-addressed HPRC /
GENCODE archive. A fresh browser opened it at
`https://a-r-d.github.io/pangenome-range/demo`, selected `GRCh38` / `chr1`,
reached the ready state without errors, and issued five real `206` requests to
the static archive. The independent origin probe also passed HEAD, CORS
preflight, immutable/no-transform caching, identity, and four bounded ranges.

Before npm publication:

1. run the protected six-target release workflow with real native artifacts;
2. protect `main` (or add a ruleset) and require CI plus CodeQL;
3. configure npm ownership for `pangenome-range` and `@pangenome-range`, trusted
   publishing, and the protected `release` environment.

Node remains deliberately `>=24 <25`: Node 24 is the repository's stated and
tested support policy, and no other Node LTS runtime was available/tested in
this tranche. The declaration was not broadened without evidence.

A whole 1000GP encode is still not responsible. HPRC proves bounded behavior
and reusable-cache correctness, but the retained 1000GP two-chunk pilot still
shows source-side scaling that needs another bounded pilot under the new cache
before documenting a whole-source resource budget.

## Exact repository paths changed by the core tranche

The working tree contains the following intended paths. `AGENTS.md` was reviewed
but is unchanged because its architectural and workflow rules remain accurate.

```text
.github/workflows/ci.yml
.github/workflows/pages.yml
.gitignore
CHANGELOG.md
README.md
crates/pangenome-range-build/src/disk_source.rs
crates/pangenome-range-build/src/features.rs
crates/pangenome-range-build/src/fixed.rs
crates/pangenome-range-build/src/lib.rs
crates/pangenome-range-build/src/scale.rs
crates/pangenome-range-build/src/source.rs
crates/pangenome-range-build/src/test_support.rs
crates/pangenome-range-cli/src/main.rs
crates/pangenome-range-format/src/archive.rs
crates/pangenome-range-format/src/features.rs
crates/pangenome-range-format/src/lib.rs
crates/pangenome-range-format/src/metadata.rs
crates/pangenome-range-format/src/regional.rs
crates/pangenome-range-format/src/validation.rs
docs/ARCHITECTURE.md
docs/DISTRIBUTION.md
docs/FILE_FORMAT_V1.md
docs/FIXED_WINDOW_ARCHIVE.md
docs/FORMAT_RELEASE_CHECKLIST.md
docs/OPTIMIZATION_LOG.md
docs/adr/0001-v1-extension-directory.md
packages/browser/bin/launcher.mjs
packages/browser/src/reader/archive.ts
packages/browser/src/reader/features.ts
packages/browser/src/reader/index.ts
packages/browser/src/reader/regional.ts
packages/browser/src/reader/sources.ts
packages/browser/src/reader/types.ts
packages/browser/test/launcher.node-test.mjs
packages/browser/test/reader.test.ts
packages/browser/test/viewer.test.ts
results/2026-08-26-release-hardening-v1/REPORT.md
results/2026-08-26-release-hardening-v1/config.json
results/2026-08-26-release-hardening-v1/environment.json
results/2026-08-26-release-hardening-v1/search-queries.csv
results/2026-08-26-release-hardening-v1/source-cache.json
results/2026-08-26-release-hardening-v1/summary.json
scripts/benchmark-locus-search.mjs
scripts/benchmark-source-cache-layout.py
test-data/README.md
test-data/conformance/corrupt-archive-metadata-version.bin
test-data/conformance/corrupt-regional-descending-edges.bin
test-data/conformance/corrupt-regional-duplicate-edges.bin
test-data/conformance/corrupt-regional-malformed-boundary-edge.bin
test-data/conformance/corrupt-regional-noncanonical-edge.bin
test-data/conformance/corrupt-regional-nonlocal-edge-source.bin
test-data/conformance/corrupt-zstd-content-checksum.zstd3
test-data/conformance/format-v1-optional-extension.archive-meta.bin
test-data/conformance/format-v1-optional-extension.extensions.bin
test-data/conformance/format-v1-optional-extension.pngr
test-data/conformance/manifest.json
test-data/conformance/unsupported-zstd-content-checksum.zstd3
test-data/golden/record-archive-v1.json
test-data/golden/record-archive-v1.pngr
test-data/micb-kir3dl1.gbz
```

The clean-checkout and deployed-demo follow-up changed these additional paths:

```text
docs/HOSTING.md
docs/components/PangenomeDemo.vue
docs/demo.md
docs/test/docs.test.mjs
package.json
```
