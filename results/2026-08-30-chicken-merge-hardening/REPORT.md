# Chicken demo merge hardening

## Verdict

`READY_TO_MERGE`

The checksum-addressed whole-chicken archive is the configured default research
demo, the exact UCD312 event is independently bound to the archive, and the
public origin passes strict byte-range validation. Announcement remains gated on
merge, the post-merge Pages workflow, configured multi-archive smoke, and
deployed chicken default/published-example checks.

## Evidence boundary

This report retains the facts measured from the archive that is actually hosted:

- source GBZ: 1,359,062,880 bytes, SHA-256
  `96c04d263e8af7cf0863cfda8a22bb5cc9b9c3aea387cdd59a05db7a7ab1ea7f`;
- `.pngr`: 1,498,984,132 bytes, SHA-256
  `93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e`;
- 64,371 physical payloads, 207 reference paths, and 1,052,949,595 reference
  bases;
- 56,390 named-locus search records and 12,237 GBWT source-path catalog records;
- 717,130 tile-local traversal groups and 1,850,732 membership records;
- 851.339 seconds construction wall time and 173,668 KiB encoder peak RSS;
- 64,371/64,371 structural validation and 8/8 retained graph/source-oracle
  workloads.

The different numbers supplied in the review prompt did not match the retained
checksummed archive, encoder report, or public object and were not substituted
for measured evidence.

## Exact paper-backed example

The pinned 430,058,854-byte graph VCF identifies the `chr15:7,944,313` record
`>14904200>14904750`, a 5,184-base reference allele, first alternate direct
deletion edge, allele count one, and UCD312 genotype `12|1`. The edge maps to
two adjacent 16 KiB tile-local groups. Both resolve only to path ID 3667,
`UCD312#2#h2tg000050l#fragment=1251366`, with multiplicity one.

The cold preset trace read 97,535 graph bytes, 13,595 membership bytes, and
9,570 catalog bytes: 120,700 bytes across 10 reported ranges. The broader IGLL1
reader check read 170,002 bytes across 12 ranges. The browser status remains
cache-aware; after the default IGLL1 view had warmed shared pages, opening the
published example reported 37.5 KiB across 6 additional ranges.

## Tool reproducibility boundary

- vg commit: `32cadf3d3ee45d04c532767158c7dee6243f5713`.
- Pinned GBWTGraph submodule: `7fac0af212b0502d40cca3d2b2c3a5fd7d85c540`.
- Local patch SHA-256:
  `2398b3c11c47e1caf978cadc17e896bb30a9ddcdc54fb7e32084cd8f66dda841`.
- The patch applies cleanly to an archive of the pinned unmodified submodule.
- The recorded vg tree has only that patch, and its binary matches SHA-256
  `80562fb2bb1240b520a139bfb060592214c71e68846c0dd915592c07b352a763`.
- `scripts/chicken/build-pinned-vg.sh` reconstructs the pinned source and
  submodules, applies the patch, fixes vg's embedded host inputs, requires the
  measured compiler, and fails unless the binary checksum matches.

A fresh from-zero vg compilation was not repeated during this bounded
hardening pass; the existing exact source state, clean patch application, and
measured binary identity were verified without another high-memory build.

## Browser and origin proof

The configured Pages smoke passed real Chicken, HPRC, 1000 Genomes, and Rice
queries. It also passed the offline fixture in Chromium, Firefox, and WebKit,
with 19 local strict `206` responses. Chicken checks covered default selection,
IGLL1, whole-genome labeling, capability/path counts, the first-use hint, exact
UCD312 selection, filtering, both tile highlights, TSV and FASTA downloads, and
no body overflow.

`origin-check.json` records exact equality for four public byte ranges plus
correct `206`, `Content-Range`, CORS, stable ETag, immutable/no-transform cache
policy, remote size, and local SHA-256.

The viewer bundle changed from 46,679 raw / 12,015 gzip bytes at the pre-change
head to 58,307 raw / 14,975 gzip bytes: +11,628 raw and +2,960 gzip bytes. All
bundle budgets pass, and reader/native-launcher isolation remains intact.

See `screenshot-index.json` for the six retained 1600×1000 screenshots and exact
tested routes.

## Gates

Passed:

- `pnpm install --frozen-lockfile`;
- `pnpm check:rust`;
- `pnpm package:cargo`;
- `pnpm check`;
- `pnpm build`;
- `pnpm test:browser:ci`;
- `pnpm test:pages`;
- configured-archive Pages smoke;
- live chicken `origin-check` against the local archive.

`package:cargo` and the tests that bind local HTTP origins were rerun outside
the filesystem/network sandbox after their first sandboxed attempts were denied
with `EPERM`; the reruns passed.

## Final correctness hardening

- FASTA reconstruction now validates opaque sequence bytes directly. Forward
  ASCII IUPAC DNA bytes preserve their exact case; reverse nodes complement
  `ACGTRYSWKMBDHVN` and lowercase equivalents. U/u, unsupported ASCII, and
  non-ASCII bytes fail with a typed error and are never replaced with `N`.
- The published preset now requires exactly two expected tiles, digests,
  occurrence weights, path IDs, multiplicities, orientations, and displayed
  patterns before highlighting both patterns and opening the claim card. An
  operation token invalidates the work on source, archive, or region change.
- The inspector always says occurrence weight is tile-local, is not allele
  frequency, and named fragments are not stitched across tiles.
- Named-path highlighting contributes its membership reads to displayed
  byte/range totals. The configured chicken smoke asserts the total before and
  after the operation equals the separately exposed highlight trace: 156,438
  bytes / 6 ranges before, plus 1,450 bytes / 3 ranges for highlighting, equals
  157,888 bytes / 9 displayed ranges after.
- Focused browser tests pass 77/77, including the eight strict-preset cases and
  the full upper/lowercase IUPAC export matrix.

## Remaining limitations

- Named identity remains an experimental pre-1.0 extension.
- Tile boundaries remain authoritative; the UI does not invent a complete
  cross-tile or chromosome path.
- The 1000 Genomes named-membership experiment remains excluded after the prior
  resource-safety OOM.
- HPRC named-membership evidence remains the bounded four-tile pilot, not an
  archive-wide result.
- No phenotype, breed, ancestry, allele-frequency, or complete-assembly claim
  is made.
