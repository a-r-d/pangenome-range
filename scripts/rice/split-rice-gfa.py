#!/usr/bin/env python3
"""Split PPanG GFA paths without changing graph records or path identity."""

from __future__ import annotations

import argparse
import json
import pathlib
import re


ACCESSION_PATH_RE = re.compile(r"^(.+)\.(chr[0-9]{2})$")
MINIGRAPH_PATH_RE = re.compile(r"^_MINIGRAPH_\.s[0-9]+$")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=pathlib.Path, required=True)
    parser.add_argument("--accession-output", type=pathlib.Path, required=True)
    parser.add_argument("--internal-output", type=pathlib.Path, required=True)
    parser.add_argument("--json", type=pathlib.Path, required=True)
    args = parser.parse_args()
    counts = {"sharedGraphLines": 0, "accessionPathLines": 0, "reviewedMinigraphInternalPathLines": 0}
    args.accession_output.parent.mkdir(parents=True, exist_ok=True)
    args.internal_output.parent.mkdir(parents=True, exist_ok=True)
    with (
        args.input.open(errors="strict") as source,
        args.accession_output.open("w") as accession,
        args.internal_output.open("w") as internal,
    ):
        for line in source:
            if line.startswith("P\t"):
                name = line.split("\t", 2)[1]
                if ACCESSION_PATH_RE.fullmatch(name):
                    fields = line.rstrip("\n").split("\t")
                    fields[1] = f"{name}.0"
                    accession.write("\t".join(fields) + "\n")
                    counts["accessionPathLines"] += 1
                elif MINIGRAPH_PATH_RE.fullmatch(name):
                    internal.write(line)
                    counts["reviewedMinigraphInternalPathLines"] += 1
                else:
                    raise RuntimeError(f"unhandled P-line path name {name!r}")
            elif line.startswith("W\t"):
                raise RuntimeError("unexpected W-line in -fW export")
            else:
                accession.write(line)
                internal.write(line)
                counts["sharedGraphLines"] += 1
    result = {
        "schemaVersion": 1,
        **counts,
        "sourceGfaBytes": args.input.stat().st_size,
        "accessionGfaBytes": args.accession_output.stat().st_size,
        "internalGfaBytes": args.internal_output.stat().st_size,
        "accessionMetadataTransform": "append .0 to each temporary accession P-line name and parse it as technical haplotype 0",
        "sourceIdentityPreservation": "raw source names remain in the XG and full GFA; the GBZ sample and contig fields preserve accession and chr06",
        "passed": counts["accessionPathLines"] > 0 and counts["reviewedMinigraphInternalPathLines"] > 0,
    }
    args.json.write_text(json.dumps(result, indent=2) + "\n")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
