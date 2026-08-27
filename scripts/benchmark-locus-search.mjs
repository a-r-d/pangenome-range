#!/usr/bin/env node

import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { FileRangeSource } from "../packages/browser/dist/node/index.js";
import {
  openPangenome,
  TracingRangeSource,
} from "../packages/browser/dist/reader/index.js";

function parseArgs(argv) {
  const options = { archive: undefined, csv: undefined, json: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--csv" || argument === "--json") {
      const value = argv[index + 1];
      if (value === undefined) throw new Error(`${argument} requires a path`);
      options[argument.slice(2)] = resolve(value);
      index += 1;
    } else if (options.archive === undefined) {
      options.archive = resolve(argument);
    } else {
      throw new Error(`unexpected argument ${argument}`);
    }
  }
  if (options.archive === undefined || options.csv === undefined) {
    throw new Error(
      "usage: node scripts/benchmark-locus-search.mjs ARCHIVE --csv PATH [--json PATH]",
    );
  }
  return options;
}

const CASES = [
  { id: "exact-symbol", name: "BRCA1", mode: "exact" },
  {
    id: "exact-stable-id",
    name: "ENSG00000012048.28",
    mode: "exact",
  },
  { id: "alias-unavailable-in-corpus", name: "RNF53", mode: "exact" },
  { id: "short-prefix", name: "BRCA", mode: "prefix" },
  { id: "broad-prefix-limit", name: "E", mode: "prefix", limit: 1 },
  { id: "missing-symbol", name: "NOT_A_GENCODE_V50_GENE", mode: "exact" },
  {
    id: "filtered-sample",
    name: "BRCA1",
    mode: "exact",
    sample: "GRCh38",
  },
  {
    id: "filtered-contig",
    name: "BRCA1",
    mode: "exact",
    contig: "chr17",
  },
];

function csvCell(value) {
  const string = String(value ?? "");
  return /[",\n]/u.test(string) ? `"${string.replaceAll('"', '""')}"` : string;
}

function jsonValue(_key, value) {
  return typeof value === "bigint" ? value.toString() : value;
}

async function measure(archive, benchmarkCase, cacheState) {
  const started = performance.now();
  const result = await archive.searchLoci({ ...benchmarkCase, trace: true });
  const wallMs = performance.now() - started;
  const trace = result.trace;
  if (trace === undefined) throw new Error("search tracing was not returned");
  return {
    case: benchmarkCase.id,
    cacheState,
    query: benchmarkCase.name,
    mode: benchmarkCase.mode,
    sample: benchmarkCase.sample ?? "",
    contig: benchmarkCase.contig ?? "",
    limit: benchmarkCase.limit ?? 50,
    hits: result.hits.length,
    truncated: result.truncated,
    requests: trace.requestRanges.length,
    dependencyRounds: trace.dependencyRounds,
    bytes: trace.totalBytes,
    cacheHits: trace.cacheHits,
    pagesAvoidedByLimit: trace.pagesAvoidedByLimit,
    integrityMs: trace.integrityMs,
    decompressionMs: trace.decompressionMs,
    decodeMs: trace.decodeMs,
    wallMs,
    maxRssKiB: process.resourceUsage().maxRSS,
    firstMatchedName: result.hits[0]?.matchedName ?? "",
    firstStableId: result.hits[0]?.stableId ?? "",
  };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const rows = [];
  for (const benchmarkCase of CASES) {
    const file = await FileRangeSource.open(options.archive);
    const traced = new TracingRangeSource(file);
    const archive = await openPangenome({ source: traced });
    try {
      archive.clearCaches();
      rows.push(await measure(archive, benchmarkCase, "cold"));
      rows.push(await measure(archive, benchmarkCase, "warm"));
    } finally {
      await archive.close();
    }
  }

  const columns = Object.keys(rows[0]);
  const csv = [
    columns.join(","),
    ...rows.map((row) =>
      columns.map((column) => csvCell(row[column])).join(","),
    ),
  ].join("\n");
  await writeFile(options.csv, `${csv}\n`);
  if (options.json !== undefined) {
    await writeFile(options.json, `${JSON.stringify(rows, jsonValue, 2)}\n`);
  }
  process.stdout.write(`${JSON.stringify(rows, jsonValue, 2)}\n`);
}

await main();
