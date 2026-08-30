import { FileRangeSource } from "../packages/browser/dist/node/index.js";
import { openPangenome } from "../packages/browser/dist/reader/index.js";

const [archivePath, sample, contig, startText, endText] = process.argv.slice(2);
const start = Number(startText);
const end = Number(endText);
if (
  !archivePath ||
  !sample ||
  !contig ||
  !Number.isSafeInteger(start) ||
  !Number.isSafeInteger(end) ||
  start >= end
) {
  throw new Error(
    "usage: node experiments/graph-query-trace.mjs ARCHIVE SAMPLE CONTIG START END",
  );
}

const source = await FileRangeSource.open(archivePath);
const archive = await openPangenome(source);
const started = performance.now();
const region = await archive.query({ sample, contig, start, end, trace: true });
const wallMs = performance.now() - started;
await archive.close();
const trace = region.trace;
process.stdout.write(
  `${JSON.stringify(
    {
      archivePath,
      query: { sample, contig, start, end },
      wallMs,
      tiles: region.tiles.length,
      bytes: trace?.totalBytes ?? 0,
      ranges: trace?.requestRanges.length ?? 0,
      dependencyRounds: trace?.dependencyRounds ?? 0,
      decodeMs: trace?.decodeMs ?? 0,
      canonicalHash: trace?.canonicalHash,
      requests: (trace?.requestRanges ?? []).map((range, index) => ({
        layer: "graph",
        index,
        offset: range.offset.toString(),
        length: range.length,
      })),
    },
    null,
    2,
  )}\n`,
);
