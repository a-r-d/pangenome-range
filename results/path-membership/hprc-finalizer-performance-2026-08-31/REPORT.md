# HPRC named-membership finalizer performance

## Outcome

The bounded finalizer is now deterministic, cancellable, progress-reporting, and
substantially faster, but it does **not** meet the requested 20-minute whole-HPRC
target. A full run was therefore not launched.

On the same 256-tile GRCh38 chr5 interval, one worker took about 65 seconds in the
membership phase. Ordered eight-worker execution reduced that to about 16 seconds.
Resolving only requested offsets while scanning each compressed GBWT record once
reduced it to about 9 seconds. With 32 bounded workers, the final retained design
completed the phase in about 5.8 seconds (44.3 tiles/s), used 1,970,640 KiB peak RSS,
and did not swap.

Every completed variant produced the identical 12,834,459-byte archive at SHA-256
`e01657069a335b5abf67ff137d6a4199ac58bab8fede20598fa26f041985fa17`.
The result contains 256 membership pages, 76,862 groups, 169,223 memberships, and
occurrence total 169,224. The maximum observed LF distance remained 1,023.

## Why this is not 20 minutes yet

The retained whole anonymous archive has 363,105 physical tiles. A linear projection
at 44.3 tiles/s is about 137 minutes for membership alone. Twenty minutes would
require about 303 tiles/s, another 6.8-fold improvement. This is a projection from
one bounded interval, not a claimed whole-archive measurement, but it is far enough
from the target that launching the full job would be irresponsible.

The remaining boundary is algorithmic: the embedded document-array samples are up
to 1,023 LF steps away. More workers flatten after 32. A 64-worker run also took
about six seconds and used 2,908,252 KiB RSS. Larger 128 KiB tiles reduced locate
starts but shifted time into traversal reconstruction and validation; 32 workers
reached 4.85 GB and were stopped, while eight workers still took about nine seconds.

The following prototypes were measured and removed because they did not improve the
end-to-end phase: 32-tile two-pass batching, cross-tile continuation caches, internal
DA-sample scanning, larger source caches, and unordered hash grouping.

## Retained implementation

- Ordered, bounded per-tile workers preserve byte-identical output.
- Progress includes tiles, rate, ETA, membership counts, LF maximum, and output size.
- Record LF resolves selected offsets directly from compressed runs instead of
  expanding the complete record.
- The HPRC script uses 32 workers, a hard 5 GiB/no-swap cgroup, a 30-minute default
  timeout, durable progress logging, and explicit transient-unit cleanup on signals.

The next credible route to the 20-minute target is a source-authenticated precomputed
r-index/FastLocate-class side structure or a similarly direct sequence-ID mapping.
The official HPRC directory does not publish a `.ri` file, and building the standard
in-memory structure previously exceeded safe memory. That builder needs a separate
bounded/external-memory design before a whole run is authorized.
