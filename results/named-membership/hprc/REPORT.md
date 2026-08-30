# HPRC named-membership production result

The final optional-v1 layout passed a four-tile TERT encode and full reconstruction under a hard 4 GiB address-space cap. It recovered 4,257 membership records in 1,686 canonical groups. Occurrence weight and membership multiplicity both total 4,257; 464 canonical paths across 232 sample labels are referenced.

Named encoding added 2.541 s (2.57%) over the anonymous control. The higher of the two observed peaks was 653,596 KiB, below the 1 GiB release target, with zero swap. Both modes wrote the same 11,922,586,123-byte bounded ephemeral source cache, so named mode added no persistent cache.

The fixed catalog dominates this four-tile object's 298% size increase. The 742,682-byte increment is 0.00841% of the retained 8.83 GB whole-HPRC archive, below the 15% preferred target. This is an amortized projection, not an archive-wide named encode.

Anonymous and named graph queries have identical canonical hashes and two dependency rounds. The registered extension adds 64 graph-query bytes. Identity adds 49,855 membership bytes and 40,105 catalog bytes; the current lazy layout requires two membership rounds followed by one catalog round. This misses the aspirational single-extra-round target and is why identity remains experimental.

The final-layout rerun uses TERT as the simple/control interval. Earlier retained, independently checked bounded evidence covers HLA-B, MICB, and the cyclic/repetitive KIR3DL1 region in `results/path-membership/hprc/`; it is not relabeled as a final-layout rerun. The synthetic golden additionally covers the MICB/KIR3DL1 fixture.
