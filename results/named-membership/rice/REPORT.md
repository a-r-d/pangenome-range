# Rice named-membership bounded pilot

The final optional-v1 layout passed a one-tile Xa7 encode and full validation under a hard 4 GiB address-space cap. Named mode recovered 60 occurrences across 51 canonical groups; membership multiplicity also totals 60. The 44 referenced canonical path IDs cover 25 biological accession labels and 19 `_gbwt_ref` fragments.

The named accessions are ARC10497, Basmati, GOBOLSAIL, KHAOYAIGUANG, LIUXU, Nagina22, Sadri, TG11, TG12, TG13, TG15, TG28, TG30, TG49, TG5, TG54, TG6, TG61, TG62, TG70, TG81, WW8, wild111, wild219, and wild65. The largest single local traversal has seven named paths and seven occurrences.

This does not equal PPanG's published “16 of 113 genomes align to Xa7” statement. The tile includes flanks, generic fragments, partial paths, and 51 local traversal definitions; the paper's Xa7 branch predicate is not encoded here. The result therefore reports exact tile-local source membership without manufacturing an allele interpretation.

Named encoding added 540.90 ms (3.49%) and 1,884 KiB peak RSS over the anonymous control. Its fixed global catalog makes the one-tile archive 170.7% larger. That bounded delta does not establish chromosome-wide storage overhead. The retained chromosome archive has 61 graph directory pages, so the mirrored membership directories alone would add 249,856 bytes (0.0767%) before membership tile pages, the catalog, descriptor, compression, and metadata. Graph-only semantics and dependency rounds are unchanged; registering the optional extension adds 64 fetched bytes. After provenance cross-checking was added, the identity query fetched 12,299 membership/provenance bytes and 10,586 catalog bytes.

Reproduce with the bounded commands in `results/path-membership/production/REPORT.md`, then run `experiments/named-membership-query.mjs` against the generated object. Large inputs and archives are intentionally not retained.
