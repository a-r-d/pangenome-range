import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { cpus, freemem, hostname, platform, release, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { promisify } from "node:util";
import { distribution, requestBytes } from "./metrics.js";
import type {
  ArchiveIdentity,
  BenchmarkSummary,
  DecoderSummary,
  QueryMeasurement,
  SerializableRequest,
} from "./types.js";
import { RESULT_SCHEMA_VERSION } from "./types.js";

const execFileAsync = promisify(execFile);

export interface RunFiles {
  readonly directory: string;
  readonly config: string;
  readonly environment: string;
  readonly requests: string;
  readonly queries: string;
  readonly summary: string;
  readonly report: string;
}

function validateRunId(runId: string): void {
  if (
    runId.length === 0 ||
    !/^[A-Za-z0-9._-]+$/.test(runId) ||
    runId === "." ||
    runId === ".."
  ) {
    throw new TypeError(
      "run ID may contain only ASCII letters, digits, '-', '_', and '.'",
    );
  }
}

export async function createRunFiles(
  resultsDirectory: string,
  runId: string,
): Promise<RunFiles> {
  validateRunId(runId);
  const parent = resolve(resultsDirectory);
  await mkdir(parent, { recursive: true });
  const directory = join(parent, runId);
  await mkdir(directory);
  return {
    directory,
    config: join(directory, "config.json"),
    environment: join(directory, "environment.json"),
    requests: join(directory, "requests.ndjson"),
    queries: join(directory, "queries.csv"),
    summary: join(directory, "summary.json"),
    report: join(directory, "REPORT.md"),
  };
}

async function commandOutput(
  command: string,
  args: readonly string[],
): Promise<string> {
  try {
    const { stdout } = await execFileAsync(command, args, {
      encoding: "utf8",
    });
    return stdout.trim();
  } catch {
    return "unavailable";
  }
}

export async function collectEnvironment(
  browsers: Readonly<Record<string, string>> = {},
): Promise<Record<string, unknown>> {
  return {
    schemaVersion: RESULT_SCHEMA_VERSION,
    capturedAt: new Date().toISOString(),
    gitSha: await commandOutput("git", ["rev-parse", "HEAD"]),
    gitStatus: await commandOutput("git", ["status", "--short"]),
    node: process.version,
    pnpm: await commandOutput("pnpm", ["--version"]),
    platform: `${platform()} ${release()}`,
    hostname: hostname(),
    cpu: cpus()[0]?.model ?? "unknown",
    logicalCpus: cpus().length,
    totalMemoryBytes: totalmem(),
    freeMemoryBytesAtStart: freemem(),
    packageVersions: {
      benchmark: "0.1.0",
      reader: "0.1.0",
      playwright: "1.62.1",
      wasmZstd: "0.0.27",
    },
    browsers,
  };
}

function csv(value: unknown): string {
  const text = value === undefined || value === null ? "" : String(value);
  return /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

function queriesCsv(measurements: readonly QueryMeasurement[]): string {
  const headers = [
    "query_id",
    "query_class",
    "sample",
    "contig",
    "start",
    "end",
    "context",
    "scenario",
    "cache_mode",
    "http_cache_policy",
    "decoder",
    "browser",
    "total_ms",
    "open_ms",
    "query_ms",
    "actual_requests",
    "actual_bytes",
    "actual_unique_bytes",
    "actual_duplicate_bytes",
    "actual_request_rounds",
    "reader_observed_requests",
    "reader_observed_bytes",
    "source_origin_reconciled",
    "planned_requests",
    "planned_bytes",
    "dependency_rounds",
    "bootstrap_cache_hits",
    "directory_cache_hits",
    "payload_cache_hits",
    "decompression_ms",
    "decompression_p50_ms",
    "decompression_p95_ms",
    "decode_ms",
    "merge_ms",
    "selected_chunks",
    "selected_nodes",
    "selected_traversals",
    "canonical_hash",
    "expected_canonical_hash",
    "correctness",
    "error",
  ];
  const rows = measurements.map((measurement) =>
    [
      measurement.queryId,
      measurement.queryClass,
      measurement.sample,
      measurement.contig,
      measurement.start,
      measurement.end,
      measurement.context,
      measurement.scenario,
      measurement.cacheMode,
      measurement.httpCachePolicy,
      measurement.decoder,
      measurement.browser,
      measurement.totalMs,
      measurement.openMs,
      measurement.queryMs,
      measurement.actualRequests,
      measurement.actualBytes,
      measurement.actualUniqueBytes,
      measurement.actualDuplicateBytes,
      measurement.actualRequestRounds,
      measurement.readerObservedRequests,
      measurement.readerObservedBytes,
      measurement.sourceOriginReconciled,
      measurement.plannedRequests,
      measurement.plannedBytes,
      measurement.dependencyRounds,
      measurement.cacheHits.bootstrap,
      measurement.cacheHits.directory,
      measurement.cacheHits.payload,
      measurement.decompression.totalMs,
      measurement.decompression.p50Ms,
      measurement.decompression.p95Ms,
      measurement.decodeMs,
      measurement.mergeMs,
      measurement.selectedChunks,
      measurement.selectedNodes,
      measurement.selectedTraversals,
      measurement.canonicalHash,
      measurement.expectedCanonicalHash,
      measurement.correctness,
      measurement.error,
    ]
      .map(csv)
      .join(","),
  );
  return `${headers.join(",")}\n${rows.join("\n")}\n`;
}

export function buildSummary(options: {
  readonly runId: string;
  readonly kind: "node" | "browser";
  readonly archive: ArchiveIdentity;
  readonly workloadSha256: string;
  readonly measurements: readonly QueryMeasurement[];
  readonly requests: readonly SerializableRequest[];
  readonly decoderSummaries: readonly DecoderSummary[];
  readonly limitations: readonly string[];
}): BenchmarkSummary {
  const bytes = requestBytes(options.requests);
  const passed = options.measurements.filter(
    (measurement) => measurement.correctness,
  ).length;
  return {
    schemaVersion: RESULT_SCHEMA_VERSION,
    runId: options.runId,
    kind: options.kind,
    archive: options.archive,
    workloadSha256: options.workloadSha256,
    measurements: options.measurements,
    decoderSummaries: options.decoderSummaries,
    totals: {
      queries: options.measurements.length,
      passed,
      failed: options.measurements.length - passed,
      requests: options.requests.length,
      responseBytes: bytes.total,
      uniqueBytes: bytes.unique,
      duplicateBytes: bytes.duplicate,
    },
    latencyMs: distribution(
      options.measurements.map((measurement) => measurement.totalMs),
    ),
    limitations: options.limitations,
  };
}

function number(value: number | null): string {
  return value === null ? "n/a" : value.toFixed(3);
}

function markdownReport(summary: BenchmarkSummary): string {
  const rows = summary.measurements
    .map(
      (measurement) =>
        `| ${measurement.browser ?? "Node"} | ${measurement.decoder} | ${measurement.scenario} | ${measurement.queryId} | ${measurement.actualRequests} / ${measurement.plannedRequests} | ${measurement.actualBytes} / ${measurement.plannedBytes} | ${measurement.actualRequestRounds ?? "n/a"} / ${measurement.dependencyRounds} | ${measurement.sourceOriginReconciled === false ? "no" : "yes"} | ${measurement.totalMs.toFixed(3)} | ${measurement.correctness ? "yes" : "no"} |`,
    )
    .join("\n");
  const decoders = summary.decoderSummaries
    .map(
      (decoder) =>
        `| ${decoder.name} | ${decoder.initializationMs.toFixed(3)} | ${decoder.javascriptBytes} | ${decoder.wasmBytes} | ${number(decoder.decompression.p50Ms)} | ${number(decoder.decompression.p95Ms)} | ${decoder.peakHeapBytes ?? "unavailable"} |`,
    )
    .join("\n");
  return `# ${summary.kind === "browser" ? "Real-browser" : "Node"} range benchmark: ${summary.runId}

Status: **${summary.totals.failed === 0 ? "correctness gate passed" : "failed"}**.

This report contains real ${summary.archive.kind === "http" ? "HTTP" : "positioned-file"} measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: \`${summary.archive.location}\`
- Size: ${summary.archive.size} bytes
- Archive SHA-256: ${summary.archive.sha256 ?? "not downloaded; workload identity only"}
- ETag: ${summary.archive.etag ?? "not applicable/unavailable"}
- Workload SHA-256: \`${summary.workloadSha256}\`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
${rows}

Latency p50/p95/max: ${number(summary.latencyMs.p50)} / ${number(summary.latencyMs.p95)} / ${number(summary.latencyMs.max)} ms across ${summary.latencyMs.count} measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
${decoders}

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

${summary.limitations.map((limitation) => `- ${limitation}`).join("\n")}
`;
}

export async function writeRun(options: {
  readonly files: RunFiles;
  readonly config: Record<string, unknown>;
  readonly environment: Record<string, unknown>;
  readonly summary: BenchmarkSummary;
  readonly requests: readonly SerializableRequest[];
}): Promise<void> {
  await Promise.all([
    writeFile(
      options.files.config,
      `${JSON.stringify(options.config, null, 2)}\n`,
    ),
    writeFile(
      options.files.environment,
      `${JSON.stringify(options.environment, null, 2)}\n`,
    ),
    writeFile(
      options.files.requests,
      options.requests.map((request) => JSON.stringify(request)).join("\n") +
        (options.requests.length === 0 ? "" : "\n"),
    ),
    writeFile(options.files.queries, queriesCsv(options.summary.measurements)),
    writeFile(
      options.files.summary,
      `${JSON.stringify(options.summary, null, 2)}\n`,
    ),
    writeFile(options.files.report, markdownReport(options.summary)),
  ]);
}

export async function sha256File(path: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

export async function sha256Text(path: string): Promise<string> {
  return createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
}

export function workspaceFromModule(moduleUrl: string): string {
  return resolve(dirname(new URL(moduleUrl).pathname), "../../..");
}
