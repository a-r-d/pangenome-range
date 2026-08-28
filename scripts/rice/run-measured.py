#!/usr/bin/env python3
"""Run one command while sampling wall time, process-tree RSS, and temp bytes."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import signal
import subprocess
import sys
import time


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def process_table() -> dict[int, tuple[int, int]]:
    table = {}
    for entry in pathlib.Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / "stat").read_text().split()
            status = (entry / "status").read_text().splitlines()
            rss_kib = next(int(line.split()[1]) for line in status if line.startswith("VmRSS:"))
            table[int(entry.name)] = (int(fields[3]), rss_kib * 1024)
        except (FileNotFoundError, PermissionError, StopIteration, ValueError, IndexError):
            continue
    return table


def tree_rss(root_pid: int) -> int:
    table = process_table()
    selected = {root_pid}
    changed = True
    while changed:
        changed = False
        for pid, (parent, _rss) in table.items():
            if parent in selected and pid not in selected:
                selected.add(pid)
                changed = True
    return sum(table.get(pid, (0, 0))[1] for pid in selected)


def directory_bytes(path: pathlib.Path | None) -> int:
    if path is None or not path.exists():
        return 0
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += (pathlib.Path(root) / name).stat().st_size
            except FileNotFoundError:
                pass
    return total


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", type=pathlib.Path, required=True)
    parser.add_argument("--stdout", type=pathlib.Path)
    parser.add_argument("--stderr", type=pathlib.Path)
    parser.add_argument("--temp-dir", type=pathlib.Path)
    parser.add_argument("--interval", type=float, default=0.25)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a command is required after --")
    if args.temp_dir:
        args.temp_dir.mkdir(parents=True, exist_ok=True)
    for path in [args.json, args.stdout, args.stderr]:
        if path:
            path.parent.mkdir(parents=True, exist_ok=True)
    stdout_file = args.stdout.open("wb") if args.stdout else None
    stderr_file = args.stderr.open("wb") if args.stderr else None
    baseline_temp = directory_bytes(args.temp_dir)
    started_at = utc_now()
    started = time.monotonic()
    process = subprocess.Popen(command, stdout=stdout_file, stderr=stderr_file)
    peak_rss = 0
    peak_temp = baseline_temp
    samples = 0
    interrupted = False
    try:
        while process.poll() is None:
            peak_rss = max(peak_rss, tree_rss(process.pid))
            peak_temp = max(peak_temp, directory_bytes(args.temp_dir))
            samples += 1
            time.sleep(args.interval)
        peak_rss = max(peak_rss, tree_rss(process.pid))
        peak_temp = max(peak_temp, directory_bytes(args.temp_dir))
    except KeyboardInterrupt:
        interrupted = True
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
    finally:
        if stdout_file:
            stdout_file.close()
        if stderr_file:
            stderr_file.close()
    result = {
        "schemaVersion": 1,
        "command": command,
        "startedAt": started_at,
        "finishedAt": utc_now(),
        "wallSeconds": time.monotonic() - started,
        "peakProcessTreeRssBytes": peak_rss,
        "peakTemporaryBytes": peak_temp,
        "baselineTemporaryBytes": baseline_temp,
        "peakTemporaryDeltaBytes": max(0, peak_temp - baseline_temp),
        "sampleIntervalSeconds": args.interval,
        "sampleCount": samples,
        "exitStatus": 130 if interrupted else process.returncode,
        "interrupted": interrupted,
    }
    args.json.write_text(json.dumps(result, indent=2) + "\n")
    return 130 if interrupted else process.returncode


if __name__ == "__main__":
    raise SystemExit(main())
