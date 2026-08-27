#!/usr/bin/env python3
"""Bounded raw, mmap, and independent zstd-block source-cache experiment."""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import mmap
import os
import random
import resource
import statistics
import tempfile
import time
from pathlib import Path


BLOCK_BYTES = 256 * 1024
READ_BYTES = 4 * 1024
SAMPLE_BLOCKS = 2048
RANDOM_READS = 4096
SEED = 20260826


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * fraction))]


def timing_summary(latencies_us: list[float], wall_seconds: float) -> dict[str, float]:
    return {
        "wallMs": wall_seconds * 1000.0,
        "meanUs": statistics.fmean(latencies_us),
        "p50Us": percentile(latencies_us, 0.50),
        "p95Us": percentile(latencies_us, 0.95),
        "readsPerSecond": len(latencies_us) / wall_seconds,
    }


class Zstd:
    def __init__(self) -> None:
        self.lib = ctypes.CDLL("libzstd.so")
        self.lib.ZSTD_compressBound.argtypes = [ctypes.c_size_t]
        self.lib.ZSTD_compressBound.restype = ctypes.c_size_t
        self.lib.ZSTD_compress.argtypes = [
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.c_int,
        ]
        self.lib.ZSTD_compress.restype = ctypes.c_size_t
        self.lib.ZSTD_decompress.argtypes = [
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.c_void_p,
            ctypes.c_size_t,
        ]
        self.lib.ZSTD_decompress.restype = ctypes.c_size_t
        self.lib.ZSTD_isError.argtypes = [ctypes.c_size_t]
        self.lib.ZSTD_isError.restype = ctypes.c_uint

    def compress(self, source: bytes) -> bytes:
        destination = ctypes.create_string_buffer(self.lib.ZSTD_compressBound(len(source)))
        encoded = self.lib.ZSTD_compress(
            destination, len(destination), source, len(source), 3
        )
        if self.lib.ZSTD_isError(encoded):
            raise RuntimeError("libzstd compression failed")
        return destination.raw[:encoded]

    def decompress(self, source: bytes, decoded_bytes: int) -> bytes:
        destination = ctypes.create_string_buffer(decoded_bytes)
        decoded = self.lib.ZSTD_decompress(
            destination, decoded_bytes, source, len(source)
        )
        if self.lib.ZSTD_isError(decoded) or decoded != decoded_bytes:
            raise RuntimeError("libzstd decompression failed")
        return destination.raw[:decoded]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("cache", type=Path)
    parser.add_argument("--json", required=True, type=Path)
    parser.add_argument("--work-dir", type=Path)
    args = parser.parse_args()

    paths = [args.cache / "records.data", args.cache / "sequences.data"]
    sizes = [path.stat().st_size for path in paths]
    block_locations = [
        (file_id, offset, min(BLOCK_BYTES, size - offset))
        for file_id, size in enumerate(sizes)
        for offset in range(0, size, BLOCK_BYTES)
    ]
    rng = random.Random(SEED)
    sampled = sorted(rng.sample(block_locations, SAMPLE_BLOCKS))
    descriptors: list[dict[str, int]] = []
    zstd = Zstd()
    work_dir = args.work_dir or args.json.parent
    work_dir.mkdir(parents=True, exist_ok=True)
    output = tempfile.NamedTemporaryFile(
        prefix="source-cache-block-sample-", suffix=".zst", dir=work_dir, delete=False
    )
    output_path = Path(output.name)
    handles = [os.open(path, os.O_RDONLY) for path in paths]
    construction_started = time.perf_counter()
    decoded_sample_bytes = 0
    try:
        for file_id, offset, length in sampled:
            raw = os.pread(handles[file_id], length, offset)
            if len(raw) != length:
                raise RuntimeError("short raw cache read")
            encoded = zstd.compress(raw)
            encoded_offset = output.tell()
            output.write(encoded)
            descriptors.append(
                {
                    "fileId": file_id,
                    "rawOffset": offset,
                    "rawBytes": length,
                    "encodedOffset": encoded_offset,
                    "encodedBytes": len(encoded),
                }
            )
            decoded_sample_bytes += length
        output.flush()
        os.fsync(output.fileno())
        construction_seconds = time.perf_counter() - construction_started
        encoded_sample_bytes = output.tell()
        output.close()

        queries = []
        for _ in range(RANDOM_READS):
            descriptor_id = rng.randrange(len(descriptors))
            descriptor = descriptors[descriptor_id]
            inner = rng.randrange(0, descriptor["rawBytes"] - READ_BYTES + 1)
            queries.append((descriptor_id, inner))

        def run_raw_pread() -> tuple[dict[str, float], str]:
            digest = hashlib.sha256()
            latencies = []
            started = time.perf_counter()
            for descriptor_id, inner in queries:
                descriptor = descriptors[descriptor_id]
                one = time.perf_counter()
                data = os.pread(
                    handles[descriptor["fileId"]],
                    READ_BYTES,
                    descriptor["rawOffset"] + inner,
                )
                latencies.append((time.perf_counter() - one) * 1_000_000.0)
                digest.update(data)
            return timing_summary(latencies, time.perf_counter() - started), digest.hexdigest()

        maps = [mmap.mmap(handle, 0, access=mmap.ACCESS_READ) for handle in handles]

        def run_raw_mmap() -> tuple[dict[str, float], str]:
            digest = hashlib.sha256()
            latencies = []
            started = time.perf_counter()
            for descriptor_id, inner in queries:
                descriptor = descriptors[descriptor_id]
                begin = descriptor["rawOffset"] + inner
                one = time.perf_counter()
                data = maps[descriptor["fileId"]][begin : begin + READ_BYTES]
                latencies.append((time.perf_counter() - one) * 1_000_000.0)
                digest.update(data)
            return timing_summary(latencies, time.perf_counter() - started), digest.hexdigest()

        compressed_handle = os.open(output_path, os.O_RDONLY)

        def run_compressed() -> tuple[dict[str, float], str]:
            digest = hashlib.sha256()
            latencies = []
            started = time.perf_counter()
            for descriptor_id, inner in queries:
                descriptor = descriptors[descriptor_id]
                one = time.perf_counter()
                encoded = os.pread(
                    compressed_handle,
                    descriptor["encodedBytes"],
                    descriptor["encodedOffset"],
                )
                decoded = zstd.decompress(encoded, descriptor["rawBytes"])
                data = decoded[inner : inner + READ_BYTES]
                latencies.append((time.perf_counter() - one) * 1_000_000.0)
                digest.update(data)
            return timing_summary(latencies, time.perf_counter() - started), digest.hexdigest()

        pread_metrics, pread_digest = run_raw_pread()
        mmap_metrics, mmap_digest = run_raw_mmap()
        compressed_metrics, compressed_digest = run_compressed()
        if len({pread_digest, mmap_digest, compressed_digest}) != 1:
            raise RuntimeError("cache layout reads returned different bytes")

        report = {
            "schemaVersion": 1,
            "boundedExperiment": True,
            "seed": SEED,
            "blockBytes": BLOCK_BYTES,
            "readBytes": READ_BYTES,
            "sampleBlocks": SAMPLE_BLOCKS,
            "randomReads": RANDOM_READS,
            "sourceFiles": [
                {"name": path.name, "bytes": size}
                for path, size in zip(paths, sizes, strict=True)
            ],
            "rawSampleBytes": decoded_sample_bytes,
            "compressedSampleBytes": encoded_sample_bytes,
            "compressedRatio": encoded_sample_bytes / decoded_sample_bytes,
            "compressionConstructionWallMs": construction_seconds * 1000.0,
            "rawPread": pread_metrics,
            "rawMmap": mmap_metrics,
            "zstdIndependentBlocks": compressed_metrics,
            "readDigestSha256": pread_digest,
            "processPeakRssKiB": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
            "decision": "experiment-only; do not adopt without a full-cache/full-encode Pareto win",
        }
        args.json.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))
    finally:
        if not output.closed:
            output.close()
        for handle in handles:
            os.close(handle)
        if "maps" in locals():
            for mapped in maps:
                mapped.close()
        if "compressed_handle" in locals():
            os.close(compressed_handle)
        output_path.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
