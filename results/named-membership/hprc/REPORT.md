# HPRC named-membership bounded pilot

The final optional-v1 layout passed a four-tile TERT encode and full reconstruction under a hard 4 GiB address-space cap. It recovered 4,257 membership records in 1,686 canonical groups. Occurrence weight and membership multiplicity both total 4,257; 464 canonical paths across 232 sample labels are referenced.

Named encoding added 2.541 s (2.57%) over the anonymous control. The higher of the two observed peaks was 653,596 KiB, below the 1 GiB release target, with zero swap. Both modes wrote the same 11,922,586,123-byte bounded ephemeral source cache, so named mode added no persistent cache.

The fixed catalog dominates this four-tile object's 298% size increase. This bounded delta does not establish archive-wide storage overhead. A complete object would require 11,559 fixed 4 KiB membership-directory pages, or 47,345,664 bytes (0.536% of the retained 8.83 GB archive), before adding 363,105 membership tile pages, the catalog, descriptor, compression, and metadata. No whole-HPRC storage percentage is claimed.

Anonymous and named graph queries have identical canonical hashes and two dependency rounds. The registered extension adds 64 graph-query bytes. After provenance cross-checking was added, identity adds 50,081 membership/provenance bytes and 40,105 catalog bytes; the current lazy layout requires two membership rounds followed by one catalog round. This misses the aspirational single-extra-round target and is why identity remains experimental.

The final-layout rerun uses TERT as the simple/control interval. Earlier retained, independently checked bounded evidence covers HLA-B, MICB, and the cyclic/repetitive KIR3DL1 region in `results/path-membership/hprc/`; it is not relabeled as a final-layout rerun. The synthetic golden additionally covers the MICB/KIR3DL1 fixture.
