import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import type { BenchmarkQuery, BenchmarkWorkload } from "./types.js";
import { WORKLOAD_SCHEMA_VERSION } from "./types.js";

const SHA256 = /^[0-9a-f]{64}$/;

function record(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new TypeError(`${label} must be a JSON object`);
  }
  return value as Record<string, unknown>;
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${label} must be a non-empty string`);
  }
  return value;
}

function integer(value: unknown, label: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    throw new TypeError(`${label} must be a non-negative safe integer`);
  }
  return value as number;
}

function parseQuery(value: unknown, index: number): BenchmarkQuery {
  const input = record(value, `queries[${index}]`);
  const start = integer(input.start, `queries[${index}].start`);
  const end = integer(input.end, `queries[${index}].end`);
  if (end <= start) {
    throw new RangeError(`queries[${index}].end must be greater than start`);
  }
  const expectedCanonicalHash =
    input.expectedCanonicalHash === undefined
      ? undefined
      : string(
          input.expectedCanonicalHash,
          `queries[${index}].expectedCanonicalHash`,
        );
  if (
    expectedCanonicalHash !== undefined &&
    !SHA256.test(expectedCanonicalHash)
  ) {
    throw new TypeError(
      `queries[${index}].expectedCanonicalHash must be 64 lowercase hex characters`,
    );
  }
  const expectedError =
    input.expectedError === undefined
      ? undefined
      : string(input.expectedError, `queries[${index}].expectedError`);
  if ((expectedCanonicalHash === undefined) === (expectedError === undefined)) {
    throw new TypeError(
      `queries[${index}] must declare exactly one of expectedCanonicalHash or expectedError`,
    );
  }
  return {
    id: string(input.id, `queries[${index}].id`),
    class: string(input.class, `queries[${index}].class`),
    sample: string(input.sample, `queries[${index}].sample`),
    contig: string(input.contig, `queries[${index}].contig`),
    start,
    end,
    context: integer(input.context, `queries[${index}].context`),
    ...(expectedCanonicalHash === undefined ? {} : { expectedCanonicalHash }),
    ...(expectedError === undefined ? {} : { expectedError }),
  };
}

export function parseWorkload(value: unknown): BenchmarkWorkload {
  const input = record(value, "workload");
  if (input.schemaVersion !== WORKLOAD_SCHEMA_VERSION) {
    throw new TypeError(
      `unsupported workload schemaVersion ${String(input.schemaVersion)}`,
    );
  }
  const archiveSha256 = string(input.archiveSha256, "archiveSha256");
  if (!SHA256.test(archiveSha256)) {
    throw new TypeError("archiveSha256 must be 64 lowercase hex characters");
  }
  if (!Array.isArray(input.queries) || input.queries.length === 0) {
    throw new TypeError("queries must be a non-empty array");
  }
  const queries = input.queries.map(parseQuery);
  const ids = new Set<string>();
  for (const query of queries) {
    if (ids.has(query.id))
      throw new TypeError(`duplicate query id ${query.id}`);
    ids.add(query.id);
  }
  const seed =
    input.seed === undefined ? undefined : string(input.seed, "workload.seed");
  return {
    schemaVersion: WORKLOAD_SCHEMA_VERSION,
    archiveSha256,
    ...(seed === undefined ? {} : { seed }),
    queries,
  };
}

export async function loadWorkload(path: string): Promise<{
  workload: BenchmarkWorkload;
  sha256: string;
}> {
  const bytes = await readFile(path);
  return {
    workload: parseWorkload(JSON.parse(bytes.toString("utf8")) as unknown),
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

export function matchesExpectedError(
  expected: string | undefined,
  actual: string | undefined,
): boolean {
  if (expected === undefined || actual === undefined) return false;
  if (expected === "reference-not-found") {
    return actual.includes("archive has no reference interval");
  }
  return actual.includes(expected);
}
