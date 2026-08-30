# Integrated encoder path-membership proof

Status: bounded local proof; not a format or publication decision.

The native encoder now accepts a prepared membership summary and matching complete
catalog through two paired experimental flags. It writes a paged path catalog and
one independently compressed membership page per selected tile into the same atomic
`.pngr` temporary object. The normal encoder emits neither page type. The pre-rename
validator reads every page, reconstructs the unchanged anonymous regional traversals,
and requires an exact `(traversal digest, occurrence weight)` match.

## Small correctness and determinism gate

The `micb-kir3dl1.gbz` fixture produced a 31,895-byte archive containing 169 catalog
records, one membership tile, 64 traversal groups, and 90 membership records. One-
and four-worker runs were byte-identical at SHA-256
`85f96c2e4eb267adf9484c3f9eb0e27924218505c41388f73d65fd61d61f4db4`.

## Bounded HPRC gate

The real HPRC test encoded four aligned 16,384 bp TERT tiles covering
`GRCh38#chr5:1245184-1310720`. The prepared source workload contained 8,522 GBWT
positions. Rust locate loaded the 4,051,134,848-byte standalone GBWT once and finished
under a 12 GiB address-space limit at 4,805,116 KiB peak RSS, zero swaps, and 1,023
maximum LF steps.

The direct release encoder ran with one worker, a 128 MiB queue, and a 4 GiB
address-space limit. It produced a 987,840-byte archive at SHA-256
`3f8af7f829a21b6c2d787a2eac39cf5015a07425ea9a3e8449b4c8ef71b846ac`.
Peak RSS was 263,828 KiB and swaps were zero. The 20.62 s whole wall was dominated by
19.65 s authenticating the 5.49 GB source; archive construction itself took 406 ms
and wrote its first regional payload 4.1 ms after build start.

The extension contains 53,150 catalog records in 52 pages plus four tile pages with
1,686 groups and 4,257 memberships. Its catalog/tile pages occupy 734,988 encoded
bytes and its descriptor occupies 3,752 bytes. Full validation succeeded under a
1 GiB address-space cap at 57,656 KiB peak RSS and zero swaps. The first 128 MiB
validation queue attempt was deliberately rejected before work because its
conservative single-job estimate was 286,150,507 bytes; the successful run used one
worker and a 384 MiB queue.

Chromium, Firefox, and WebKit then opened the same archive through a strict local
range origin. For `GRCh38#chr5:1261568-1277952`, each browser recovered 689 groups,
1,570 membership occurrences, and 464 exact catalog records through four `206`
requests totaling 76,869 bytes. This is loopback functional evidence, not an
internet-latency result.

## Boundary and next experiment

This tranche packages already prepared, source-validated membership data. It does
not yet call the unpublished local `gbwt-rs` locate implementation from the encoder,
and the public TypeScript reader intentionally skips the optional unregistered
`path-members-v1-` entry. The 65,536 tile-page safety bound also prevents treating
this implementation as a whole-genome projection.

The next high-information experiment is to give the encoder a stable bounded locate
source, build one tile's memberships at a time, and release that state before moving
forward. A larger bounded chromosome pilot should follow; another whole-genome run
should not.

Exact retained numbers are in `summary.json`. Large GBWT, prepared membership, and
generated `.pngr` artifacts remain outside the repository.
