#!/usr/bin/env python3
"""Audit PPanG XG, GFA, and GBZ path identity metadata."""

from __future__ import annotations

import argparse
import csv
import json
import pathlib
import re
import sys


ACCESSION_PATH_RE = re.compile(r"^(.+)\.(chr[0-9]{2})$")
MINIGRAPH_PATH_RE = re.compile(r"^(_MINIGRAPH_)\.(s[0-9]+)$")
REQUIRED = {"IRGSP-1.0.chr06", "NATELBORO.chr06"}


def read_names(path: pathlib.Path) -> list[str]:
    names = [line.strip() for line in path.read_text().splitlines() if line.strip()]
    if len(names) != len(set(names)):
        raise RuntimeError(f"duplicate path names in {path}")
    return names


def read_graph_stats(format_path: pathlib.Path, size_path: pathlib.Path, graph_path: pathlib.Path) -> dict[str, object]:
    format_text = format_path.read_text().strip()
    graph_format = format_text.split(":", 1)[1].strip() if ":" in format_text else format_text
    values: dict[str, object] = {}
    for line in size_path.read_text().splitlines():
        if "\t" not in line:
            continue
        key, value = line.split("\t", 1)
        values[key] = int(value) if value.isdigit() else value
    return {
        "graphFormat": graph_format,
        "graphBytes": graph_path.stat().st_size,
        "nodeCount": values.get("nodes"),
        "edgeCount": values.get("edges"),
    }


def audit_xg(
    names_path: pathlib.Path,
    tsv_path: pathlib.Path,
    json_path: pathlib.Path,
    format_path: pathlib.Path,
    size_path: pathlib.Path,
    graph_path: pathlib.Path,
) -> None:
    names = read_names(names_path)
    rows = []
    accession_count = 0
    minigraph_count = 0
    exceptions = []
    for name in names:
        match = ACCESSION_PATH_RE.fullmatch(name)
        if match:
            rows.append((name, match.group(1), match.group(2), "accession"))
            accession_count += 1
            continue
        match = MINIGRAPH_PATH_RE.fullmatch(name)
        if match:
            rows.append((name, match.group(1), match.group(2), "reviewed_minigraph_internal"))
            minigraph_count += 1
        else:
            rows.append((name, "", "", "unmatched"))
            exceptions.append(name)
    missing = sorted(REQUIRED - set(names))
    exception_fraction = len(exceptions) / len(names) if names else 1.0
    with tsv_path.open("w", newline="") as output:
        writer = csv.writer(output, delimiter="\t", lineterminator="\n")
        writer.writerow(["raw_path_name", "parsed_sample", "parsed_contig", "parse_status"])
        writer.writerows(rows)
    graph = read_graph_stats(format_path, size_path, graph_path)
    audit = {
        "schemaVersion": 1,
        **graph,
        "recognizedAsXg": graph["graphFormat"] == "XG",
        "pathCount": len(names),
        "accessionPathCount": accession_count,
        "reviewedMinigraphInternalPathCount": minigraph_count,
        "reviewedMinigraphInternalPattern": "^_MINIGRAPH_[.]s[0-9]+$",
        "reviewedMinigraphHandling": "preserve verbatim as generic GBWT paths; do not treat as rice accessions",
        "exceptionCount": len(exceptions),
        "exceptionFraction": exception_fraction,
        "exceptions": exceptions,
        "requiredPaths": sorted(REQUIRED),
        "missingRequiredPaths": missing,
        "passed": graph["graphFormat"] == "XG" and bool(graph["nodeCount"]) and accession_count > 0 and minigraph_count > 0 and not missing and exception_fraction <= 0.01,
    }
    json_path.write_text(json.dumps(audit, indent=2) + "\n")
    if not audit["passed"]:
        raise RuntimeError(f"XG path audit failed; see {json_path}")


