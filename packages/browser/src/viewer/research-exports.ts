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

export type LocalTraversalExportErrorCode =
  | "missing-node"
  | "invalid-sequence-bounds"
  | "unsupported-sequence-byte";

/** A tile-local sequence cannot be exported without changing its source bytes. */
export class LocalTraversalExportError extends Error {
  readonly code: LocalTraversalExportErrorCode;
  readonly nodeId: bigint;
  readonly byteOffset: number | undefined;
  readonly byteValue: number | undefined;

  constructor(
    code: LocalTraversalExportErrorCode,
    message: string,
    details: {
      readonly nodeId: bigint;
      readonly byteOffset?: number;
      readonly byteValue?: number;
    },
  ) {
    super(message);
    this.name = "LocalTraversalExportError";
    this.code = code;
    this.nodeId = details.nodeId;
    this.byteOffset = details.byteOffset;
    this.byteValue = details.byteValue;
  }
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

/**
 * Reconstruct one tile-local traversal without normalizing source bytes.
 *
 * Sequence bytes must be ASCII IUPAC DNA (ACGTRYSWKMBDHVN, preserving case).
 * U/u is intentionally unsupported because this export is explicitly DNA.
 */
export function localTraversalSequence(
  tile: RegionTile,
  orientedNodes: readonly bigint[] | BigUint64Array,
): string {
  const indexById = new Map(
    Array.from(tile.nodes.ids, (id, index) => [id, index] as const),
  );
  const parts: string[] = [];
  for (const handle of orientedNodes) {
    const id = handle >> 1n;
    const index = indexById.get(id);
    if (index === undefined)
      throw new LocalTraversalExportError(
        "missing-node",
        `Node ${id} is absent from the tile`,
        { nodeId: id },
      );
    const start = tile.nodes.sequenceOffsets[index];
    const end = tile.nodes.sequenceOffsets[index + 1];
    if (
      start === undefined ||
      end === undefined ||
      end < start ||
      end > tile.nodes.sequenceBytes.length
    )
      throw new LocalTraversalExportError(
        "invalid-sequence-bounds",
        `Node ${id} has invalid sequence bounds`,
        { nodeId: id },
      );
    parts.push(
      sequenceBytesToString(
        tile.nodes.sequenceBytes,
        start,
        end,
        id,
        (handle & 1n) !== 0n,
      ),
    );
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

function sequenceBytesToString(
  bytes: Uint8Array,
  start: number,
  end: number,
  nodeId: bigint,
  reverse: boolean,
): string {
  const characters = new Array<string>(end - start);
  for (let output = 0; output < characters.length; output += 1) {
    const byteOffset = reverse ? end - 1 - output : start + output;
    const byte = bytes[byteOffset];
    if (byte === undefined)
      throw new LocalTraversalExportError(
        "invalid-sequence-bounds",
        `Node ${nodeId} has invalid sequence bounds`,
        { nodeId },
      );
    const encoded = reverse ? complementByte(byte) : validatedByte(byte);
    if (encoded === undefined) {
      const printable =
        byte >= 0x20 && byte <= 0x7e ? ` '${String.fromCharCode(byte)}'` : "";
      throw new LocalTraversalExportError(
        "unsupported-sequence-byte",
        `Node ${nodeId} contains unsupported DNA byte 0x${byte.toString(16).padStart(2, "0")}${printable} at sequence offset ${byteOffset}`,
        { nodeId, byteOffset, byteValue: byte },
      );
    }
    characters[output] = String.fromCharCode(encoded);
  }
  return characters.join("");
}

function validatedByte(byte: number): number | undefined {
  return complementByte(byte) === undefined ? undefined : byte;
}

function complementByte(byte: number): number | undefined {
  switch (byte) {
    case 0x41:
      return 0x54; // A -> T
    case 0x43:
      return 0x47; // C -> G
    case 0x47:
      return 0x43; // G -> C
    case 0x54:
      return 0x41; // T -> A
    case 0x52:
      return 0x59; // R -> Y
    case 0x59:
      return 0x52; // Y -> R
    case 0x53:
      return 0x53; // S -> S
    case 0x57:
      return 0x57; // W -> W
    case 0x4b:
      return 0x4d; // K -> M
    case 0x4d:
      return 0x4b; // M -> K
    case 0x42:
      return 0x56; // B -> V
    case 0x56:
      return 0x42; // V -> B
    case 0x44:
      return 0x48; // D -> H
    case 0x48:
      return 0x44; // H -> D
    case 0x4e:
      return 0x4e; // N -> N
    case 0x61:
      return 0x74; // a -> t
    case 0x63:
      return 0x67; // c -> g
    case 0x67:
      return 0x63; // g -> c
    case 0x74:
      return 0x61; // t -> a
    case 0x72:
      return 0x79; // r -> y
    case 0x79:
      return 0x72; // y -> r
    case 0x73:
      return 0x73; // s -> s
    case 0x77:
      return 0x77; // w -> w
    case 0x6b:
      return 0x6d; // k -> m
    case 0x6d:
      return 0x6b; // m -> k
    case 0x62:
      return 0x76; // b -> v
    case 0x76:
      return 0x62; // v -> b
    case 0x64:
      return 0x68; // d -> h
    case 0x68:
      return 0x64; // h -> d
    case 0x6e:
      return 0x6e; // n -> n
    default:
      return undefined;
  }
}

function tsvCell(value: string | number | bigint): string {
  return String(value).replace(/[\t\r\n]+/g, " ");
}

function safeHeader(value: string): string {
  return value.replace(/\s+/g, "_");
}
