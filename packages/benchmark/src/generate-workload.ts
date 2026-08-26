import { open } from "node:fs/promises";
import { FileRangeSource } from "pangenome-range/node";
import {
  openPangenome,
  type PangenomeArchive,
  type ReferenceDescriptor,
} from "pangenome-range/reader";
import { sha256File } from "./reporting.js";
import type { BenchmarkQuery, BenchmarkWorkload } from "./types.js";

class XorShift64 {
  #state: bigint;

  constructor(seed: bigint) {
    this.#state = seed === 0n ? 1n : BigInt.asUintN(64, seed);
  }

  next(): bigint {
    let value = this.#state;
    value ^= value << 13n;
    value ^= value >> 7n;
    value ^= value << 17n;
    this.#state = BigInt.asUintN(64, value);
    return this.#state;
  }

  bounded(limit: number): number {
    if (!Number.isSafeInteger(limit) || limit <= 0) return 0;
    return Number(this.next() % BigInt(limit));
  }
}

type QueryCoordinates = Omit<
  BenchmarkQuery,
  "expectedCanonicalHash" | "expectedError"
>;

function covered(
  references: readonly ReferenceDescriptor[],
  query: QueryCoordinates,
): boolean {
  return references.some(
    (reference) =>
      reference.sample === query.sample &&
      reference.contig === query.contig &&
      reference.start <= query.start &&
      reference.end >= query.end,
  );
}

async function withHash(
  archive: PangenomeArchive,
  query: QueryCoordinates,
): Promise<BenchmarkQuery> {
  const result = await archive.query({ ...query, trace: true });
  if (result.trace === undefined) {
    throw new Error(`query ${query.id} did not produce a canonical trace`);
  }
  return { ...query, expectedCanonicalHash: result.trace.canonicalHash };
}

function interval(
  id: string,
  queryClass: string,
  reference: ReferenceDescriptor,
  start: number,
  end: number,
): QueryCoordinates {
  return {
    id,
    class: queryClass,
    sample: reference.sample,
    contig: reference.contig,
    start,
    end,
    context: 100,
  };
}

export async function generateWorkload(options: {
  readonly archivePath: string;
  readonly outputPath: string;
  readonly randomPerSize: number;
  readonly seed: bigint;
  readonly includeAbsent: boolean;
}): Promise<BenchmarkWorkload> {
  if (
    !Number.isSafeInteger(options.randomPerSize) ||
    options.randomPerSize < 0
  ) {
    throw new RangeError("randomPerSize must be a non-negative safe integer");
  }
  const input = await FileRangeSource.open(options.archivePath);
  const archive = await openPangenome(input);
  try {
    const references = archive.references();
    if (references.length === 0) throw new Error("archive has no references");
    const candidates: QueryCoordinates[] = [];
    const hardLoci = [
      interval(
        "fixed-micb",
        "fixed-biological-micb",
        { sample: "GRCh38", contig: "chr6", start: 0, end: 0 },
        31_498_145,
        31_511_124,
      ),
      interval(
        "fixed-kir3dl1",
        "fixed-biological-kir3dl1",
        { sample: "GRCh38", contig: "chr19", start: 0, end: 0 },
        54_816_468,
        54_830_778,
      ),
    ];
    candidates.push(...hardLoci.filter((query) => covered(references, query)));

    const anchorReference = references[0] as ReferenceDescriptor;
    const anchorLength = Math.min(
      10_000,
      anchorReference.end - anchorReference.start,
    );
    if (anchorLength > 0) {
      const anchorStart = anchorReference.start;
      candidates.push(
        interval(
          "boundary-start",
          "boundary-start",
          anchorReference,
          anchorStart,
          anchorStart + Math.min(1_000, anchorLength),
        ),
      );
      const middleStart =
        anchorReference.start +
        Math.floor(
          (anchorReference.end - anchorReference.start - anchorLength) / 2,
        );
      candidates.push(
        interval(
          "pan-anchor",
          "pan-anchor",
          anchorReference,
          middleStart,
          middleStart + anchorLength,
        ),
      );
      const nearbyStart = Math.min(
        anchorReference.end - anchorLength,
        middleStart + Math.max(1, Math.floor(anchorLength / 2)),
      );
      candidates.push(
        interval(
          "nearby-pan",
          "nearby-pan",
          anchorReference,
          nearbyStart,
          nearbyStart + anchorLength,
        ),
      );
      candidates.push(
        interval(
          "boundary-end",
          "boundary-end",
          anchorReference,
          anchorReference.end - Math.min(1_000, anchorLength),
          anchorReference.end,
        ),
      );
    }
    const distantReference = references.at(-1) as ReferenceDescriptor;
    const distantLength = Math.min(
      10_000,
      distantReference.end - distantReference.start,
    );
    if (distantLength > 0) {
      const start =
        distantReference.start +
        Math.floor(
          (distantReference.end - distantReference.start - distantLength) / 2,
        );
      candidates.push(
        interval(
          "distant-random",
          "distant-random",
          distantReference,
          start,
          start + distantLength,
        ),
      );
    }

    const random = new XorShift64(options.seed);
    for (const size of [1_000, 10_000, 100_000, 1_000_000]) {
      const eligible = references.filter(
        (reference) => reference.end - reference.start >= size,
      );
      for (let ordinal = 0; ordinal < options.randomPerSize; ordinal += 1) {
        if (eligible.length === 0) break;
        const reference = eligible[
          random.bounded(eligible.length)
        ] as ReferenceDescriptor;
        const available = reference.end - reference.start - size + 1;
        const start = reference.start + random.bounded(available);
        candidates.push(
          interval(
            `random-${size}-${String(ordinal).padStart(5, "0")}`,
            `random-${size}`,
            reference,
            start,
            start + size,
          ),
        );
      }
    }

    const unique = new Map<string, QueryCoordinates>();
    for (const query of candidates) {
      unique.set(query.id, query);
    }
    const queries: BenchmarkQuery[] = [];
    for (const query of unique.values())
      queries.push(await withHash(archive, query));
    if (options.includeAbsent) {
      queries.push({
        id: "absent-reference",
        class: "absent-region",
        sample: "__absent_sample__",
        contig: "__absent_contig__",
        start: 0,
        end: 1_000,
        context: 100,
        expectedError: "reference-not-found",
      });
    }
    const workload: BenchmarkWorkload = {
      schemaVersion: 1,
      archiveSha256: await sha256File(options.archivePath),
      seed: `0x${options.seed.toString(16)}`,
      queries,
    };
    const handle = await open(options.outputPath, "wx");
    try {
      await handle.writeFile(`${JSON.stringify(workload, null, 2)}\n`);
    } finally {
      await handle.close();
    }
    return workload;
  } finally {
    await archive.close();
  }
}