def audit_gfa(gfa_path: pathlib.Path, names_path: pathlib.Path, json_path: pathlib.Path) -> None:
    expected = set(read_names(names_path))
    paths: set[str] = set()
    duplicates: list[str] = []
    segment_count = 0
    edge_count = 0
    min_node: int | None = None
    max_node: int | None = None
    nonnumeric_segments: list[str] = []
    w_lines = 0
    headers: list[str] = []
    with gfa_path.open(errors="strict") as source:
        for line in source:
            if line.startswith("H\t"):
                if len(headers) < 20:
                    headers.append(line.rstrip("\n"))
            elif line.startswith("S\t"):
                segment_count += 1
                name = line.split("\t", 2)[1]
                try:
                    node = int(name)
                except ValueError:
                    if len(nonnumeric_segments) < 100:
                        nonnumeric_segments.append(name)
                else:
                    min_node = node if min_node is None else min(min_node, node)
                    max_node = node if max_node is None else max(max_node, node)
            elif line.startswith("L\t"):
                edge_count += 1
            elif line.startswith("P\t"):
                name = line.split("\t", 2)[1]
                if name in paths:
                    duplicates.append(name)
                paths.add(name)
            elif line.startswith("W\t"):
                w_lines += 1
    missing = sorted(expected - paths)
    extra = sorted(paths - expected)
    audit = {
        "schemaVersion": 1,
        "gfaBytes": gfa_path.stat().st_size,
        "headerLines": headers,
        "segmentCount": segment_count,
        "edgeCount": edge_count,
        "minimumNodeId": min_node,
        "maximumNodeId": max_node,
        "nonnumericSegmentIds": nonnumeric_segments,
        "pLineCount": len(paths),
        "wLineCount": w_lines,
        "duplicatePathNames": duplicates,
        "missingPathNames": missing,
        "extraPathNames": extra,
        "passed": segment_count > 0 and not nonnumeric_segments and not duplicates and not missing and not extra and w_lines == 0,
    }
    json_path.write_text(json.dumps(audit, indent=2) + "\n")
    if not audit["passed"]:
        raise RuntimeError(f"GFA audit failed; see {json_path}")


