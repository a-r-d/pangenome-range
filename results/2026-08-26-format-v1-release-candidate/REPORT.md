# File-format v1 release-candidate report

This tranche produces a defensible **release candidate, not stable v1**. It
keeps the accepted archive construction and haplotype model, moves normative
format/query responsibilities to their intended crates, adds cross-language
conformance data, decides extension and integrity policy, and measures the
candidate on the same whole HPRC source and host as the retained baseline.

## Format decisions

Two incompatible pre-stable v1 changes are deliberate:

1. Header bytes 48..63 now optionally point to a bounded, versioned extension
   directory. Unknown optional extensions are skipped and unknown required
   extensions fail closed. ADR 0001 records why this was chosen over requiring
   v2 for title/provenance/locus metadata.
2. Every 56-byte directory entry now carries BLAKE3-128 over the exact encoded
   regional payload. Readers verify it before decompression. ADR 0002 records
   the no-checksum, 64-bit, 128-bit, header, and extension-table alternatives.

There is no compatibility decoder for old 40-byte research directory entries.
Old research archives must be regenerated.

## Whole HPRC result

Source: 5,492,627,216 bytes, SHA-256
`11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`.
The new archive is retained outside the repository at the path in
`config.json`.

| Measurement | Accepted baseline | Release candidate |
| --- | ---: | ---: |
| Archive bytes | 8,828,788,418 | 8,828,788,418 |
| Archive/source | 1.607389 | 1.607389 |
| Index bytes | 47,376,617 | 47,376,617 |
| Directory pages / entries | 11,559 / 363,105 | 11,559 / 363,105 |
| Construction including pre-rename validation | 552.565 s | 354.481 s |
| Pre-rename structural validation | 240.736 s | 28.123 s |
| Final output SHA-256 | not separately timed | 32.520 s |
| Whole command | about 641 s | 438.720 s |
| Peak RSS | 8,776,204 KiB | 8,775,928 KiB |
| Occurrence scratch / payload spool / general scratch | 0 / 0 / 0 B | 0 / 0 / 0 B |

The archive and index did not grow: the observed maximum page occupancy was 52,
so the larger 56-byte entries still fit the existing arithmetic page count.
The SHA-256 changed from `9dec2631…6176b80d` to
`76ae6616…f0f0121e` because the formerly zero directory padding now contains
payload digests. The 28.123-second standard gate validated all 363,105 physical
payloads before atomic rename. It checked directory/range relationships,
BLAKE3-128, exact decompression, and regional structural decoding.

The retained nine-query workload passed all graph comparisons and all
tile-local weighted-haplotype comparisons against the source GBZ, including
fragment-start and terminal-boundary queries.

## Validation and integrity measurements

On MHC, standard validation measured 85.550, 44.236, 22.534, and 12.049 ms at
1, 2, 4, and 8 effective workers. Full reconstruction measured 237.976 ms; its
conservative occurrence-memory estimate reduced an eight-worker request to one
effective worker under the 512 MiB budget. Worker CPU milliseconds are reported
separately from wall critical-path time.

Scanning the prior whole-HPRC archive found 363,105 payloads, 8,781,411,801
encoded bytes, and maximum page occupancy 52. Reading those payloads took
3.915 s; BLAKE3 took 2.335 s (3,586.7 MiB/s). The chosen directory placement
modeled zero archive/index growth and zero extra page reads. A browser query
over seven cold payloads measured median 1.775 ms for BLAKE3 verification;
verified warm payloads are cached and the test requires zero repeat integrity
time.

## Determinism and conformance

The MHC release-candidate archive was byte-identical at one and four encoder
workers: 4,806,677 bytes and SHA-256
`164d18c254cae1e52bfed5a6cd53ea9d48c8d14ab50dbcc85d5e3b54f5569c70`.
The Rust and TypeScript readers consume the same schema-2 conformance manifest,
decode the same archives, produce graph hash
`bf984cbc…a6fa1724` and tile-local hash `283a80ba…65f6aabb`,
and reject the manifest's malformed archive/regional cases. Five opt-in fuzz
targets cover header, root, directory page, regional payload, and packed record
parsers.

## Source access and 1000GP decision

The new `PangenomeSource` seam separates metadata/reference discovery,
node/record borrowing, and reference-position lookup from the encoder. The
existing fully loaded GBZ adapter remains the baseline. A two-chunk GRCh38 chr6
pilot still peaked at 8,776,080 KiB and took 51.37 s, proving that filtering
does not bound source memory. It wrote a 4,807-byte archive with no occurrence
index, spool, or scratch.

A whole 5,008-haplotype 1000GP attempt is therefore **not responsible yet**.
The next source-scale experiment must first demonstrate lazy/mmap access on a
bounded chromosome pilot and document its memory budget. The checked-in
upstream issue draft narrows the required GBZ/simple-sds API work.

## Remaining stable-v1 blockers

Before stable v1 is frozen:

- standardize a provenance extension payload, or a cryptographically
  archive-bound external sidecar contract;
- close the remaining cross-language zstd and merge/request-order adversarial
  checklist rows;
- run and retain bounded opt-in campaigns for all five fuzz targets.

Full-load GBZ access is a documented release-time RAM requirement, not a hidden
claim of bounded loading. It blocks a responsible 1000GP run but need not block
format v1 if the requirement remains explicit.
