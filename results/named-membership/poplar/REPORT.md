# Poplar chromosome 19 named-membership demo candidate

The Populus trichocarpa corpus passed a two-stage, memory-capped local proof. A
two-tile Chr16 canary first recovered 780 exact memberships in 399 canonical groups
with 89,784 KiB peak RSS. The complete 16,626,325-base Chr19 reference then encoded
1,015 tiles under a strict 4 GiB memory and zero-swap cgroup. The full run peaked at
170,772 KiB RSS, reached its first payload in 501 ms, and finished the encoder report
in 124.94 s.

The whole-chromosome archive contains 2,213 catalog paths, 97,272 canonical groups,
183,057 membership records, and occurrence weight 183,061. Multiplicity reconciled
for every group during exhaustive validation. Locate covered 368,152 GBWT positions
in 42.84 s and required at most 1,023 LF steps, below the configured 8,192-step hard
guard. Full validation reconstructed all 1,015 payloads successfully. A 64 MiB
validation queue first rejected the largest estimated job safely; the retained full
validation used a 512 MiB admission budget and did not weaken allocation checks.

The local archive is 75,902,344 bytes at SHA-256
`baf33e2e181efa4485f8ea2a253b24e4bda08ef5725c4ddf9585b495ddafe6ae`.
The registered extensions occupy 2,622,392 encoded bytes. There is no anonymous
whole-Chr19 control, so this is not reported as a named-mode percentage overhead.

The proposed demo interval is `Nisqually-1#Chr19:6291456-6324224`. Independent
source verification agrees on two tiles, 10,912 nodes, and canonical hash
`8fac6dedb14fdca24ed68659997cea9662896033b15e047ecd8a732fdc80e5cd`.
The identity query returns 412 traversal groups, 886 memberships, 89 source paths,
and all 45 sample labels. It fetches 15,413 membership bytes and 22,889 catalog bytes
in four cold identity waves. This is a structurally dense coordinate window, not a
claimed sex-determination interval. The GBZ has no phenotype sidecar, so the demo
does not label paths male or female.

This result is additional plant-genome evidence, not a substitute for the excluded
5,008-haplotype 1000G pilot. The source files are publicly downloadable, but their
project page and README do not state a redistribution license. The derived archive
therefore remains local and the Pages integration is opt-in until reuse permission
is confirmed.
