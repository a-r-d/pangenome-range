import type { ReferenceDescriptor } from "../reader/types.js";

export type GenomicCommand =
  | {
      readonly kind: "coordinate";
      readonly reference: ReferenceDescriptor;
      readonly start: number;
      readonly end: number;
      readonly canonical: string;
    }
  | {
      readonly kind: "locus";
      readonly name: string;
    };

/**
 * Parse the command-bar grammar without adding a second biological name index.
 * Non-coordinate input is deliberately returned as a locus command for the
 * caller to resolve through PangenomeArchive.searchLoci().
 */
export function parseGenomicCommand(
  input: string,
  references: readonly ReferenceDescriptor[],
  preferredSample?: string,
): GenomicCommand {
  const value = input.trim();
  if (value.length === 0) throw new TypeError("Enter a locus or coordinate.");
  const coordinate =
    /^(?:(?<sample>[^#\s:]+)(?:#|\s+))?(?<contig>[^:\s]+):(?<start>[\d,]+)(?:-(?<end>[\d,]+))?$/.exec(
      value,
    );
  if (coordinate === null) {
    if (value.includes(":")) {
      throw new TypeError(
        "Coordinate commands use sample#contig:start-end or contig:start-end.",
      );
    }
    return { kind: "locus", name: value };
  }

  const sample = coordinate.groups?.sample;
  const contig = coordinate.groups?.contig;
  const start = parseCoordinate(coordinate.groups?.start, "start");
  const endText = coordinate.groups?.end;
  const end =
    endText === undefined
      ? checkedCoordinateEnd(start)
      : parseCoordinate(endText, "end");
  if (end <= start)
    throw new RangeError("Coordinate end must be greater than start.");
  if (contig === undefined)
    throw new TypeError("Coordinate contig is missing.");

  const matching = references.filter(
    (reference) =>
      reference.contig === contig &&
      (sample === undefined || reference.sample === sample) &&
      reference.start < end &&
      reference.end > start,
  );
  const preferred =
    sample === undefined && preferredSample !== undefined
      ? matching.filter((reference) => reference.sample === preferredSample)
      : matching;
  const candidates = preferred.length > 0 ? preferred : matching;
  if (candidates.length === 0) {
    throw new RangeError(
      `Archive has no overlapping reference for ${sample === undefined ? "" : `${sample}#`}${contig}:${start}-${end}.`,
    );
  }
  const identities = new Set(
    candidates.map((reference) => `${reference.sample}\0${reference.contig}`),
  );
  if (identities.size > 1) {
    throw new RangeError(
      `${contig} exists in multiple reference samples; prefix the coordinate with sample# or sample and a space.`,
    );
  }
  const first = candidates[0];
  if (first === undefined) throw new RangeError("Reference resolution failed.");
  return {
    kind: "coordinate",
    reference: first,
    start,
    end,
    canonical: formatGenomicCoordinate(first.sample, contig, start, end),
  };
}

export function formatGenomicCoordinate(
  sample: string,
  contig: string,
  start: number,
  end: number,
): string {
  return `${sample}#${contig}:${start.toLocaleString("en-US")}-${end.toLocaleString("en-US")}`;
}

function parseCoordinate(value: string | undefined, label: string): number {
  if (value === undefined || !/^\d[\d,]*$/.test(value)) {
    throw new TypeError(`Coordinate ${label} is invalid.`);
  }
  const parsed = Number(value.replaceAll(",", ""));
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new RangeError(
      `Coordinate ${label} must be a non-negative safe integer.`,
    );
  }
  return parsed;
}

function checkedCoordinateEnd(start: number): number {
  if (start === Number.MAX_SAFE_INTEGER) {
    throw new RangeError("Coordinate end exceeds the safe integer range.");
  }
  return start + 1;
}
