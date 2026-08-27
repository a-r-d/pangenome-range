# Pangenome explorer and viewer report

Status: **implementation and correctness gates passed; 2x detailed-query goal
not met**.

## Product result

`/demo` is a full-viewport scientific explorer with a unified coordinate/locus
command bar, configured/fixture/custom/local/recent archive sources,
summary-driven LOD, progressive detailed graph, genomic pan/zoom and history,
selection inspector, persistent light/dark layers, and a collapsible range and
performance panel. The application imports only `pangenome-range/reader` and
`pangenome-range/viewer`; it adds no backend or proxy.

The full Chromium flow and Firefox/WebKit smoke passed against a strict local
`206` origin. The flow covers absent and populated native locus indexes,
limited prefix suggestions, coordinate navigation, keyboard selection,
node hover/selection, custom URL, local-file isolation, stale-load
cancellation, actionable malformed `Content-Range`, canonical hash evidence,
browser back/forward restoration, and desktop/dark/tablet screenshots. Twelve
application range responses were
observed. Semantic roles, labels, command-bar focus, live status, visible focus,
keyboard navigation, reduced motion, and high-DPI canvas behavior are covered;
this is not a complete WCAG/axe audit.

A separate configured-default browser page opened the public 8.8 GB object
without an `archive` URL override, reached `ready`, and exposed its
`present-populated` native locus index. This directly exercises the production
default-selection path rather than only inspecting build configuration.

## Public reader and viewer changes

The framework-neutral viewer adds command parsing, deterministic summary LOD,
reference-anchored stable layout, node/edge/traversal provenance and hit tests,
theme/layer/display controls, genomic viewport updates, performance snapshots,
and viewport/selection/LOD events. It does not import Vue, VitePress, transport,
Node, or benchmark code.

Reader traces now retain nonoverlapping interval unions for integrity,
decompression, and regional decode and separately expose aggregate
decompression task duration. The timing wrapper preserves synchronous decoder
returns, avoiding scheduler delay attributed to decompression.

Bundle gates passed:

| Entry | Raw | gzip | Gate |
| --- | ---: | ---: | ---: |
| reader | 161,412 B | 36,829 B | <=160 KiB / <=50 KiB |
| viewer | 58,619 B | 14,811 B | <=64 KiB / <=16 KiB |
| node | 1,634 B | 638 B | <=8 KiB raw |

The native launcher remains absent from reader and viewer bundles.

## Retained 100 kb before/after

Archive: 8,832,749,949 bytes, SHA-256
`ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63`.
Workload: `CHM13#chr1:1,000,000-1,100,000`, context 100. Every scenario
matched canonical hash
`b191be02fc2a9556349d8b5b97b268c90c579b1c275cc600355bfaae5b499473`.

| Chromium strict-loopback cold no-store | Before | Current pure JS | Current WASM |
| --- | ---: | ---: | ---: |
| Query wall | 1,678.3 ms | 1,565.3 ms | 1,458.1 ms |
| Total / open | 1,717.3 / 27.3 ms | 1,577.7 / 12.2 ms | 1,485.3 / 12.4 ms |
| Actual requests / bytes | 4 / 294,198 B | 4 / 294,358 B | 4 / 294,358 B |
| Decompression wall/task | invalid overlap | 32.2 / 32.2 ms | 6.5 / 6.5 ms |
| Integrity | invalid overlap | 32.8 ms | 32.8 ms |
| Regional decode | 1,352.8 ms accumulated | 1,373.4 ms union | 1,288.5 ms union |
| Merge | not comparable | 55.5 ms | 63.4 ms |

The pure-JS observation is 6.7% faster than the retained query wall; WASM is
13.1% faster. The 2x target (`<=839.2 ms`) failed. WASM initialization measured
6.8 ms and adds 251,806 WASM bytes. Regional decode/reconstruction dominates,
so WASM remains optional and workers/OffscreenCanvas were not selected without
a measured end-to-end win.

## Search, summary, frame, and origin evidence

One public-path measurement after open produced:

| Operation | Cold | Warm | Cold bytes / rounds |
| --- | ---: | ---: | ---: |
| exact `HLA-B` | 125.5 ms | 1.2 ms | 69,112 / 2 |
| CHM13 100 kb summary | 106.2 ms | 0.3 ms | 49,503 / 2 |

The limited `HLA-` prefix returned five hits and `truncated=true`; an absent
exact search returned zero hits after one 7,430-byte leaf. These are single
public-network observations, not loopback percentiles.

The deterministic fixture app measured 13.6 ms open, 0.5 ms first summary
paint, 2.1 ms first graph tile, 2.6 ms query completion, and 1.2 ms frame p95.
This validates the bounded render path; it is not a large-query frame corpus or
a DevTools long-task trace.

The configured public Cloudflare path passed `HEAD`, exact/overlap/tail `206`,
local byte equality, 8,832,749,949-byte length, ETag, identity encoding,
immutable/no-transform caching, CORS for GitHub Pages, exposed headers, and
range/If-Range preflight. No full-object fallback occurred.

## Actual format gaps and remaining work

The regional payload lacks exact genomic coordinates for every reference node.
The ruler and interval are exact and node order/length/orientation are real,
but individual node placement is a proportional reference-anchored mapping.
The format also does not contain normalized biological variants, genotypes, or
frequencies; topology classifications are explicitly visual only. Stable
anonymous sample identity is intentionally absent, not a gap.

Before calling the viewer world-class, the highest-information work is a
controlled regional decode/reconstruction worker experiment with transferable
typed arrays, a repeatable large-region frame/long-task corpus, DevTools
interaction profiling, pixel-diff visual baselines, and a broader public/CDN
cold/warm corpus. Exact per-node reference anchors should be investigated only
with a compact measured format proposal and cross-language fixtures.

## Evidence inventory

- `config.json`, `environment.json`, `workload.json`
- `queries.csv`, `searches.csv`, `summaries.csv`, `requests.jsonl`
- `frames.json`, `summary.json`, `BROWSER_REPORT.md`, `public-origin.json`
- `screenshots/explorer-light.png`
- `screenshots/explorer-light-dark.png`
- `screenshots/explorer-light-tablet.png`

## Verification commands

- `pnpm check` — Biome, version sync, strict TypeScript, 37 reader/viewer
  tests, 7 launcher tests, 3 docs tests, and 5 benchmark-harness tests passed.
- `pnpm build` — package builds, export isolation, docs build, and bundle gates
  passed.
- configured `pnpm test:pages` — public default archive plus full Chromium app
  workflow and Firefox/WebKit smoke passed.
- `pnpm test:browser` — 36/36 Chromium/Firefox/WebKit pure-JS/WASM cache
  scenarios passed.
- `pnpm check:rust` — rustfmt, 72 Rust tests, doc tests, and Clippy with warnings
  denied passed.
- `origin-check` — public object metadata, CORS, strict ranges, and sampled local
  byte equality passed.
