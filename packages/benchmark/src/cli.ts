#!/usr/bin/env node
import { resolve } from "node:path";
import { runArchiveBenchmark } from "./archive-benchmark.js";
import { runBrowserBenchmark } from "./browser-benchmark.js";
import { compareRuns } from "./compare.js";
import { generateWorkload } from "./generate-workload.js";
import { checkOrigin, formatOriginCheck } from "./origin-check.js";
import type { CacheMode, DecoderName } from "./types.js";

class Arguments {
  readonly #values: string[];
  readonly #used = new Set<number>();

  constructor(values: readonly string[]) {
    this.#values = [...values];
  }

  value(flag: string): string | undefined {
    const index = this.#values.indexOf(flag);
    if (index < 0) return undefined;
    const value = this.#values[index + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new TypeError(`${flag} requires a value`);
    }
    this.#used.add(index);
    this.#used.add(index + 1);
    return value;
  }

  values(flag: string): string[] {
    const index = this.#values.indexOf(flag);
    if (index < 0) return [];
    this.#used.add(index);
    const values: string[] = [];
    for (let cursor = index + 1; cursor < this.#values.length; cursor += 1) {
      const value = this.#values[cursor] as string;
      if (value.startsWith("--")) break;
      this.#used.add(cursor);
      values.push(value);
    }
    return values;
  }

  boolean(flag: string): boolean {
    const index = this.#values.indexOf(flag);
    if (index < 0) return false;
    this.#used.add(index);
    return true;
  }

  assertComplete(): void {
    const unused = this.#values.filter(
      (_value, index) => !this.#used.has(index),
    );
    if (unused.length > 0) {
      throw new TypeError(`unexpected arguments: ${unused.join(" ")}`);
    }
  }
}

function required(value: string | undefined, flag: string): string {
  if (value === undefined) throw new TypeError(`${flag} is required`);
  return value;
}

function integer(
  value: string | undefined,
  fallback: number,
  flag: string,
): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new TypeError(`${flag} must be a non-negative safe integer`);
  }
  return parsed;
}

function decoders(value: string | undefined): DecoderName[] {
  if (value === undefined || value === "both") return ["pure-js", "wasm"];
  if (value === "pure-js" || value === "wasm") return [value];
  throw new TypeError("--decoder must be pure-js, wasm, or both");
}

function modes(value: string | undefined): CacheMode[] {
  if (value === undefined || value === "both") return ["cold", "warm"];
  if (value === "cold" || value === "warm") return [value];
  throw new TypeError("--mode must be cold, warm, or both");
}

function cache(value: string | undefined): RequestCache | undefined {
  if (value === undefined || value === "default") return undefined;
  if (
    value === "no-store" ||
    value === "reload" ||
    value === "no-cache" ||
    value === "force-cache"
  ) {
    return value;
  }
  throw new TypeError("--http-cache has an unsupported RequestCache value");
}

function help(): string {
  return `pangenome-range benchmark harness

pnpm bench -- archive (--url URL | --file PATH) --workload PATH --run-id ID [options]
pnpm bench -- browser --file PATH --workload PATH --run-id ID [options]
pnpm bench -- origin-check --url URL [--origin PAGES_ORIGIN] [--file PATH] [--sha256 HEX] [--report PATH]
pnpm bench -- compare --runs RUN_A RUN_B [...] [--rust-summary PATH | --rust-queries PATH] [--output PATH]
pnpm bench -- workload --file PATH --output PATH [--random-per-size N] [--seed N] [--no-absent]

Common benchmark options:
  --results-dir PATH              default: results
  --decoder pure-js|wasm|both    default: both
  --directory-cache-bytes N      default: 1048576
  --payload-cache-bytes N        default: 33554432

Archive options:
  --mode cold|warm|both           default: both
  --http-cache MODE              default|no-store|reload|no-cache|force-cache

Browser options:
  --browsers LIST                comma-separated; default: chromium,firefox,webkit
`;
}

const invocationDirectory = process.env.INIT_CWD ?? process.cwd();

function inputPath(value: string): string {
  return resolve(invocationDirectory, value);
}

