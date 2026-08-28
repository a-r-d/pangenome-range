#!/usr/bin/env python3
"""Stream FASTA from stdin and write normalized per-path sequence hashes."""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import re
import sys


RAW_RE = re.compile(r"^(.+)\.(chr[0-9]{2})$")
PANSN_RE = re.compile(r"^(.+)#[0-9]+#(chr[0-9]{2})(?:#[^#]+)?$")
REFERENCE_RE = re.compile(r"^(.+)#(chr[0-9]{2})$")
MINIGRAPH_RE = re.compile(r"^_MINIGRAPH_\.s[0-9]+$")


def normalize(name: str) -> str:
    name = name.split()[0]
    match = RAW_RE.fullmatch(name)
    if match:
        return f"{match.group(1)}.{match.group(2)}"
    match = PANSN_RE.fullmatch(name)
    if match:
        return f"{match.group(1)}.{match.group(2)}"
    match = REFERENCE_RE.fullmatch(name)
    if match:
        return f"{match.group(1)}.{match.group(2)}"
    if MINIGRAPH_RE.fullmatch(name):
        return name
    raise RuntimeError(f"unsupported FASTA path name {name!r}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    records: list[tuple[str, int, str]] = []
    current: str | None = None
    digest = hashlib.sha256()
    length = 0

    def finish() -> None:
        nonlocal current, digest, length
        if current is not None:
            records.append((current, length, digest.hexdigest()))
        current = None
        digest = hashlib.sha256()
        length = 0

    for raw_line in sys.stdin.buffer:
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith(b">"):
            finish()
            current = normalize(line[1:].decode("utf-8"))
        else:
            if current is None:
                raise RuntimeError("sequence data appeared before the first FASTA header")
            sequence = line.upper()
            digest.update(sequence)
            length += len(sequence)
    finish()
    records.sort()
    names = [record[0] for record in records]
    if len(names) != len(set(names)):
        raise RuntimeError("normalized FASTA path names are not unique")
    with args.output.open("w") as output:
        output.write("normalized_path_name\tsequence_bytes\tsha256\n")
        for record in records:
            output.write(f"{record[0]}\t{record[1]}\t{record[2]}\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
