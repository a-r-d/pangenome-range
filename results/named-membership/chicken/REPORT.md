# Chicken chromosome 15 named-membership demo

The 30-chicken corpus now has a bounded, publishable `.pngr` demonstration.
The complete bGalGal1b chromosome 15 component encoded into a 15,939,592-byte
archive with searchable GRCg7b genes, overview summaries, provenance, and exact
named source-path membership. Exhaustive archive validation passed all 776
tiles. Five fixed source-oracle queries also reconstructed the same graph and
tile-local haplotype semantics from the derived GBZ.

## Why this is chromosome-first

The first attempt passed the complete 49,001,347-node, 12.6 GB uncompressed GFA
to `vg gbwt`. Under a 12 GiB, zero-swap cgroup, vg exhausted the budget while
indexing paths/walks in build job 0, before it reached final GBZ construction.
That path is abandoned and the production script explicitly prohibits it.

The replacement performs a streaming, constant-memory extraction of the closed
chr15 component (`14724903..15245596`) only after verifying the complete source
checksum. The subset contains 518,756 segments, 703,861 links, and 80 walks and
is 143,961,984 bytes. Its vg conversion peaked at 378,252 KiB under a 2 GiB
ceiling with no swap. The local gbwtgraph rvalue-copy repair addresses a later
GBZ constructor copy; it would not have prevented the full-source failure,
which occurred earlier during path indexing.

## Named identity result

The archive catalog contains 80 exact GBWT paths across 18 sample labels. The
encoder located 47,222 positions directly from the embedded DA, with at most
1,023 LF steps against an 8,192-step hard limit. Across the chromosome, 8,441
canonical traversal groups contain 22,835 memberships. Membership multiplicity
sums to the anonymous occurrence weight exactly: 22,835.

The IGLL1 showcase covers `bGalGal1b#chr15:7913472-7979008`. The archive finds
the exact GRCg7b `IGLL1` gene, reads four graph tiles, and returns 72 canonical
groups with 196 memberships across 30 source paths. The membership multiplicity
and occurrence weight both equal 196. Example retained identities include
`UCD312#1#h1tg000050l#fragment=1265655` and
`UCD312#2#h2tg000050l#fragment=1251366`.

The public reader fetched 139,103 graph bytes in three dependency rounds, 7,086
membership bytes in three rounds, and 1,058 catalog bytes in one additional
round. Its canonical hash is
`d21b5f3495521359e06afd365c951247f10379a08ec49ad5e4bd310d857df7c3`.

## Construction result

- Component extraction: 258.1 MiB peak RSS, zero swap.
- GFA-to-GBZ conversion: 369.4 MiB peak RSS, zero swap.
- Source-cache build: 56.5 MiB peak RSS, zero swap.
- Archive encoding: 9.56 seconds and 44,648 KiB process peak RSS.
- Full archive validation: passed all 776 tiles.
- Five-query source oracle: all graph hashes and haplotype tiles passed, 78.4
  MiB peak RSS.

The archive SHA-256 is
`fcb19b2c6e850c16e7e831613f34d27feef477331064cde5b16137492e6d1b43`.
There is no anonymous whole-chromosome control, so the retained evidence does
not claim a named-mode percentage overhead.

The content-addressed object is live at
`https://archives.ard.ninja/pangenome-range/sha256/fcb19b2c6e850c16e7e831613f34d27feef477331064cde5b16137492e6d1b43/chicken-chr15-named.pngr`.
The retained origin probe passed strict ranges, CORS/preflight, immutable
no-transform caching, ETag stability, exact length, checksum, and byte equality.
A real Chromium Pages smoke opened IGLL1, selected a traversal, and displayed
resolved catalog sample/path names, orientation, and multiplicity. Firefox and
WebKit retained the common fixture coverage.

## Provenance and distribution

The source graph is `pangenome.gfa.gz` from Rice et al., *A pangenome graph
reference of 30 chicken genomes allows genotyping of large and complex
structural variants*, BMC Biology (2023), Zenodo record 10018222. Zenodo marks
the file CC BY 4.0. The gene subset comes from NCBI RefSeq assembly
GCF_016699485.2. Exact source URLs, byte lengths, and checksums are in
`data/chicken/sources.json`.

The complete GFA and all generated GFA, GBZ, cache, and `.pngr` objects remain
outside the repository. Only compact evidence and reproducible, memory-capped
scripts are retained here.
