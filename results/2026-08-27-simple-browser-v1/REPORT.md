# Single-screen tube-map browser report

Date: 2026-08-27

## Scope

This tranche replaces the production `/demo` application and public viewer
entry without modifying the Rust encoder, `.pngr` v1 bytes, archive extensions,
metadata, or haplotype semantics. The configured immutable archive remains:

```text
https://archives.ard.ninja/pangenome-range/sha256/ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63/hprc-v1-gencode-v50-disk-t8.pngr
```

## Baseline

- production Vue component files: 10 (one `PangenomeDemo` plus nine explorer
  components);
- viewer public entry: 64,461 bytes raw and 16,240 bytes gzip in a clean
  pre-change build;
- built demo route JavaScript (reader plus viewer/application chunks): 182,120
  bytes raw and 59,910 bytes gzip; built site CSS: 149,540 bytes raw and 27,877
  bytes gzip;
- baseline DOM at 1600x1000: 423 descendants, two canvases, 31 buttons;
- live HLA-B: two tiles, 139.3 KiB new transfer, 1,233.1 ms, 5,718/5,740
  displayed/decoded nodes, 7,787/7,819 edges, and 16/405 local traversals;
- the first screen was a marketing showcase beside a dense Canvas preview.

The baseline screenshot is `screenshots/before-demo-1600x1000.png`.

## Result

- production Vue component files: 10 (a thin route wrapper plus nine explicit
  browser components);
- viewer public entry: 37,546 bytes raw and 9,859 bytes gzip;
- built demo route JavaScript: 131,101 bytes raw and 44,811 bytes gzip; built
  site CSS: 124,969 bytes raw and 23,233 bytes gzip;
- the normal demo has one screen, no permanent sidebar, no overview mode, no
  loading modal, and no evidence drawer;
- the desktop document remains exactly 1600x1000 with no page overflow;
- the default pattern count is eight and the post-collapse refusal limits are
  400 node groups and 800 topology edges;
- HLA-B fits those limits at 141 node groups, 240 topology edges, eight local
  patterns, and 548 SVG descendants in the retained Chromium run.

## Implementation inventory

Removed from the production path:

- `docs/components/PangenomeDemo.css`;
- the nine Vue components and shared type file under
  `docs/components/explorer/`.

Created:

- the browser shell, toolbar, location search, reference track, SVG tube-map
  view, node/pattern inspectors, archive source menu, status bar, and shared
  styling/types under `docs/components/browser/`;
- `browser-policy.ts`, `tube-map-model.ts`, `tube-map-layout.ts`, and
  `tube-map-renderer.ts` in the public viewer source;
- the deterministic golden JSON and its model/layout/renderer tests;
- ADR 0003, the non-navigation tube-map lab page, and this retained result.

Substantially rewritten:

- `PangenomeDemo.vue` is now a thin route wrapper;
- the public viewer entry exposes only the browser policy, navigation, and
  tube-map API;
- the docs architecture, distribution, product requirements, performance,
  benchmarks, optimization log, package README, fixture preparation, export
  smoke test, docs contract tests, and Pages browser test now describe and
  exercise the replacement.

## Renderer decision

SequenceTubeMap's MIT-licensed core was inspected. Its useful conventions are
reference-first order, smooth path routing, visible orientation, node
compression, and direct hover/selection. Its mutable 5,000-line D3 core remains
coupled to React-era read/track state and broader application infrastructure,
so extraction was rejected. The local renderer copies no SequenceTubeMap code;
no third-party notice was added. See `docs/adr/0003-tube-map-renderer.md`.

## Retained screenshots

- `screenshots/browser-golden.png`: static two-tile golden model;
- `screenshots/browser-hla-b.png`: configured archive default HLA-B;
- `screenshots/browser-micb.png`: configured archive lower-complexity locus;
- `screenshots/browser-search.png`: compact archive-native search results;
- `screenshots/browser-fixture.png`: hermetic range-backed fixture.

## Browser and query evidence

