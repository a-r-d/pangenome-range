# ADR 0002: put BLAKE3-128 in each regional directory entry

- Status: accepted for the v1 release candidate
- Date: 2026-08-26
- Supersedes: no per-payload checksum in the pre-checksum research layout

## Context

Pre-checksum v1 had exact ranges and decoded lengths, strict one-frame zstd
validation, full regional structural decoding, immutable HTTP identity, and
external whole-object SHA-256 evidence. It could not authenticate a selected
encoded payload before asking the decompressor to consume it.

The decision was measured against the accepted current whole-HPRC archive
(`9dec2631…6b80d`, 8,828,788,418 bytes, 363,105 physical payloads) without
rewriting it. The evaluation command was:

```text
target/release/pangenome-range evaluate-integrity \
  hprc-v2.1-mc-grch38-v1-t8-zstd3.pngr \
  --report hprc-integrity-options.json
```

That bounded scan read and hashed every encoded physical payload once. It took
6.37 s process wall time and 29,544 KiB peak RSS. Directory scan was 81.3 ms,
payload reads were 3.915 s, and BLAKE3 CPU wall was 2.335 s for 8,781,411,801
bytes (3,586.7 MiB/s). These are integrity-scan measurements, not encoder or
browser-query latency results.

## Alternatives measured

| Alternative | Current-HPRC object/index growth | Page capacity | Pages over capacity | Extra query index read | Detects before decompression |
| --- | ---: | ---: | ---: | ---: | --- |
| No checksum | 0 | 102 | 0 | 0 | No |
| BLAKE3-64 in directory | 0 | 85 | 0 | 0 | Yes |
| BLAKE3-128 in directory | 0 | 72 | 0 | 0 | Yes |
| BLAKE3-64 in compressed regional header | at least 2,904,840 raw bytes | 102 | 0 | 0 | No |
| BLAKE3-128 in compressed regional header | at least 5,809,680 raw bytes | 102 | 0 | 0 | No |
| BLAKE3-64 offset-keyed extension table | 5,809,776 bytes | 102 | 0 | at least 1 uncached | Yes |
| BLAKE3-128 offset-keyed extension table | 8,714,616 bytes | 102 | 0 | at least 1 uncached | Yes |

Directory pages are fixed at 4096 bytes, so expanding an entry from 40 to 56
bytes consumes existing zero padding rather than growing the index. The measured
whole-HPRC maximum was 52 entries in one page, leaving 20 entries of headroom
under the 72-entry candidate limit. The MHC archive maximum was 32. No measured
page exceeded either checksum candidate's capacity.

The 64-bit and 128-bit alternatives use the same BLAKE3 computation and differ
only in truncation/storage. The extra eight bytes of BLAKE3-128 have no measured
archive/index/request cost in the fixed-page layout and materially reduce
accidental collision risk. Neither digest is a keyed authenticity mechanism;
the immutable object identity and published whole-object SHA-256 remain the
provenance/authenticity boundary.

On the 4,806,677-byte MHC archive, the byte-format change kept archive size
exactly constant. One-worker construction changed from 691.954 ms to 684.198 ms
and four-worker construction from 345.214 ms to 346.422 ms; those deltas are
ordinary noise and are not claimed as throughput improvements. Both new worker
counts emitted SHA-256 `164d18c2…69c70`.

The Node execution of the TypeScript reference decoder measured 20 cold-library
queries over seven selected chunks. BLAKE3-128 verification median was 1.775 ms
(mean 2.067 ms; 1.748–5.522 ms) before decompression. The cold range round read
73,320 coalesced payload bytes; the exact selected compressed payload bytes are
verified individually. Warm-cache verification is skipped because cache
insertion happens only after a digest match.

## Decision

Every 56-byte regional directory entry stores the first 128 bits of BLAKE3 over
the exact encoded payload bytes. A reader MUST compare this value before
decompression or regional parsing. Digest mismatch is corruption. Exact
duplicate physical entries must carry the same digest and are verified once.

The fixed page capacity becomes 72. An encoder must split or fail before a 73rd
entry. This is an incompatible pre-stable v1 change: the entry-size field,
directory bytes, fixtures, Rust decoder, and TypeScript decoder change together,
and pre-checksum research archives must be regenerated. There is no 40-byte
compatibility decoder.

## Consequences and safeguards

- Archive and index sizes remain unchanged for the measured MHC and whole-HPRC
  layouts because page count and payload offsets are unchanged.
- Query request count and dependency rounds do not change.
- Validation can reject encoded corruption without invoking zstd.
- Bounded parallel validation can hash before decompression using the same
  encoded buffer.
- The reduced dense-bucket ceiling is a real format cost. Generated 72/73 tests,
  encoder preflight, and retained whole-source occupancy are release gates.
- A future graph whose estimated bucket occupancy exceeds 72 must split earlier
  or fail clearly; it must not add overflow pages that break arithmetic lookup.
