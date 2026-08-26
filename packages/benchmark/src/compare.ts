import { open, readFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { distribution } from "./metrics.js";
import type { BenchmarkSummary, QueryMeasurement } from "./types.js";

function parseSummary(value: unknown, path: string): BenchmarkSummary {
  if (typeof value !== "object" || value === null) {
    throw new TypeError(`${path} is not a benchmark summary`);
  }
  const summary = value as Partial<BenchmarkSummary>;
  if (
    summary.schemaVersion !== 1 ||
    (summary.kind !== "node" && summary.kind !== "browser") ||
    !Array.isArray(summary.measurements)
  ) {
    throw new TypeError(`${path} has an unsupported benchmark summary schema`);
  }
  return summary as BenchmarkSummary;
}

function queryAggregate(measurements: readonly QueryMeasurement[]): {
  requests: ReturnType<typeof distribution>;
  bytes: ReturnType<typeof distribution>;
  rounds: ReturnType<typeof distribution>;
  actualRounds: ReturnType<typeof distribution>;
  decodeMs: ReturnType<typeof distribution>;
  totalMs: ReturnType<typeof distribution>;
} {
  return {
    requests: distribution(measurements.map((item) => item.actualRequests)),
    bytes: distribution(measurements.map((item) => item.actualBytes)),
    rounds: distribution(measurements.map((item) => item.dependencyRounds)),
    actualRounds: distribution(
      measurements
        .map((item) => item.actualRequestRounds)
        .filter((value): value is number => value !== undefined),
    ),
    decodeMs: distribution(measurements.map((item) => item.decodeMs)),
    totalMs: distribution(measurements.map((item) => item.totalMs)),
  };
}

function display(value: number | null): string {
  return value === null ? "n/a" : value.toFixed(3);
}

function parseCsvLine(line: string): string[] {
  const values: string[] = [];
  let value = "";
  let quoted = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (character === '"') {
      if (quoted && line[index + 1] === '"') {
        value += '"';
        index += 1;
      } else {
        quoted = !quoted;
      }
    } else if (character === "," && !quoted) {
      values.push(value);
      value = "";
    } else {
      value += character;
    }
  }
  values.push(value);
  return values;
}

async function rustRows(path: string): Promise<
  Array<{
    requests: number;
    bytes: number;
    rounds: number;
    decodeMs: number;
    simulated20Ms: number;
  }>
> {
  const lines = (await readFile(path, "utf8")).trim().split(/\r?\n/);
  const headers = parseCsvLine(lines.shift() ?? "");
  const index = (name: string): number => {
    const value = headers.indexOf(name);
    if (value < 0) throw new TypeError(`${path} is missing ${name}`);
    return value;
  };
  const storage = index("storage_kind");
  const gap = index("coalescing_gap");
  const requests = index("physical_reads");
  const bytes = index("total_bytes_fetched");
  const rounds = index("dependency_rounds");
  const decode = index("decode_us");
  const simulated = index("simulated_20ms_ms");
  return lines
    .map(parseCsvLine)
    .filter(
      (row) =>
        row[storage] === "fixed-window" &&
        (row[gap] === "65536" || row[gap] === ""),
    )
    .map((row) => ({
      requests: Number(row[requests]),
      bytes: Number(row[bytes]),
      rounds: Number(row[rounds]),
      decodeMs: Number(row[decode]) / 1_000,
      simulated20Ms: Number(row[simulated]),
    }));
}

async function rustSummaryRows(path: string): Promise<
  Array<{
    requests: number;
    bytes: number;
    rounds: number;
    decodeMs: number;
    simulated20Ms: number;
  }>
> {
  const parsed = JSON.parse(await readFile(path, "utf8")) as {
    measurements?: Array<Record<string, unknown>>;
  };
  if (!Array.isArray(parsed.measurements)) {
    throw new TypeError(`${path} has no Rust verification measurements`);
  }
  return parsed.measurements.map((measurement) => ({
    requests: Number(measurement.physical_reads),
    bytes: Number(measurement.total_bytes_fetched),
    rounds: Number(measurement.dependency_rounds),
    decodeMs: Number(measurement.decode_us) / 1_000,
    simulated20Ms: Number(measurement.simulated_20ms_ms),
  }));
}

