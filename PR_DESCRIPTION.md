## What this adds

- Adds optional experimental `path-members-v1-` identity pages without changing
  the regional graph payload or anonymous weighted tile semantics.
- Builds named membership directly in the bounded disk-backed Rust encoder using
  embedded GBWT document-array samples and tile-batched LF locate; neither the
  browser decoder nor the encoder depends on a local `gbwt-rs` checkout.
- Exposes lazy catalog, tile-membership, path search, batched path lookup, and
  combined graph/membership query APIs in TypeScript.
- Adds a fragment-aware tube-map inspector with exact sample, haplotype,
  fragment, multiplicity, orientation, path sense, filtering, cross-tile path-ID
  highlighting, named-path TSV, and oriented tile-local FASTA.
- Makes the complete 30-assembly chicken pangenome the default configured
  research demo, with HPRC, 1000 Genomes, rice, and the offline fixture kept as
  semantically distinct alternatives.

## Whole chicken result

- Source GBZ: 1,359,062,880 bytes.
- `.pngr`: 1,498,984,132 bytes; 1.102954215x source size.
- Reference scope: 207 `bGalGal1b` paths; 1,052,949,595 bases.
- Regional payloads: 64,371; full structural validation passed all 64,371.
- Named loci: 56,390 searchable records from 25,437 GRCg7b gene rows.
- Path catalog: 12,237 GBWT source-path records—not chickens or genomes.
- Membership: 717,130 traversal groups; 1,850,732 membership records;
  occurrence weight 1,850,762.
- Construction: 851.339 seconds; 173,668 KiB encoder peak RSS; zero occurrence
  index, payload spool, or scratch bytes.
- Source oracle: 8/8 retained graph workloads passed, including all selected
  tile-local haplotype comparisons and IGLL1.
- Broad IGLL1 combined reader query: 146,013 graph bytes + 14,419 membership
  bytes + 9,570 catalog bytes = 170,002 bytes across 12 reported ranges.
- Public archive: checksum-addressed 1.50 GB object; retained origin validation
  passed exact ranges, CORS, ETag, immutable/no-transform cache policy, checksum,
  and local/remote byte equality.
- Exact published preset: 120,700 cold bytes across 10 reported ranges; the
  broader IGLL1 reader check remains 170,002 bytes across 12 ranges.

## Paper-backed IGLL1 example

- Pins the paper graph VCF at 430,058,854 bytes and SHA-256
  `ec85ea239c4d39cb77c09bfaca347347c73f8d54578a9ba4fb78cfa31eed5ee4`.
- Validates the exact `chr15:7,944,313` multiallelic record, its 5,184-base
  reference allele, direct deletion edge, allele count one, and UCD312 genotype
  `12|1`.
- Maps that edge to two adjacent tile-local digests, both exclusively carried by
  `UCD312#2#h2tg000050l#fragment=1251366` with multiplicity one.
- Keeps the paper's biological interpretation separate from directly observed
  archive membership. It makes no phenotype, breed, ancestry, or
  allele-frequency claim.

## Reproducibility and safety

- Pins vg commit `32cadf3d...`, GBWTGraph `7fac0af...`, g++ 15.2.0, build
  identity, and binary SHA-256.
- Checks in the one-line GBWT rvalue-move patch and verifies that it applies to a
  clean pinned submodule checkout.
- Keeps the measured one-job/20-million-node vg conversion under a 42 GB
  zero-swap cap; `.pngr` construction remains under a 2 GB cap.
- Large GFA, VCF, GBZ, cache, and `.pngr` objects remain outside git.

## Explicit exclusions and limits

- The 1000 Genomes named-membership pilot remains excluded after the prior
  resource-safety OOM; no result is inferred from that exclusion.
- There is no archive-wide HPRC named-membership overhead claim; HPRC evidence is
  bounded to the retained four-tile pilot.
- Named source identity is still pre-1.0 experimental. Fragment boundaries and
  tile-local traversal semantics remain authoritative.
- No normative format redesign, regional payload change, variant index,
  phenotype metadata, query server, or new species is included.

## Validation

- Rust and TypeScript golden/corruption/multiplicity tests cover the extension.
- Mixed forward/reverse FASTA reconstruction and exact TSV columns have focused
  unit tests.
- Pages smoke covers chicken defaulting, whole-genome labeling, capabilities,
  path counts, first-use hint, exact published preset, filtering/highlighting,
  exports, explicit source overrides, responsive overflow, and retained
  1600×1000 screenshots.
- Viewer bundle change from the pre-change head: +3,911 raw bytes and +1,112
  gzip bytes; all bundle budgets pass.
- `pnpm check:rust`, `pnpm package:cargo`, `pnpm check`, `pnpm build`,
  `pnpm test:browser:ci`, `pnpm test:pages`, configured-archive smoke, and the
  live origin check all pass.
- Full evidence and retained screenshots are in
  `results/2026-08-30-chicken-merge-hardening/`. This branch does not publish
  packages or merge a normative format change.
