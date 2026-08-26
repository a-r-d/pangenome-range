# Unified workers at whole-HPRC scale

## Verdict

Accepted. On the exact same retained HPRC source, archive options, host, and
storage layout, the encoder completed in 409.94 seconds instead of 503.19
seconds. That is a 93.25-second or 18.53% whole-command reduction. The archive
remained byte-identical: 8,829,030,376 bytes, a 47,376,777-byte bootstrap/index,
363,105 physical chunks, and SHA-256
`0b03325564dcfd558015f4722f509fd749216e30f98522c550182c1090317293`.

Peak RSS was effectively unchanged at 640,556 KiB versus 642,220 KiB. The
mandatory standard validator still completed before atomic rename. Occurrence
scratch, payload spool, and general encoder scratch remained zero.

## Changes measured

The old payload loop created a fresh scoped native thread set for each small
construction batch and another set for each compression batch. It also waited
at each construction-batch boundary. The accepted loop has one bounded pool of
exactly `--threads` workers shared by construction and compression. Construction
uses a rolling window, results may finish out of order internally, and the
coordinator restores strict coordinate order before split handling and append.
Compression uses the same workers, so the encoder never creates a second pool
that competes for the same cores.

Source SHA-256 now runs concurrently with disk-cache construction. The report
schema is 6 and distinguishes the two overlapping worker-wall measurements
from their non-overlapping combined critical-path wall. On this run, the
23.104-second checksum worker fit entirely inside the 27.598-second cache-build
worker, so these formerly serial phases cost 27.598 seconds rather than 50.702
seconds on the critical path.

## Exact comparison

| Measurement | Previous current-v1 | Unified workers | Delta |
| --- | ---: | ---: | ---: |
| Whole command | 503.191 s | 409.937 s | -93.254 s (-18.53%) |
| Prebuild | 112.899 s | 79.182 s | -33.717 s (-29.87%) |
| Payload pipeline | 328.715 s | 267.234 s | -61.482 s (-18.70%) |
| Construction including validation | 357.660 s | 297.977 s | -59.683 s (-16.69%) |
| Pre-rename validation | 28.743 s | 30.454 s | +1.711 s |
| Final output SHA-256 | 32.557 s | 32.701 s | +0.143 s |
| Peak RSS | 642,220 KiB | 640,556 KiB | -1,664 KiB |
| Peak ready raw bytes | 29,440,657 B | 18,734,465 B | -10,706,192 B |
| Voluntary context switches | 29,711,742 | 19,652,973 | -10,058,769 (-33.85%) |

The source checksum/cache overlap accounts for 23.104 seconds on this run. The
path index also measured 51.583 seconds versus 61.611 seconds, but that
single-run difference is treated as ordinary cache/system variation rather
than an implementation improvement. The payload improvement is directly tied
to the changed worker schedule and appears both in the bounded 8,192-tile pilot
(5.721 to 4.545 seconds) and in this whole-source result.

The accumulated compression wait increased from 35.101 to 58.016 seconds
because compression jobs share the bounded FIFO pool and may wait behind
already-submitted construction jobs. It is not an additional wall phase to add
to the payload pipeline. Despite that conservative scheduling, removal of the
repeated construction barriers reduced the complete payload critical path by
61.482 seconds.

`/usr/bin/time -v` independently observed 410.12 seconds, 640,556 KiB peak RSS,
484% average CPU, 19,652,973 voluntary context switches, and 191,131
involuntary context switches. The previous run observed 503.36 seconds, 395%
average CPU, 29,711,742 voluntary switches, and 358,056 involuntary switches.

## Correctness and determinism

The resulting whole archive SHA-256 is exactly the same as the retained
current-v1 archive that passed all 9/9 graph-oracle queries and all 58/58
selected tile-local weighted-haplotype comparisons. Exact byte identity makes
a second semantic-oracle run redundant rather than weaker evidence.

The focused deterministic test now covers sixteen windows instead of one
four-worker batch and proves one- and four-worker output equality. The full
Rust gate also covers the golden archive, disk-versus-loaded source identity,
format conformance, malformed inputs, validation worker counts, and the source
oracle fixtures.

## Rejected intermediate experiments

The retained raw pilot directory also records the failed steps. Merely keeping
the old batch barriers while reusing threads changed the 8,192-tile payload
phase from 5.721 to 6.663 seconds. A first pseudo-rolling two-pool version looked
good on chr6 but regressed whole payload time to 349.205 seconds, whole wall to
530.114 seconds, and RSS to 751,812 KiB. A coordinator ordering bug then
serialized a bounded pilot to 17.911 seconds. Limiting a separate compression
pool to two workers fixed oversubscription but raised compression wait. None of
those designs is retained.

A separate read-only experiment ran whole-archive SHA-256 concurrently with
standard validation. Validation remained 30.40 seconds while hashing slowed to
61.49 seconds, leaving the combined critical path no better than the serial
passes. The encoder therefore does not overlap output hashing with validation.

## Remaining opportunity

The largest measured phases are now the 267.234-second payload pipeline, the
51.583-second compact reference index, 30.454-second validation, and
32.701-second final SHA-256. Further payload work should profile source-cache
lookups and local GBWT reconstruction inside the persistent pool before adding
SIMD or GPU complexity. A single-pass validation/output-hash design could
remove a read pass, but naïve concurrent reads are explicitly rejected by the
measurement above.