export async function compareRuns(options: {
  readonly runs: readonly string[];
  readonly resultsDirectory: string;
  readonly rustQueriesPath?: string;
  readonly rustSummaryPath?: string;
  readonly outputPath?: string;
}): Promise<string> {
  if (options.runs.length < 2) {
    throw new TypeError("compare requires at least two run IDs");
  }
  if (
    options.rustQueriesPath !== undefined &&
    options.rustSummaryPath !== undefined
  ) {
    throw new TypeError(
      "compare accepts either --rust-queries or --rust-summary, not both",
    );
  }
  const summaries = await Promise.all(
    options.runs.map(async (run) => {
      const path = join(resolve(options.resultsDirectory), run, "summary.json");
      return parseSummary(
        JSON.parse(await readFile(path, "utf8")) as unknown,
        path,
      );
    }),
  );
  const rows = summaries.map((summary) => {
    const aggregate = queryAggregate(summary.measurements);
    return `| ${summary.runId} | ${summary.kind} | ${
      summary.measurements
        .map((item) => item.browser)
        .filter(Boolean)
        .filter((value, index, all) => all.indexOf(value) === index)
        .join(", ") || "Node"
    } | ${summary.measurements
      .map((item) => item.decoder)
      .filter((value, index, all) => all.indexOf(value) === index)
      .join(
        ", ",
      )} | ${display(aggregate.rounds.p50)} | ${display(aggregate.actualRounds.p50)} | ${display(aggregate.requests.p50)} | ${display(aggregate.bytes.p50)} | ${display(aggregate.decodeMs.p50)} | ${display(aggregate.totalMs.p50)} |`;
  });
  let rustSection = `## Rust simulated layout evidence

No Rust query table was supplied. Use \`--rust-queries results/<rust-run>/queries.csv\` to add simulated layout rows; omission is explicit and is not treated as zero.
`;
  const rustPath = options.rustSummaryPath ?? options.rustQueriesPath;
  if (rustPath !== undefined) {
    const rust =
      options.rustSummaryPath === undefined
        ? await rustRows(rustPath)
        : await rustSummaryRows(rustPath);
    const aggregate = {
      rounds: distribution(rust.map((item) => item.rounds)),
      requests: distribution(rust.map((item) => item.requests)),
      bytes: distribution(rust.map((item) => item.bytes)),
      decode: distribution(rust.map((item) => item.decodeMs)),
      simulated: distribution(rust.map((item) => item.simulated20Ms)),
    };
    rustSection = `## Rust simulated layout evidence

Source: \`${rustPath}\`. These are Rust local decode measurements plus the explicit 20 ms simulated network model. They are not Node or browser observations.

| rows | planned rounds p50 | planned ranges p50 | planned bytes p50 | local Rust decode p50 ms | simulated 20 ms profile p50 ms |
|---:|---:|---:|---:|---:|---:|
| ${rust.length} | ${display(aggregate.rounds.p50)} | ${display(aggregate.requests.p50)} | ${display(aggregate.bytes.p50)} | ${display(aggregate.decode.p50)} | ${display(aggregate.simulated.p50)} |
`;
  }
  const report = `# Range benchmark comparison

The real-runtime table uses origin/source observations for actual request counts, bytes, and request rounds; reader traces provide planned dependency rounds, and runtime phase clocks provide decode/total latency. It does not combine those values with Rust's optimistic network model.

| run | runtime | engines | decoders | planned rounds p50 | observed rounds p50 | actual ranges p50 | actual bytes p50 | decode p50 ms | end-to-end p50 ms |
|---|---|---|---|---:|---:|---:|---:|---:|---:|
${rows.join("\n")}

${rustSection}
## Cold/warm and per-browser detail

${summaries
  .map((summary) => {
    const groups = new Map<string, QueryMeasurement[]>();
    for (const item of summary.measurements) {
      const key = `${item.browser ?? "Node"} / ${item.decoder} / ${item.cacheMode}`;
      const group = groups.get(key) ?? [];
      group.push(item);
      groups.set(key, group);
    }
    return `### ${summary.runId}\n\n${[...groups]
      .map(([key, values]) => {
        const aggregate = queryAggregate(values);
        return `- ${key}: ${values.length} queries; latency p50/p95 ${display(aggregate.totalMs.p50)}/${display(aggregate.totalMs.p95)} ms; actual bytes p50 ${display(aggregate.bytes.p50)}.`;
      })
      .join("\n")}`;
  })
  .join("\n\n")}
`;
  if (options.outputPath !== undefined) {
    const handle = await open(options.outputPath, "wx");
    try {
      await handle.writeFile(report);
    } finally {
      await handle.close();
    }
  }
  return report;
}

export function comparisonName(runs: readonly string[]): string {
  return runs.map((run) => basename(run)).join("-vs-");
}
