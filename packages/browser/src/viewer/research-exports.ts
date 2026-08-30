import type {
  NamedSourcePath,
  NamedTraversalGroup,
  RegionTile,
} from "../reader/types.js";

const PATH_TSV_COLUMNS = [
  "traversal digest",
  "tile sample",
  "tile contig",
  "tile start",
  "tile end",
  "source path ID",
  "canonical path name",
  "sample",
  "contig",
  "haplotype",
  "fragment",
  "path sense",
  "local multiplicity",
  "orientation relative to displayed traversal",
] as const;

export interface NamedPathTsvOptions {
  readonly tile: RegionTile;
  readonly traversalDigest: string;
  readonly group: NamedTraversalGroup;
  readonly paths: readonly NamedSourcePath[];
}

export interface LocalTraversalFastaOptions {
  readonly archiveIdentity: string;
  readonly tile: RegionTile;
  readonly traversalDigest: string;
  readonly occurrenceWeight: bigint;
  readonly orientedNodes: readonly bigint[] | BigUint64Array;
}

/** Export exact named membership without treating catalog rows as individuals. */
export function namedPathMembershipTsv(options: NamedPathTsvOptions): string {
  const paths = new Map(options.paths.map((path) => [path.pathId, path]));
  const rows = options.group.memberships.map((membership) => {
    const path = paths.get(membership.pathId);
    if (path === undefined)
      throw new Error(`Path ${membership.pathId} is absent from the catalog`);
    return [
      options.traversalDigest,
      options.tile.reference.sample,
      options.tile.reference.contig,
      options.tile.coreStart,
      options.tile.coreEnd,
      path.pathId,
      path.canonicalName,
      path.sample,
      path.contig,
      path.haplotype,
      path.fragment,
      path.sense,
      membership.multiplicity,
      membership.reversedRelativeToGroup ? "reverse" : "forward",
    ];
  });
  return `${[PATH_TSV_COLUMNS, ...rows]
    .map((row) => row.map(tsvCell).join("\t"))
    .join("\n")}\n`;
}

/** Reconstruct one tile-local traversal, applying orientation per graph node. */
export function localTraversalSequence(
  tile: RegionTile,
  orientedNodes: readonly bigint[] | BigUint64Array,
): string {
  const indexById = new Map(
    Array.from(tile.nodes.ids, (id, index) => [id, index] as const),
  );
  const decoder = new TextDecoder();
  const parts: string[] = [];
  for (const handle of orientedNodes) {
    const id = handle >> 1n;
    const index = indexById.get(id);
    if (index === undefined)
      throw new Error(`Node ${id} is absent from the tile`);
    const start = tile.nodes.sequenceOffsets[index];
    const end = tile.nodes.sequenceOffsets[index + 1];
    if (start === undefined || end === undefined || end < start)
      throw new Error(`Node ${id} has invalid sequence bounds`);
    const sequence = decoder.decode(
      tile.nodes.sequenceBytes.subarray(start, end),
    );
    parts.push((handle & 1n) === 0n ? sequence : reverseComplement(sequence));
  }
  return parts.join("");
}

/** FASTA explicitly labels the export as regional, never a complete source path. */
export function localTraversalFasta(
  options: LocalTraversalFastaOptions,
): string {
  const sequence = localTraversalSequence(options.tile, options.orientedNodes);
  const header = [
    "local_traversal",
    `archive=${safeHeader(options.archiveIdentity)}`,
    `interval=${safeHeader(options.tile.reference.sample)}:${safeHeader(options.tile.reference.contig)}:${options.tile.coreStart}-${options.tile.coreEnd}`,
    `traversal_digest=${options.traversalDigest}`,
    `occurrence_weight=${options.occurrenceWeight}`,
    "scope=tile-local-not-complete-assembly-path",
  ].join(" ");
  const lines = sequence.match(/.{1,80}/g) ?? [""];
  return `>${header}\n${lines.join("\n")}\n`;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function reverseComplement(sequence: string): string {
  const complement: Record<string, string> = {
    A: "T",
    C: "G",
    G: "C",
    T: "A",
    N: "N",
    a: "t",
    c: "g",
    g: "c",
    t: "a",
    n: "n",
  };
  return Array.from(sequence)
    .reverse()
    .map((base) => complement[base] ?? "N")
    .join("");
}

function tsvCell(value: string | number | bigint): string {
  return String(value).replace(/[\t\r\n]+/g, " ");
}

function safeHeader(value: string): string {
  return value.replace(/\s+/g, "_");
}