The built Pages workflow passed Chromium, Firefox, and WebKit. Its hermetic
fixture made eight strict `206 Partial Content` responses and passed coordinate
navigation, back/forward history, zoom controls, `Cmd/Ctrl+K`, node inspection,
pattern inspection, archive-native named-locus search, a custom URL, local-file
isolation, golden-fixture loading, and a 1600x1000 no-overflow assertion.

Configured public archive measurements from a fresh Chromium page:

| Locus | Plan / ranges | Transfer | Open | First tile | Complete | Model |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| HLA-B | 2 tiles / 3 ranges | 169.7 KiB | 563.8 ms | 1,254.6 ms | 1,431.9 ms | 141 groups, 240 edges, 8 patterns, 548 SVG descendants |
| MICB | 2 tiles / 3 ranges | 96.3 KiB | same open archive | 598.5 ms | 664.1 ms | 82 groups, 153 edges, 8 patterns, 343 SVG descendants |

Both completed with payload SHA-256 verification. These are single public
network observations from the retained run, not latency distributions. Time to
linear context is not yet separately instrumented; it precedes the first-tile
measurement after locus resolution and exact planning.

## Interaction tuning

Direct Brave manipulation reproduced the reported wheel sensitivity. The same
synthetic trackpad gesture enlarged a reference node by 1.5625x before the
repair and 1.0661x afterward. The continuous transform now normalizes wheel
delta modes, caps each logarithmic step, zooms around the pointer, and repaints
at most once per animation frame. The built Chromium assertion measured 0.05 px
of pointer-anchor drift.

Measured visible-label collisions fell from four to zero for HLA-B and from
three to zero for CHAD after moving labels outside their pattern lanes,
increasing deterministic lane spacing, and width-gating compact node labels.
Tile boundaries now follow their first visible source-tile reference node
instead of an unrelated linear coordinate projection.

A collapsed-node inspection click now leaves the node count unchanged and opens
a bounded sequence/neighbor/provenance drawer. Expanding the chain is explicit;
the retained HLA-B check changed 141 groups to 204 only after that action.
Escape and an empty graph click close the drawer, horizontal wheel and drag pan
remain local, double-click zooms, and Home restores fit.

## Measurement protocol

The final retained run used macOS 26.5.1 arm64 (Darwin 25.5.0), Node.js
24.16.0, pnpm 11.24.0, Playwright 1.62.1, and pre-change commit `5129649`.
The baseline and current bundles were built with `pnpm build`; the listed gzip
sizes are `gzip -c` byte counts over the same route, reader/viewer, and CSS
artifacts. The browser evidence and screenshots were refreshed with:

```bash
VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL="https://archives.ard.ninja/pangenome-range/sha256/ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63/hprc-v1-gencode-v50-disk-t8.pngr" \
PANGENOME_RANGE_DEMO_SCREENSHOT="results/2026-08-27-simple-browser-v1/screenshots/browser.png" \
pnpm test:pages
```

The final repository gates were `pnpm check`, `pnpm build`, and
`pnpm check:rust`.

## Scientific limitations

- anonymous patterns remain tile-local and cannot identify or stitch a global
  sample path;
- structural segment widths are display encodings, not exact genomic scale;
- collapse preserves member IDs, sequence bytes, orientation, weights, and
  provenance, but dense alternate structure is summarized into inspectable
  bundles;
- the current archive query returns complete fixed-window payloads, so a very
  small coordinate inside one tile does not reduce decoded tile complexity;
- the linear track shows only the selected named locus because the current
  named-locus API is a search index, not an interval-annotation enumerator.

## UX limitations

- the first SVG is main-thread rendered; a worker has not been justified by a
  retained failure;
- fit-scale views intentionally suppress narrow node labels, which return on
  zoom;
- mobile controls collapse, but this tranche is optimized for desktop genome
  browser use;
- the lab route is deliberately nonpublic and absent from documentation
  navigation.

## Verdict

Yes, for bounded local research exploration. A researcher familiar with genome
browsers can open the page, search a locus, recognize the reference and local
alternate structure, inspect sequence or weighted tile-local evidence, and
share the URL without learning a separate product-mode vocabulary. It is not a
global sample-path browser, an allele-frequency view, or a substitute for exact
base-scale topology inspection after structural bundling.