async function main(): Promise<void> {
  const input = process.argv.slice(2);
  if (input[0] === "--") input.shift();
  const [command, ...values] = input;
  if (command === undefined || command === "help" || command === "--help") {
    console.log(help());
    return;
  }
  const args = new Arguments(values);
  if (command === "archive") {
    const url = args.value("--url");
    const file = args.value("--file");
    const workloadPath = required(args.value("--workload"), "--workload");
    const runId = required(args.value("--run-id"), "--run-id");
    const resultsDirectory = inputPath(
      args.value("--results-dir") ?? "results",
    );
    const selectedModes = modes(args.value("--mode"));
    const selectedDecoders = decoders(args.value("--decoder"));
    const directoryCacheBytes = integer(
      args.value("--directory-cache-bytes"),
      1_048_576,
      "--directory-cache-bytes",
    );
    const payloadCacheBytes = integer(
      args.value("--payload-cache-bytes"),
      33_554_432,
      "--payload-cache-bytes",
    );
    const httpCache = cache(args.value("--http-cache"));
    args.assertComplete();
    const result = await runArchiveBenchmark({
      ...(url === undefined ? {} : { url }),
      ...(file === undefined ? {} : { file: inputPath(file) }),
      workloadPath: inputPath(workloadPath),
      runId,
      resultsDirectory,
      modes: selectedModes,
      decoders: selectedDecoders,
      directoryCacheBytes,
      payloadCacheBytes,
      ...(httpCache === undefined ? {} : { httpCache }),
    });
    console.log(`results: ${result.directory}`);
    return;
  }
  if (command === "browser") {
    const archivePath = inputPath(required(args.value("--file"), "--file"));
    const workloadPath = inputPath(
      required(args.value("--workload"), "--workload"),
    );
    const runId = required(args.value("--run-id"), "--run-id");
    const resultsDirectory = inputPath(
      args.value("--results-dir") ?? "results",
    );
    const selectedBrowsers = (
      args.value("--browsers") ?? "chromium,firefox,webkit"
    )
      .split(",")
      .filter(Boolean);
    const selectedDecoders = decoders(args.value("--decoder"));
    const directoryCacheBytes = integer(
      args.value("--directory-cache-bytes"),
      1_048_576,
      "--directory-cache-bytes",
    );
    const payloadCacheBytes = integer(
      args.value("--payload-cache-bytes"),
      33_554_432,
      "--payload-cache-bytes",
    );
    args.assertComplete();
    const result = await runBrowserBenchmark({
      archivePath,
      workloadPath,
      runId,
      resultsDirectory,
      browsers: selectedBrowsers,
      decoders: selectedDecoders,
      directoryCacheBytes,
      payloadCacheBytes,
    });
    console.log(`results: ${result.directory}`);
    return;
  }
  if (command === "origin-check") {
    const url = args.value("--url") ?? process.env.PANGENOME_RANGE_ARCHIVE_URL;
    const requestOrigin =
      args.value("--origin") ?? process.env.PANGENOME_RANGE_CORS_ORIGIN;
    const localFile = args.value("--file");
    const expectedSha256 = args.value("--sha256");
    const reportPath = args.value("--report");
    args.assertComplete();
    const result = await checkOrigin({
      url: required(url, "--url or PANGENOME_RANGE_ARCHIVE_URL"),
      ...(requestOrigin === undefined ? {} : { requestOrigin }),
      ...(localFile === undefined ? {} : { localFile: inputPath(localFile) }),
      ...(expectedSha256 === undefined ? {} : { expectedSha256 }),
      ...(reportPath === undefined
        ? {}
        : { reportPath: inputPath(reportPath) }),
    });
    console.log(formatOriginCheck(result));
    if (!result.passed) process.exitCode = 1;
    return;
  }
  if (command === "compare") {
    const runs = args.values("--runs");
    const resultsDirectory = inputPath(
      args.value("--results-dir") ?? "results",
    );
    const rustQueriesPath = args.value("--rust-queries");
    const rustSummaryPath = args.value("--rust-summary");
    const outputPath = args.value("--output");
    args.assertComplete();
    console.log(
      await compareRuns({
        runs,
        resultsDirectory,
        ...(rustQueriesPath === undefined
          ? {}
          : { rustQueriesPath: inputPath(rustQueriesPath) }),
        ...(rustSummaryPath === undefined
          ? {}
          : { rustSummaryPath: inputPath(rustSummaryPath) }),
        ...(outputPath === undefined
          ? {}
          : { outputPath: inputPath(outputPath) }),
      }),
    );
    return;
  }
  if (command === "workload") {
    const archivePath = inputPath(required(args.value("--file"), "--file"));
    const outputPath = inputPath(required(args.value("--output"), "--output"));
    const randomPerSize = integer(
      args.value("--random-per-size"),
      2,
      "--random-per-size",
    );
    const seedText = args.value("--seed") ?? "0x504e47524e473031";
    const seed = BigInt(seedText);
    const includeAbsent = !args.boolean("--no-absent");
    args.assertComplete();
    const workload = await generateWorkload({
      archivePath,
      outputPath,
      randomPerSize,
      seed,
      includeAbsent,
    });
    console.log(`workload: ${outputPath} (${workload.queries.length} queries)`);
    return;
  }
  throw new TypeError(`unknown benchmark command ${command}\n\n${help()}`);
}

main().catch((error: unknown) => {
  console.error(
    error instanceof Error ? `${error.name}: ${error.message}` : error,
  );
  process.exitCode = 1;
});
