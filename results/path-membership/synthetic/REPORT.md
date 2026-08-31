# Synthetic named-membership result

The independent C++ oracle recovered the complete 56-occurrence document-array
mapping exactly. `FastLocate::decompressDA()` and an independent walk of all ten
stored GBWT sequences agreed at every `(oriented node, record offset)`. The checked-in
56-row `expected-node-da.tsv` also fixes the expected sequence ID, canonical path ID,
and stored orientation for manual inspection.

The fixture has the required reference plus `SAMPLE_A` insertion, `SAMPLE_B`
deletion-like skip, `SAMPLE_C` reverse traversal, and `SAMPLE_D` revisit. In
particular, `SAMPLE_D` occurs twice in both forward node 2 and forward node 3 records,
which is the source evidence for two fragments when the active region is restricted
to those nodes. The full 20 bp production tile keeps that path continuous, so its
four anonymous groups happen to form a disjoint partition; that result must not be
generalized.

The unchanged 5,071-byte v1 archive reconstructed four anonymous groups with total
weight four. Joining the oracle identities and subtracting exactly one real reference
occurrence preserved every production group weight. The exact catalog estimates are
154 bytes for both front-coded and columnar variants; memberships are 20 bytes. A
same-object extension model is 294 bytes (5.80%) including a 120-byte experimental
index. The adaptive codec chose dense bitset for all four singleton groups. Chromium
decoded the 68-byte framed corpus 1,000 times in 4.0 ms, or 0.004 ms per corpus.

This tier passes source identity, brute-force equality, grouping, reference
subtraction, codec round trips, and browser decoding. It does not establish a
population-scale extraction mechanism.

The experimental Rust ordinary-locate implementation also matches all 56 rows
exactly without reading an `.ri`. The current local-fork path loads the GBWT once,
decoded 10 embedded samples from a 304-byte DA option, used 3,076,096 bytes peak RSS,
and needed at most 7 LF steps. A fork regression test also proves exact passthrough
serialization of the original DA option. The earlier two-pass result is retained in
`rust-locate.json`; the current result is `rust-locate-single-pass.json`. Both
measurements used the release binary directly, so compile memory is excluded.
