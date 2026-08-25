# Direct-writer bounded pilot report

This report retains release-mode construction evidence for the Prompt 2 direct
writer. It is an uncommitted measurement tranche on top of base commit
`9cd4b4ef8dad8cc3ebfc292b72f86cc16300c458`; it is not a whole-genome result.

Environment: rustc 1.97.1, Linux 6.17.0-41-generic x86_64, Intel Core i7-10700
(8 cores / 16 threads). Raw compact metrics are in `summary.json`.

## Commands

```bash
cargo run --release -p pangenome-range-cli -- encode \
  test-data/mhc-10.gbz /tmp/.../mhc-final-t1.pngr \
  --threads 1 --progress off --report /tmp/.../mhc-final-t1.json

cargo run --release -p pangenome-range-cli -- encode \
  test-data/mhc-10.gbz /tmp/.../mhc-final-t4.pngr \
  --threads 4 --progress off --report /tmp/.../mhc-final-t4.json

cargo run --release -p pangenome-range-cli -- encode \
  /media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/sources/hprc-v2.1-mc-grch38.gbz \
  /tmp/.../hprc-chr6-2-filtered.pngr \
  --sample GRCh38 --contig chr6 --max-chunks 2 --threads 1 \
  --progress plain --report /tmp/.../hprc-chr6-2-filtered.json

cargo run --release -p pangenome-range-cli -- \
  benchmark-fixed-window-smoke test-data/mhc-10.gbz \
  2026-08-25-direct-writer-mhc-smoke 10
```

## MHC construction

Both thread counts produced the same 3,194,336-byte object and SHA-256
`119f8e15a0681bd4418ba0eba71c590ca2b6dfc79c1625243bde05fa1358d89a`.

| Measurement | threads=1 | threads=4 |
|---|---:|---:|
| Construction | 1,754.494 ms | 1,808.041 ms |
| First payload | 11.222 ms | 14.686 ms |
| Compression | 54.149 ms | 26.521 ms |
| Payload spool / extra scratch | 0 / 0 B | 0 / 0 B |
| Peak queued raw | 565,982 B | 1,579,650 B |
| Peak queued compressed | 51,526 B | 157,258 B |

Parallel compression reduced its own phase but increased total wall time; one
thread remains the default. The separately retained smoke run passed 40 source
queries at six coalescing gaps and checked 4,338 tile payloads.
The preceding spool run used the same fixed-seed workload and passed the same
independent source-oracle gates, so old/reference and direct writers decode to
equivalent query results. The old archive SHA-256 was not retained; byte
identity is therefore asserted across current thread counts, not retroactively
for the deleted spool object.

## Bounded HPRC chr6 pilot

Only the first two 16 KiB chunks of `GRCh38#chr6` were encoded. The 5.12 GiB
source checksum took 20.824 s; it still requires full upstream deserialization
(15.304 s), a compact reference index (15.533 s), and 8,775,512 KiB
whole-process peak RSS. After those source costs, filtered manifest discovery
plus first payload took 445.932 ms; construction completed in 446.212 ms.
Encode start through first payload, including checksum, was 52.108 s.

The pilot created no occurrence index, payload spool, or additional scratch.
Its temporary final prefix was 4,266 bytes, peak raw/compressed queues were
19,282 / 194 bytes, and the final 4,644-byte archive SHA-256 was
`69703d39204afcf3856b2151b0a948d1a4e6ff81d82514bc4256219b450a1887`.

## Conclusion

Direct assembly and filtered experiments are accepted. A whole HPRC encode is
not authorized by this evidence: lazy or memory-mapped source access is the
next highest-information scale experiment, while compression parallelism is
not currently the bottleneck.
