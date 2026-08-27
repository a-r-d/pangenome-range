# Explorer redesign v2

## Outcome

The demo is now a full-viewport, showcase-first scientific application rather
than a documentation-style overview widget. The configured archive and HLA-B
open automatically, and the first screen pairs a concise product statement
with a live, budgeted graph query.

The redesign retains the existing public reader/viewer boundary and archive
semantics. No format, encoder, range-source, or query-model behavior changed.

## Product changes

- Added a first-impression showcase with live HLA-B detail, real archive
  metrics, and direct overview/search/detail actions.
- Split the former monolithic shell into topbar, tool rail, showcase, overview,
  detailed graph, search, loading, inspector, and evidence components.
- Replaced the one-bin HLA overview with a bounded multi-bin regional context.
  Its default normalized visible-bin scale makes relative variation legible and
  is labeled explicitly; linear and log controls remain available.
- Kept detail as a dark focus surface in both shell themes and reduced the
  default render budget to 160 nodes, 260 edges, and 6 traversal lanes so dense
  regions summarize instead of becoming raw edge hairlines.
- Added a Raycast-style archive-native search palette with stable IDs,
  coordinates, and honest detail-versus-overview affordances.
- Preserved prior visual context under a staged loading card with planned bytes,
  planned tiles, decoder, rationale, and cancellation.
- Clarified evidence labels: overview bytes/tiles are planned, while detail bytes
  are new transfer and tiles are ready.
- Kept deep links, history, source selection, local files, copy flows, semantic
  layer controls, errors, and technical evidence.

## Live configured-archive evidence

The final configured HPRC/Gencode run showed:

- archive size: 8.23 GiB;
- showcase detail: 3 tiles, 198.2 KiB new transfer in the retained run;
- regional overview: 11 exact summary bins around HLA-B;
- wider overview detail plan: 641 graph tiles and 17.0 MiB if that whole
  10.5 Mb context were requested;
- no graph payloads fetched by overview mode itself.

These figures are observations from one live browser run, not performance
benchmarks.

## Verification

- Biome: 86 files checked.
- TypeScript: browser, benchmark, and docs projects passed.
- Browser package: 38 Vitest tests and 7 launcher tests passed.
- Benchmark package: 5 tests passed, including strict range/origin behavior.
- Documentation tests: 3 passed.
- Bundle budgets and public export isolation passed.
- Pages smoke: 12 real local 206 responses; configured archive, local file,
  cancellation, bad-origin error, and history checks passed.
- Browser matrix: Chromium, Firefox, and WebKit passed.
- Rust gate: 74 library/CLI tests passed, plus rustfmt/clippy/doc tests.

The globally installed pnpm 11.21.0 hung before producing output on this host,
while the repository pins pnpm 11.24.0. A temporary Corepack shim resolved the
pinned version for nested workspace scripts; the exact `pnpm check`,
`pnpm check:rust`, `pnpm build`, and `pnpm test:browser` gates then passed.

## Remaining weaknesses

- The archive's finest summary bin is 1,048,576 bp, while HLA-B is much smaller.
  The overview therefore cannot honestly show sub-bin structure. It expands to
  neighboring exact bins and labels the normalized visible-bin scale.
- The live HLA-B payload contains thousands of graph records. The default canvas
  is intentionally budgeted and reports omitted records; individual labels
  require a narrower viewport.
- The contextual inspector opens after a real canvas selection rather than
  inventing a preselected node.
- The live preview is hidden below 900 px width so the tablet showcase stays
  coherent; focused detail remains available.
- The prompt referenced source SVG mockups and `design-tokens.css`, but only
  the supplied rendered PNG references were present on this host.
- The optional population archive was not configured during this run.

## Screenshot index

Primary comparison:

- [before public HLA](screenshots/before-public-hla.png)
- [after showcase](screenshots/after-public-showcase.png)
- [after regional overview](screenshots/after-public-overview.png)

Supporting states:

- [before search](screenshots/before-public-search.png)
- [after search](screenshots/after-public-search.png)
- [before loading](screenshots/before-loading.png)
- [after loading](screenshots/after-loading.png)
- [after detail](screenshots/after-detail.png)
- [after tablet](screenshots/after-tablet.png)
- [after broken-origin error](screenshots/after-error.png)

## Highest-information follow-up

Prototype and benchmark a dedicated mid-detail aggregation model that emits
stable branch bundles and branch-count capsules before paint. That would make
dense real loci read more like the detailed mockup without fabricating nodes or
changing tile-local traversal semantics.
