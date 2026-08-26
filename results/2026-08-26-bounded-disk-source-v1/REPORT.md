# Project-owned bounded GBZ source

## Verdict

Accept the disk-backed source adapter as the default production encoder path.
The final whole-HPRC run produced exactly the loaded release candidate's bytes
while reducing peak RSS from 8,775,928 KiB (8.37 GiB) to 608,060 KiB
(0.580 GiB), a 93.07% reduction.

The measured trade is 11,921,858,427 bytes (11.10 GiB) of ephemeral source
cache and 60.62 seconds of additional whole-command wall time. The exact
same-source/options/host comparison is 438.72 seconds loaded versus 499.34
seconds disk-backed, a 13.82% penalty. This is an encoder implementation
change, not a `.pngr` byte-format change.

`gbz-base` remains an independent source oracle and research baseline. The
production encoder no longer calls its path index, subgraph, or record wrapper.

## What changed

- `DiskGbzSource` parses the locked GBZ/simple-sds sections and streams packed
  records plus decoded concatenated sequences to temporary disk.
- Four arithmetic offset/data files use a 64 MiB total explicit cache. Each
  cache is split into 16 fixed shards so parallel workers do not serialize on
  one file/cache mutex.
- The source interface provides a length-only sequence lookup, so reference
  indexing does not materialize sequence bodies.
- `SourcePathIndex` samples only real reference paths. It is not a global
  haplotype occurrence index.
- `LocalSubgraph` walks reference intervals and expands bidirectional context
  directly from exact packed GBWT records.
- `LoadedGbzSource` remains available with `--source-access loaded` as the
  correctness baseline; `disk` is the default.

## Whole-HPRC result

Source:

```text
bytes:   5,492,627,216
sha256:  11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b
```

Both the loaded release candidate and final disk-backed encoder produced:

```text
archive bytes:  8,828,788,418
index bytes:       47,376,617
entries:              363,105
sha256:  76ae6616d296af1c270420ecbaa1fdb1dfa80f28645d72df589a31d2f0f0121e
```

| Measurement | Loaded release candidate | Final disk-backed | Difference |
| --- | ---: | ---: | ---: |
| Whole command | 438.72 s | 499.34 s | +13.82% |
| Prebuild | 51.260 s | 95.034 s | +43.774 s |
| Construction including validation | 354.481 s | 369.561 s | +4.25% |
| Payload pipeline | about 326.358 s | 337.140 s | +3.30% |
| Pre-rename validation | 28.123 s | 32.221 s | +4.098 s |
| Output SHA-256 | 32.520 s | 34.493 s | +1.973 s |
| Peak RSS | 8,775,928 KiB | 608,060 KiB | -93.07% |
| Source scratch | 0 B | 11,921,858,427 B | +11.10 GiB |

The disk-backed prebuild separates 21.350 seconds for source SHA-256, 24.412
seconds for streaming the cache, and 49.272 seconds for the project-owned
reference index. RSS was 33,560 KiB after the cache build and 183,972 KiB after
the reference index. The source read cache has an explicit 67,108,864-byte
ceiling and the ephemeral directory was removed on exit.

The encoder still used zero occurrence-index, payload-spool, and general
encoder scratch bytes. Its peak queued raw-plus-compressed payload data was
35,562,074 bytes. The archive passed the required standard structural,
integrity, decompression, and regional decode gate before atomic rename.

## Hill-climbing evidence

The first correct disk prototype used one small seek per source access. It
peaked at 407,756 KiB and produced the exact pilot archive, but reference
indexing took 378.746 seconds; it was rejected.

A single shared 64 MiB block cache reduced reference indexing to about 56
seconds, but a first whole-source run exposed worker serialization. It completed
in 580.43 seconds with 622,000 KiB peak RSS. Sharding the same cache budget and
adding length-only lookup reduced the final whole run to 499.34 seconds and
608,060 KiB without changing bytes.

A final bounded chr6 run encoded 1,024 consecutive 16 KiB windows covering
16,777,216 bp. It wrote the retained 26,156,797-byte archive with SHA-256
`239242b09acc247601ff58829002697cfd74bcebfe1b88bf457512f611604f46`.
Construction took 912.557 ms (0.891 ms per entry), including 61.718 ms of
validation, while peak RSS remained 407,964 KiB.

MHC encodes at one and four workers and the loaded one-worker encoder all
produced the retained 4,806,677-byte archive with SHA-256
`164d18c254cae1e52bfed5a6cd53ea9d48c8d14ab50dbcc85d5e3b54f5569c70`.
Focused tests also compare disk and loaded records, sequences, references, full
archive bytes, and the project-owned payload against the `gbz-base` JSON oracle.

## Limits and next gate

The cache builder temporarily loads one compact simple-sds offset index at a
time. Memory therefore scales with compact record/sequence metadata, not with
the source bodies; it is not mathematically constant for arbitrary graph size.
The measured 608,060 KiB whole-HPRC peak is the current evidence bound.

A whole 1000GP encode is still not authorized. The next responsible experiment
is a bounded 1000GP pilot that measures source-cache expansion, compact-index
RSS, construction throughput, and required disk headroom. A full run requires
that pilot to demonstrate the expected resource budget first.