def audit_gbz(
    metadata_path: pathlib.Path,
    names_path: pathlib.Path,
    tsv_path: pathlib.Path,
    json_path: pathlib.Path,
    format_path: pathlib.Path,
    size_path: pathlib.Path,
    graph_path: pathlib.Path,
) -> None:
    expected_names = read_names(names_path)
    expected_accessions = {}
    expected_internal = set()
    for name in expected_names:
        match = ACCESSION_PATH_RE.fullmatch(name)
        if match:
            expected_accessions[(match.group(1), match.group(2))] = name
            continue
        if MINIGRAPH_PATH_RE.fullmatch(name):
            expected_internal.add(name)
            continue
        raise RuntimeError(f"cannot map unmatched source path {name!r}")
    with metadata_path.open(newline="") as source:
        reader = csv.DictReader((line for line in source if not line.startswith("#") or line.startswith("#NAME")), delimiter="\t")
        if reader.fieldnames and reader.fieldnames[0].startswith("#"):
            reader.fieldnames[0] = reader.fieldnames[0][1:]
        rows = list(reader)
    output_rows = []
    seen_accessions: dict[tuple[str, str], int] = {}
    seen_internal: dict[str, int] = {}
    unexpected_rows: list[str] = []
    for row in rows:
        sample = row.get("SAMPLE", "")
        contig = row.get("LOCUS", "")
        key = (sample, contig)
        name = row.get("NAME", "")
        sense = row.get("SENSE", "")
        if key in expected_accessions and sense in {"REFERENCE", "HAPLOTYPE"}:
            raw = expected_accessions[key]
            seen_accessions[key] = seen_accessions.get(key, 0) + 1
            status = "matched_accession"
        elif name in expected_internal and sense == "GENERIC" and contig == name:
            raw = name
            seen_internal[name] = seen_internal.get(name, 0) + 1
            status = "matched_reviewed_minigraph_internal"
        else:
            raw = ""
            status = "unexpected"
            unexpected_rows.append(name or f"{sample}.{contig}")
        output_rows.append({
            "raw_path_name": raw,
            "gbz_path_name": name,
            "sense": sense,
            "sample": sample,
            "haplotype": row.get("HAPLOTYPE", ""),
            "contig": contig,
            "phase_block": row.get("PHASE_BLOCK", ""),
            "subrange": row.get("SUBRANGE", ""),
            "status": status,
        })
    with tsv_path.open("w", newline="") as output:
        fieldnames = list(output_rows[0]) if output_rows else ["raw_path_name"]
        writer = csv.DictWriter(output, fieldnames=fieldnames, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(output_rows)
    missing = sorted(name for key, name in expected_accessions.items() if key not in seen_accessions)
    missing.extend(sorted(name for name in expected_internal if name not in seen_internal))
    duplicates = sorted(f"{sample}.{contig}" for (sample, contig), count in seen_accessions.items() if count != 1)
    duplicates.extend(sorted(name for name, count in seen_internal.items() if count != 1))
    samples = sorted({row.get("SAMPLE", "") for row in rows if row.get("SAMPLE") != "NO_SAMPLE_NAME"})
    references = sorted({row.get("SAMPLE", "") for row in rows if row.get("SENSE") == "REFERENCE"})
    accession_rows = [row for row in rows if row.get("SENSE") in {"REFERENCE", "HAPLOTYPE"}]
    generic_rows = [row for row in rows if row.get("SENSE") == "GENERIC"]
    accession_haplotypes = sorted({row.get("HAPLOTYPE", "") for row in accession_rows})
    graph = read_graph_stats(format_path, size_path, graph_path)
    audit = {
        "schemaVersion": 1,
        **graph,
        "recognizedAsGbz": graph["graphFormat"] == "GBZ",
        "pathCount": len(rows),
        "gbwtBidirectionalSequenceCount": len(rows) * 2,
        "gbwtSequenceCountDerivation": "two stored orientations per forward GBWT path",
        "sampleCount": len(samples),
        "samples": samples,
        "biologicalAccessionSampleCount": len({row.get("SAMPLE", "") for row in accession_rows}),
        "accessionHaplotypeValues": accession_haplotypes,
        "accessionHaplotypeQualification": "technical 0 assigned because each source accession has one chr06 path; no additional individual identity inferred",
        "reviewedMinigraphInternalPathCount": len(generic_rows),
        "referenceSamples": references,
        "missingSourcePaths": missing,
        "unexpectedPaths": sorted(unexpected_rows),
        "duplicateOrFragmentedMetadataKeys": duplicates,
        "genericSentinelUse": "vg uses _gbwt_ref internally for the reviewed _MINIGRAPH_.s<number> generic paths only; accession rows are independently checked as structured",
        "allAccessionPathsStructured": len(accession_rows) == len(expected_accessions),
        "allReviewedMinigraphPathsGeneric": len(generic_rows) == len(expected_internal),
        "requiredSamplesPresent": all(sample in samples for sample in ["IRGSP-1.0", "NATELBORO"]),
        "requiredReferencesPresent": all(sample in references for sample in ["IRGSP-1.0", "NATELBORO"]),
        "passed": graph["graphFormat"] == "GBZ" and bool(graph["nodeCount"]) and len(rows) == len(expected_names) and not missing and not unexpected_rows and not duplicates and len(accession_rows) == len(expected_accessions) and accession_haplotypes == ["0"] and len(generic_rows) == len(expected_internal) and all(sample in references for sample in ["IRGSP-1.0", "NATELBORO"]),
    }
    json_path.write_text(json.dumps(audit, indent=2) + "\n")
    if not audit["passed"]:
        raise RuntimeError(f"GBZ metadata audit failed; see {json_path}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="mode", required=True)
    xg = subparsers.add_parser("xg")
    xg.add_argument("--names", type=pathlib.Path, required=True)
    xg.add_argument("--tsv", type=pathlib.Path, required=True)
    xg.add_argument("--json", type=pathlib.Path, required=True)
    xg.add_argument("--format", type=pathlib.Path, required=True)
    xg.add_argument("--size", type=pathlib.Path, required=True)
    xg.add_argument("--graph", type=pathlib.Path, required=True)
    gfa = subparsers.add_parser("gfa")
    gfa.add_argument("--gfa", type=pathlib.Path, required=True)
    gfa.add_argument("--names", type=pathlib.Path, required=True)
    gfa.add_argument("--json", type=pathlib.Path, required=True)
    gbz = subparsers.add_parser("gbz")
    gbz.add_argument("--metadata", type=pathlib.Path, required=True)
    gbz.add_argument("--names", type=pathlib.Path, required=True)
    gbz.add_argument("--tsv", type=pathlib.Path, required=True)
    gbz.add_argument("--json", type=pathlib.Path, required=True)
    gbz.add_argument("--format", type=pathlib.Path, required=True)
    gbz.add_argument("--size", type=pathlib.Path, required=True)
    gbz.add_argument("--graph", type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.mode == "xg":
        audit_xg(args.names, args.tsv, args.json, args.format, args.size, args.graph)
    elif args.mode == "gfa":
        audit_gfa(args.gfa, args.names, args.json)
    else:
        audit_gbz(args.metadata, args.names, args.tsv, args.json, args.format, args.size, args.graph)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
