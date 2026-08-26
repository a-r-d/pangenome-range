import type { RegionTile } from "./types.js";

const RECORD_MAGIC = "PNGRGN04";
const WEIGHTED_MAGIC = "PNGRGN03";
const NAMED_MAGIC = "PNGRGN02";
const RECORD_VERSION = 4;
const CONSTRUCTION_CONTEXT = 100;
const MAX_DECODED_OCCURRENCES = 16 * 1024 * 1024;
const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);
const textDecoder = new TextDecoder("utf-8", { fatal: true });

export class CorruptRegionalPayloadError extends Error {
  override readonly name = "CorruptRegionalPayloadError";
}

export class UnsupportedRegionalPayloadVersionError extends Error {
  override readonly name = "UnsupportedRegionalPayloadVersionError";
}

export interface DecodeRegionalPayloadOptions {
  archiveOffset?: bigint;
  encodedLength?: number;
  codec?: "none" | "zstd-1" | "zstd-3" | "zstd-6";
  coreStart?: number;
  coreEnd?: number;
  referenceSample?: string;
  referenceContig?: string;
}

interface Successor {
  handle: bigint;
  offset: number;
}

interface PackedRecord {
  handle: bigint;
  occurrenceCount: number;
  successors: Successor[];
  hasPredecessor: boolean[];
}

interface ParsedRecordPayload {
  coreStart: number;
  coreEnd: number;
  sample: string;
  contig: string;
  haplotype: number;
  fragmentStart: number;
  queryOffset: number;
  nodeOffset: number;
  referenceHandle: bigint;
  referenceOccurrence: number;
  nodeIds: bigint[];
  nodeSequences: Uint8Array[];
  edges: bigint[];
  records: PackedRecord[];
}

class BinaryReader {
  readonly #bytes: Uint8Array;
  readonly #view: DataView;
  #position = 0;

  constructor(bytes: Uint8Array) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  get remaining(): number {
    return this.#bytes.byteLength - this.#position;
  }

  take(length: number): Uint8Array {
    if (!Number.isSafeInteger(length) || length < 0) {
      throw corrupt("invalid binary section length");
    }
    const end = this.#position + length;
    if (!Number.isSafeInteger(end) || end > this.#bytes.byteLength) {
      throw corrupt("unexpected end of regional payload");
    }
    const result = this.#bytes.subarray(this.#position, end);
    this.#position = end;
    return result;
  }

  u8(): number {
    return this.take(1)[0] as number;
  }

  u32(): number {
    const position = this.#position;
    this.take(4);
    return this.#view.getUint32(position, true);
  }

  u64(): bigint {
    const position = this.#position;
    this.take(8);
    return this.#view.getBigUint64(position, true);
  }

  bytes(): Uint8Array {
    return this.take(safeNumber(this.u64(), "binary byte length"));
  }

  string(): string {
    try {
      return textDecoder.decode(this.bytes());
    } catch (error) {
      throw corrupt(`invalid UTF-8 string: ${String(error)}`);
    }
  }

  finish(): void {
    if (this.remaining !== 0) {
      throw corrupt(`${this.remaining} trailing regional payload bytes`);
    }
  }
}

function corrupt(message: string): CorruptRegionalPayloadError {
  return new CorruptRegionalPayloadError(message);
}

function safeNumber(value: bigint, label: string): number {
  if (value < 0n || value > MAX_SAFE_BIGINT) {
    throw corrupt(`${label} exceeds JavaScript's safe integer range`);
  }
  return Number(value);
}

function magic(bytes: Uint8Array): string {
  if (bytes.byteLength < 8) {
    throw corrupt("regional payload is shorter than its magic");
  }
  return textDecoder.decode(bytes.subarray(0, 8));
}

export function detectRegionalPayloadVersion(bytes: Uint8Array): 2 | 3 | 4 {
  const payloadMagic = magic(bytes);
  if (payloadMagic === RECORD_MAGIC) {
    return 4;
  }
  if (payloadMagic === WEIGHTED_MAGIC) {
    return 3;
  }
  if (payloadMagic === NAMED_MAGIC) {
    return 2;
  }
  throw new UnsupportedRegionalPayloadVersionError(
    `unsupported regional payload magic ${JSON.stringify(payloadMagic)}`,
  );
}

function countBoundedByBytes(
  count: bigint,
  remaining: number,
  minimumBytes: number,
  section: string,
): number {
  const value = safeNumber(count, `${section} count`);
  if (value > Math.floor(remaining / minimumBytes)) {
    throw corrupt(`${section} count exceeds the remaining payload`);
  }
  return value;
}

function decodeBytecode(
  bytes: Uint8Array,
  position: { value: number },
): bigint {
  let result = 0n;
  let shift = 0n;
  for (;;) {
    const byte = bytes[position.value];
    if (byte === undefined) {
      throw corrupt("truncated GBWT bytecode integer");
    }
    position.value += 1;
    result += BigInt(byte & 0x7f) << shift;
    if (result > 0xffff_ffff_ffff_ffffn) {
      throw corrupt("GBWT bytecode integer overflow");
    }
    if ((byte & 0x80) === 0) {
      return result;
    }
    shift += 7n;
    if (shift >= 70n) {
      throw corrupt("GBWT bytecode integer overflow");
    }
  }
}

function decodeRecord(bytes: Uint8Array, occurrenceCount: number): Successor[] {
  if (occurrenceCount <= 0 || occurrenceCount > MAX_DECODED_OCCURRENCES) {
    throw corrupt("GBWT record occurrence count exceeds its safety bound");
  }
  const position = { value: 0 };
  const sigma = safeNumber(decodeBytecode(bytes, position), "GBWT edge count");
  if (sigma === 0 || sigma > Math.floor((bytes.length - position.value) / 2)) {
    throw corrupt("invalid GBWT edge alphabet");
  }
  const edges: Successor[] = [];
  let previousHandle = 0n;
  for (let edgeIndex = 0; edgeIndex < sigma; edgeIndex += 1) {
    const handle = previousHandle + decodeBytecode(bytes, position);
    if (edgeIndex > 0 && handle <= previousHandle) {
      throw corrupt("GBWT successor handles are not strictly sorted");
    }
    edges.push({
      handle,
      offset: safeNumber(
        decodeBytecode(bytes, position),
        "GBWT successor offset",
      ),
    });
    previousHandle = handle;
  }
  if (position.value >= bytes.length) {
    throw corrupt("GBWT record has no run-length data");
  }
  const threshold = sigma < 255 ? Math.floor(256 / sigma) : 0;
  const successors: Successor[] = [];
  while (position.value < bytes.length) {
    let rank: number;
    let runLength: number;
    if (sigma >= 255) {
      rank = safeNumber(decodeBytecode(bytes, position), "GBWT run rank");
      runLength =
        safeNumber(decodeBytecode(bytes, position), "GBWT run length") + 1;
    } else {
      const byte = bytes[position.value];
      if (byte === undefined) {
        throw corrupt("truncated GBWT run");
      }
      position.value += 1;
      rank = byte % sigma;
      runLength = Math.floor(byte / sigma) + 1;
      if (runLength === threshold) {
        runLength += safeNumber(
          decodeBytecode(bytes, position),
          "GBWT extended run length",
        );
      }
    }
    const edge = edges[rank];
    if (edge === undefined || successors.length + runLength > occurrenceCount) {
      throw corrupt("GBWT run exceeds its edge alphabet or occurrence count");
    }
    for (let index = 0; index < runLength; index += 1) {
      successors.push({
        handle: edge.handle,
        offset: edge.offset + index,
      });
    }
    edge.offset += runLength;
    if (!Number.isSafeInteger(edge.offset)) {
      throw corrupt("GBWT successor offset overflow");
    }
  }
  if (successors.length !== occurrenceCount) {
    throw corrupt("GBWT runs differ from the declared occurrence count");
  }
  return successors;
}

function canonicalEdge(from: bigint, to: bigint): boolean {
  const fromNode = from / 2n;
  const toNode = to / 2n;
  const fromReverse = from % 2n === 1n;
  const toReverse = to % 2n === 1n;
  return fromReverse
    ? toNode > fromNode || (toNode === fromNode && !toReverse)
    : toNode >= fromNode;
}

function parseRecordPayload(bytes: Uint8Array): ParsedRecordPayload {
  const reader = new BinaryReader(bytes);
  if (textDecoder.decode(reader.take(8)) !== RECORD_MAGIC) {
    throw corrupt("invalid record regional payload magic");
  }
  if (reader.u32() !== RECORD_VERSION || reader.u32() !== 1) {
    throw new UnsupportedRegionalPayloadVersionError(
      "unsupported record regional payload version or flags",
    );
  }
  if (reader.u8() !== 2 || reader.take(7).some((value) => value !== 0)) {
    throw corrupt("record payload semantics or reserved bytes are invalid");
  }
  const nodeCount = reader.u64();
  const edgeCount = reader.u64();
  const recordCount = reader.u64();
  const totalOccurrences = safeNumber(
    reader.u64(),
    "record payload occurrence total",
  );
  const coreStart = safeNumber(reader.u64(), "core start");
  const coreEnd = safeNumber(reader.u64(), "core end");
  const context = safeNumber(reader.u64(), "construction context");
  const haplotype = safeNumber(reader.u64(), "reference haplotype");
  const fragmentStart = safeNumber(reader.u64(), "reference fragment start");
  const queryOffset = safeNumber(reader.u64(), "reference query offset");
  const nodeOffset = safeNumber(reader.u64(), "reference node offset");
  const referenceHandle = reader.u64();
  const referenceOccurrence = safeNumber(
    reader.u64(),
    "reference occurrence offset",
  );
  if (
    coreStart >= coreEnd ||
    context !== CONSTRUCTION_CONTEXT ||
    fragmentStart + queryOffset !== coreStart ||
    totalOccurrences <= 0 ||
    totalOccurrences > MAX_DECODED_OCCURRENCES ||
    recordCount !== nodeCount * 2n
  ) {
    throw corrupt("record payload header invariants are invalid");
  }
  const sample = reader.string();
  const contig = reader.string();
  if (sample.length === 0 || contig.length === 0) {
    throw corrupt("record payload reference provenance is empty");
  }
  const nodes = countBoundedByBytes(nodeCount, reader.remaining, 16, "node");
  const nodeIds: bigint[] = [];
  const nodeSequences: Uint8Array[] = [];
  const localNodes = new Set<bigint>();
  let previousNode = 0n;
  for (let index = 0; index < nodes; index += 1) {
    const node = previousNode + reader.u64();
    if (node === 0n || node <= previousNode) {
      throw corrupt("record payload nodes are not strictly sorted");
    }
    const sequence = reader.bytes();
    if (sequence.length === 0) {
      throw corrupt("record payload node sequence is empty");
    }
    nodeIds.push(node);
    nodeSequences.push(sequence);
    localNodes.add(node);
    previousNode = node;
  }
  const edges: bigint[] = [];
  const edgeTotal = countBoundedByBytes(
    edgeCount,
    reader.remaining,
    16,
    "edge",
  );
  const seenEdges = new Set<string>();
  for (let index = 0; index < edgeTotal; index += 1) {
    const from = reader.u64();
    const to = reader.u64();
    if (!localNodes.has(from / 2n) || !canonicalEdge(from, to)) {
      throw corrupt("record payload edge source or orientation is invalid");
    }
    const key = `${from}:${to}`;
    if (seenEdges.has(key)) {
      throw corrupt("record payload contains a duplicate edge");
    }
    seenEdges.add(key);
    edges.push(from, to);
  }
  const records: PackedRecord[] = [];
  const recordsTotal = countBoundedByBytes(
    recordCount,
    reader.remaining,
    24,
    "GBWT record",
  );
  let previousRecord: bigint | undefined;
  let decodedOccurrences = 0;
  for (let index = 0; index < recordsTotal; index += 1) {
    const handle = reader.u64();
    const occurrenceCount = safeNumber(
      reader.u64(),
      "GBWT record occurrence count",
    );
    const recordBytes = reader.bytes();
    if (
      handle === 0n ||
      (previousRecord !== undefined && handle <= previousRecord) ||
      !localNodes.has(handle / 2n)
    ) {
      throw corrupt(
        "record payload handle ordering or node identity is invalid",
      );
    }
    const successors = decodeRecord(recordBytes, occurrenceCount);
    records.push({
      handle,
      occurrenceCount,
      successors,
      hasPredecessor: new Array<boolean>(occurrenceCount).fill(false),
    });
    decodedOccurrences += occurrenceCount;
    previousRecord = handle;
  }
  reader.finish();
  if (decodedOccurrences !== totalOccurrences) {
    throw corrupt("record payload occurrence total differs from its records");
  }
  const referenceRecord = records.find(
    (record) => record.handle === referenceHandle,
  );
  const referenceNodeIndex = nodeIds.findIndex(
    (node) => node === referenceHandle / 2n,
  );
  if (
    referenceRecord === undefined ||
    referenceOccurrence >= referenceRecord.occurrenceCount ||
    referenceNodeIndex < 0 ||
    nodeOffset >= (nodeSequences[referenceNodeIndex]?.length ?? 0)
  ) {
    throw corrupt("record payload reference anchor is invalid");
  }
  return {
    coreStart,
    coreEnd,
    sample,
    contig,
    haplotype,
    fragmentStart,
    queryOffset,
    nodeOffset,
    referenceHandle,
    referenceOccurrence,
    nodeIds,
    nodeSequences,
    edges,
    records,
  };
}

function comparePaths(left: bigint[], right: bigint[]): number {
  const limit = Math.min(left.length, right.length);
  for (let index = 0; index < limit; index += 1) {
    const a = left[index] as bigint;
    const b = right[index] as bigint;
    if (a < b) return -1;
    if (a > b) return 1;
  }
  return left.length - right.length;
}

function samePath(left: bigint[], right: bigint[]): boolean {
  return comparePaths(left, right) === 0;
}

function canonicalPath(path: bigint[]): boolean {
  const first = path[0];
  const last = path.at(-1);
  if (first === undefined || last === undefined) return true;
  const firstReverse = first % 2n === 1n;
  const lastReverse = last % 2n === 1n;
  if (firstReverse === lastReverse) return !firstReverse;
  const firstNode = first / 2n;
  const lastNode = last / 2n;
  return firstReverse
    ? lastNode > firstNode || (lastNode === firstNode && !lastReverse)
    : lastNode >= firstNode;
}

function reconstructPaths(payload: ParsedRecordPayload): {
  reference: bigint[];
  referenceStart: number;
  referenceEnd: number;
  traversals: Array<{ weight: bigint; path: bigint[] }>;
} {
  const recordByHandle = new Map(
    payload.records.map((record) => [record.handle, record] as const),
  );
  for (const record of payload.records) {
    for (const successor of record.successors) {
      const target = recordByHandle.get(successor.handle);
      if (target === undefined || successor.handle === 0n) continue;
      if (successor.offset >= target.hasPredecessor.length) {
        throw corrupt("GBWT successor offset is outside the local record");
      }
      target.hasPredecessor[successor.offset] = true;
    }
  }
  const paths: bigint[][] = [];
  let referencePath: bigint[] | undefined;
  let referencePathOffset: number | undefined;
  const occurrenceLimit = payload.records.reduce(
    (sum, record) => sum + record.occurrenceCount,
    0,
  );
  for (const record of payload.records) {
    for (let offset = 0; offset < record.occurrenceCount; offset += 1) {
      if (record.hasPredecessor[offset]) continue;
      let position: Successor | undefined = { handle: record.handle, offset };
      const path: bigint[] = [];
      let matchedReference: number | undefined;
      while (position !== undefined) {
        if (
          position.handle === payload.referenceHandle &&
          position.offset === payload.referenceOccurrence
        ) {
          matchedReference = path.length;
        }
        path.push(position.handle);
        const current = recordByHandle.get(position.handle);
        const next: Successor | undefined =
          current?.successors[position.offset];
        if (next === undefined) {
          throw corrupt("local GBWT traversal is out of bounds");
        }
        position =
          next.handle !== 0n && recordByHandle.has(next.handle)
            ? next
            : undefined;
        if (path.length > occurrenceLimit) {
          throw corrupt("cyclic local GBWT traversal");
        }
      }
      if (matchedReference !== undefined) {
        referencePathOffset = matchedReference;
        referencePath = path;
        paths.push(path);
      } else if (canonicalPath(path)) {
        paths.push(path);
      }
    }
  }
  if (referencePath === undefined || referencePathOffset === undefined) {
    throw corrupt("record payload does not contain its reference anchor path");
  }
  const sequenceLength = new Map(
    payload.nodeIds.map(
      (node, index) =>
        [node, payload.nodeSequences[index]?.length ?? 0] as const,
    ),
  );
  const referenceLength = referencePath.reduce((sum, handle) => {
    const length = sequenceLength.get(handle / 2n);
    if (length === undefined) throw corrupt("reference handle is not local");
    return sum + length;
  }, 0);
  const prefixLength = referencePath
    .slice(0, referencePathOffset)
    .reduce((sum, handle) => {
      const length = sequenceLength.get(handle / 2n);
      if (length === undefined) throw corrupt("reference handle is not local");
      return sum + length;
    }, payload.nodeOffset);
  const relativeStart = payload.queryOffset - prefixLength;
  if (relativeStart < 0) {
    throw corrupt("reference context starts before its fragment");
  }
  const referenceStart = payload.fragmentStart + relativeStart;
  const referenceEnd = referenceStart + referenceLength;
  if (!Number.isSafeInteger(referenceEnd)) {
    throw corrupt("reference interval exceeds JavaScript's safe integer range");
  }
  paths.sort(comparePaths);
  const traversals: Array<{ weight: bigint; path: bigint[] }> = [];
  for (let index = 0; index < paths.length; ) {
    let end = index + 1;
    while (
      end < paths.length &&
      samePath(paths[index] as bigint[], paths[end] as bigint[])
    ) {
      end += 1;
    }
    const path = paths[index] as bigint[];
    const count = BigInt(end - index);
    const weight = samePath(path, referencePath) ? count - 1n : count;
    if (weight > 0n) traversals.push({ weight, path });
    index = end;
  }
  traversals.sort((left, right) => comparePaths(left.path, right.path));
  return { reference: referencePath, referenceStart, referenceEnd, traversals };
}

function flattenSequences(sequences: Uint8Array[]): {
  offsets: Uint32Array;
  bytes: Uint8Array;
} {
  const total = sequences.reduce((sum, sequence) => sum + sequence.length, 0);
  if (total > 0xffff_ffff)
    throw corrupt("node sequences exceed Uint32 offsets");
  const offsets = new Uint32Array(sequences.length + 1);
  const bytes = new Uint8Array(total);
  let position = 0;
  for (let index = 0; index < sequences.length; index += 1) {
    offsets[index] = position;
    const sequence = sequences[index] as Uint8Array;
    bytes.set(sequence, position);
    position += sequence.length;
  }
  offsets[sequences.length] = position;
  return { offsets, bytes };
}

function topologyTables(edges: BigUint64Array): {
  from: BigUint64Array;
  to: BigUint64Array;
} {
  if (edges.length % 2 !== 0) throw corrupt("edge table is not paired");
  const from = new BigUint64Array(edges.length / 2);
  const to = new BigUint64Array(edges.length / 2);
  for (let index = 0; index < from.length; index += 1) {
    from[index] = edges[index * 2] as bigint;
    to[index] = edges[index * 2 + 1] as bigint;
  }
  return { from, to };
}

function weightedSemantics(
  code: number,
): "anonymous-all-tile-paths" | "anonymous-distinct-weighted-tile-paths" {
  if (code === 1) return "anonymous-all-tile-paths";
  if (code === 2) return "anonymous-distinct-weighted-tile-paths";
  throw corrupt(`unsupported weighted haplotype semantics code ${code}`);
}

function decodeWeightedPayload(
  bytes: Uint8Array,
  options: DecodeRegionalPayloadOptions,
): RegionTile {
  const reader = new BinaryReader(bytes);
  if (textDecoder.decode(reader.take(8)) !== WEIGHTED_MAGIC) {
    throw corrupt("invalid weighted regional payload magic");
  }
  if (reader.u32() !== 3 || reader.u32() !== 1) {
    throw corrupt("unsupported weighted regional version or flags");
  }
  const semantics = weightedSemantics(reader.u8());
  if (reader.take(7).some((value) => value !== 0)) {
    throw corrupt("weighted regional reserved bytes are nonzero");
  }
  const nodeCount = countBoundedByBytes(
    reader.u64(),
    reader.remaining,
    16,
    "weighted node",
  );
  const edgeCount = safeNumber(reader.u64(), "weighted edge count");
  const coreStart = safeNumber(reader.u64(), "weighted core start");
  const coreEnd = safeNumber(reader.u64(), "weighted core end");
  const context = safeNumber(reader.u64(), "weighted construction context");
  const referenceStart = safeNumber(reader.u64(), "weighted reference start");
  const referenceEnd = safeNumber(reader.u64(), "weighted reference end");
  const haplotype = reader.u64();
  const referenceCount = safeNumber(
    reader.u64(),
    "weighted reference traversal count",
  );
  const traversalCount = safeNumber(reader.u64(), "weighted traversal count");
  if (
    coreStart >= coreEnd ||
    referenceStart >= referenceEnd ||
    context !== CONSTRUCTION_CONTEXT
  ) {
    throw corrupt("weighted regional interval or context is invalid");
  }
  const sample = reader.string();
  const contig = reader.string();
  const nodeIds: bigint[] = [];
  const nodeSequences: Uint8Array[] = [];
  let previousNode = 0n;
  for (let index = 0; index < nodeCount; index += 1) {
    const node = previousNode + reader.u64();
    if (index > 0 && node <= previousNode) {
      throw corrupt("weighted node identifiers are not strictly increasing");
    }
    nodeIds.push(node);
    nodeSequences.push(reader.bytes());
    previousNode = node;
  }
  if (edgeCount > Math.floor(reader.remaining / 16)) {
    throw corrupt("weighted edge count exceeds the remaining payload");
  }
  const edges = new BigUint64Array(edgeCount * 2);
  for (let index = 0; index < edgeCount; index += 1) {
    edges[index * 2] = reader.u64();
    edges[index * 2 + 1] = reader.u64();
  }
  if (referenceCount > Math.floor(reader.remaining / 8)) {
    throw corrupt("weighted reference traversal exceeds the payload");
  }
  const referenceTraversal = new BigUint64Array(referenceCount);
  for (let index = 0; index < referenceCount; index += 1) {
    referenceTraversal[index] = reader.u64();
  }
  const sequenceLength = new Map(
    nodeIds.map((id, index) => [id, nodeSequences[index]?.byteLength ?? 0]),
  );
  const traversalEnd = Array.from(referenceTraversal).reduce(
    (coordinate, handle) => {
      const length = sequenceLength.get(handle / 2n);
      if (length === undefined) {
        throw corrupt("weighted reference traversal uses an absent node");
      }
      return coordinate + length;
    },
    referenceStart,
  );
  if (traversalEnd !== referenceEnd) {
    throw corrupt(
      "weighted reference traversal length differs from its interval",
    );
  }
  if (traversalCount > Math.floor(reader.remaining / 16)) {
    throw corrupt("weighted traversal count exceeds the remaining payload");
  }
  const traversalOffsets = new Uint32Array(traversalCount + 1);
  const traversalWeights = new BigUint64Array(traversalCount);
  const traversalNodes: bigint[] = [];
  for (let index = 0; index < traversalCount; index += 1) {
    const weight = reader.u64();
    const visitCount = safeNumber(reader.u64(), "weighted traversal visits");
    if (
      weight === 0n ||
      visitCount === 0 ||
      visitCount > Math.floor(reader.remaining / 8)
    ) {
      throw corrupt("weighted traversal is empty or exceeds the payload");
    }
    traversalOffsets[index] = traversalNodes.length;
    traversalWeights[index] = weight;
    for (let visit = 0; visit < visitCount; visit += 1) {
      const handle = reader.u64();
      if (!sequenceLength.has(handle / 2n)) {
        throw corrupt("weighted traversal uses an absent node");
      }
      traversalNodes.push(handle);
    }
    if (traversalNodes.length > 0xffff_ffff) {
      throw corrupt("weighted traversal nodes exceed Uint32 offsets");
    }
  }
  traversalOffsets[traversalCount] = traversalNodes.length;
  reader.finish();
  const sequences = flattenSequences(nodeSequences);
  const ids = BigUint64Array.from(nodeIds);
  const orientedNodes = BigUint64Array.from(traversalNodes);
  const nodes = {
    ids,
    sequenceOffsets: sequences.offsets,
    sequenceBytes: sequences.bytes,
  } as const;
  const topology = topologyTables(edges);
  const haplotypes = {
    kind: "weighted-traversals",
    traversalOffsets,
    orientedNodes,
    weights: traversalWeights,
  } as const;
  const archiveOffset = options.archiveOffset ?? 0n;
  const encodedLength = options.encodedLength ?? bytes.byteLength;
  const codec = options.codec ?? "none";
  return {
    reference: {
      sample,
      contig,
      start: referenceStart,
      end: referenceEnd,
      haplotype: safeNumber(haplotype, "weighted reference haplotype"),
      orientation: "forward",
    },
    coreStart,
    coreEnd,
    start: coreStart,
    end: coreEnd,
    semantics,
    nodes,
    topology,
    haplotypes,
    provenance: {
      archiveOffset,
      compressedBytes: encodedLength,
      uncompressedBytes: bytes.byteLength,
      codec,
    },
    archiveOffset,
    encodedLength,
    nodeIds: ids,
    nodeSequenceOffsets: sequences.offsets,
    nodeSequences: sequences.bytes,
    edges,
    referenceTraversal,
    traversalOffsets,
    traversalNodes: orientedNodes,
    traversalWeights,
  };
}

interface NamedPath {
  pathId: bigint;
  haplotype: bigint;
  fragment: bigint;
  sampleId: number;
  contigId: number;
  isReference: boolean;
  visitIndices: bigint[];
  nodes: bigint[];
}

function decodeNamedPayload(
  bytes: Uint8Array,
  options: DecodeRegionalPayloadOptions,
): RegionTile {
  const reader = new BinaryReader(bytes);
  if (textDecoder.decode(reader.take(8)) !== NAMED_MAGIC) {
    throw corrupt("invalid named regional payload magic");
  }
  if (reader.u32() !== 2 || reader.u32() !== 1) {
    throw corrupt("unsupported named regional version or flags");
  }
  const nodeCount = countBoundedByBytes(
    reader.u64(),
    reader.remaining,
    16,
    "named node",
  );
  const edgeCount = safeNumber(reader.u64(), "named edge count");
  const pathCount = safeNumber(reader.u64(), "named path count");
  const referenceCount = safeNumber(
    reader.u64(),
    "named reference visit count",
  );
  const sampleCount = reader.u32();
  const contigCount = reader.u32();
  const nodeIds: bigint[] = [];
  const nodeSequences: Uint8Array[] = [];
  let previousNode = 0n;
  for (let index = 0; index < nodeCount; index += 1) {
    const node = previousNode + reader.u64();
    if (index > 0 && node <= previousNode) {
      throw corrupt("named node identifiers are not strictly increasing");
    }
    nodeIds.push(node);
    nodeSequences.push(reader.bytes());
    previousNode = node;
  }
  if (edgeCount > Math.floor(reader.remaining / 16)) {
    throw corrupt("named edge count exceeds the remaining payload");
  }
  const edges = new BigUint64Array(edgeCount * 2);
  for (let index = 0; index < edgeCount; index += 1) {
    edges[index * 2] = reader.u64();
    edges[index * 2 + 1] = reader.u64();
  }
  if (
    sampleCount > Math.floor(reader.remaining / 8) ||
    contigCount > Math.floor(reader.remaining / 8)
  ) {
    throw corrupt("named string dictionary count exceeds the payload");
  }
  const samples = Array.from({ length: sampleCount }, () => reader.string());
  const contigs = Array.from({ length: contigCount }, () => reader.string());
  if (pathCount > Math.floor(reader.remaining / 56)) {
    throw corrupt("named path count exceeds the remaining payload");
  }
  const paths: NamedPath[] = [];
  const nodeSet = new Set(nodeIds);
  for (let index = 0; index < pathCount; index += 1) {
    const pathId = reader.u64();
    const haplotype = reader.u64();
    const fragment = reader.u64();
    const sampleId = reader.u32();
    const contigId = reader.u32();
    const isReference = reader.u8() !== 0;
    if (reader.take(7).some((value) => value !== 0)) {
      throw corrupt("named path reserved bytes are nonzero");
    }
    if (sampleId >= samples.length || contigId >= contigs.length) {
      throw corrupt("named path dictionary index is out of range");
    }
    const visitCount = safeNumber(reader.u64(), "named path visit count");
    if (visitCount > Math.floor(reader.remaining / 16)) {
      throw corrupt("named path visits exceed the remaining payload");
    }
    const visitIndices: bigint[] = [];
    const nodes: bigint[] = [];
    let previousVisit = 0n;
    for (let visit = 0; visit < visitCount; visit += 1) {
      const visitIndex = previousVisit + reader.u64();
      if (visit > 0 && visitIndex <= previousVisit) {
        throw corrupt("named path visits are not strictly increasing");
      }
      const node = reader.u64();
      if (!nodeSet.has(node / 2n)) {
        throw corrupt("named path uses an absent node");
      }
      visitIndices.push(visitIndex);
      nodes.push(node);
      previousVisit = visitIndex;
    }
    paths.push({
      pathId,
      haplotype,
      fragment,
      sampleId,
      contigId,
      isReference,
      visitIndices,
      nodes,
    });
  }
  if (referenceCount > Math.floor(reader.remaining / 40)) {
    throw corrupt("named reference visits exceed the remaining payload");
  }
  const referenceVisitPathIds = new BigUint64Array(referenceCount);
  const referenceVisitIndices = new BigUint64Array(referenceCount);
  const referenceVisitStarts = new BigUint64Array(referenceCount);
  const referenceVisitEnds = new BigUint64Array(referenceCount);
  const referenceVisitNodes = new BigUint64Array(referenceCount);
  for (let index = 0; index < referenceCount; index += 1) {
    referenceVisitPathIds[index] = reader.u64();
    referenceVisitIndices[index] = reader.u64();
    const start = reader.u64();
    const end = reader.u64();
    referenceVisitStarts[index] = start;
    referenceVisitEnds[index] = end;
    referenceVisitNodes[index] = reader.u64();
    if (start >= end) {
      throw corrupt("named reference visit interval is empty");
    }
  }
  reader.finish();
  const referencePath = paths.find(
    (path) =>
      path.isReference &&
      (options.referenceSample === undefined ||
        samples[path.sampleId] === options.referenceSample) &&
      (options.referenceContig === undefined ||
        contigs[path.contigId] === options.referenceContig),
  );
  if (referencePath === undefined) {
    throw corrupt("named payload has no matching reference path");
  }
  const referenceRows = Array.from({ length: referenceCount }, (_, index) => ({
    pathId: referenceVisitPathIds[index] as bigint,
    start: referenceVisitStarts[index] as bigint,
    end: referenceVisitEnds[index] as bigint,
  })).filter(({ pathId }) => pathId === referencePath.pathId);
  if (referenceRows.length === 0) {
    throw corrupt("named payload has no coordinate-bearing reference visits");
  }
  const derivedStart = referenceRows.reduce(
    (minimum, row) => (row.start < minimum ? row.start : minimum),
    referenceRows[0]?.start ?? 0n,
  );
  const derivedEnd = referenceRows.reduce(
    (maximum, row) => (row.end > maximum ? row.end : maximum),
    referenceRows[0]?.end ?? 0n,
  );
  const sequences = flattenSequences(nodeSequences);
  const ids = BigUint64Array.from(nodeIds);
  const visitTotal = paths.reduce(
    (total, path) => total + path.nodes.length,
    0,
  );
  if (visitTotal > 0xffff_ffff) {
    throw corrupt("named path visits exceed Uint32 offsets");
  }
  const visitOffsets = new Uint32Array(paths.length + 1);
  const visitIndices = new BigUint64Array(visitTotal);
  const orientedNodes = new BigUint64Array(visitTotal);
  let visitPosition = 0;
  paths.forEach((path, index) => {
    visitOffsets[index] = visitPosition;
    visitIndices.set(path.visitIndices, visitPosition);
    orientedNodes.set(path.nodes, visitPosition);
    visitPosition += path.nodes.length;
  });
  visitOffsets[paths.length] = visitPosition;
  const haplotypes = {
    kind: "named-paths",
    samples,
    contigs,
    pathIds: BigUint64Array.from(paths.map(({ pathId }) => pathId)),
    sampleIds: Uint32Array.from(paths.map(({ sampleId }) => sampleId)),
    contigIds: Uint32Array.from(paths.map(({ contigId }) => contigId)),
    haplotypes: BigUint64Array.from(paths.map(({ haplotype }) => haplotype)),
    fragments: BigUint64Array.from(paths.map(({ fragment }) => fragment)),
    referenceFlags: Uint8Array.from(
      paths.map(({ isReference }) => Number(isReference)),
    ),
    visitOffsets,
    visitIndices,
    orientedNodes,
    referenceVisitPathIds,
    referenceVisitIndices,
    referenceVisitStarts,
    referenceVisitEnds,
    referenceVisitNodes,
  } as const;
  const packedEdges = edges;
  const topology = topologyTables(packedEdges);
  const archiveOffset = options.archiveOffset ?? 0n;
  const encodedLength = options.encodedLength ?? bytes.byteLength;
  const coreStart =
    options.coreStart ?? safeNumber(derivedStart, "named core start");
  const coreEnd = options.coreEnd ?? safeNumber(derivedEnd, "named core end");
  const referenceTraversal = BigUint64Array.from(referencePath.nodes);
  return {
    reference: {
      sample: samples[referencePath.sampleId] as string,
      contig: contigs[referencePath.contigId] as string,
      start: safeNumber(derivedStart, "named reference start"),
      end: safeNumber(derivedEnd, "named reference end"),
      fragment: safeNumber(referencePath.fragment, "named reference fragment"),
      orientation: "forward",
    },
    coreStart,
    coreEnd,
    start: coreStart,
    end: coreEnd,
    semantics: "named-paths-v3",
    nodes: {
      ids,
      sequenceOffsets: sequences.offsets,
      sequenceBytes: sequences.bytes,
    },
    topology,
    haplotypes,
    provenance: {
      archiveOffset,
      compressedBytes: encodedLength,
      uncompressedBytes: bytes.byteLength,
      codec: options.codec ?? "none",
    },
    archiveOffset,
    encodedLength,
    nodeIds: ids,
    nodeSequenceOffsets: sequences.offsets,
    nodeSequences: sequences.bytes,
    edges: packedEdges,
    referenceTraversal,
    traversalOffsets: new Uint32Array(1),
    traversalNodes: new BigUint64Array(),
    traversalWeights: new BigUint64Array(),
  };
}

/** Decodes the record-preserving v4 regional payload into typed arrays. */
export function decodeRegionalPayload(
  bytes: Uint8Array,
  options: DecodeRegionalPayloadOptions = {},
): RegionTile {
  const version = detectRegionalPayloadVersion(bytes);
  if (version === 2) return decodeNamedPayload(bytes, options);
  if (version === 3) return decodeWeightedPayload(bytes, options);
  const payload = parseRecordPayload(bytes);
  const reconstructed = reconstructPaths(payload);
  const sequences = flattenSequences(payload.nodeSequences);
  const traversalOffsets = new Uint32Array(reconstructed.traversals.length + 1);
  const traversalVisitCount = reconstructed.traversals.reduce(
    (sum, traversal) => sum + traversal.path.length,
    0,
  );
  if (traversalVisitCount > 0xffff_ffff) {
    throw corrupt("weighted traversals exceed Uint32 offsets");
  }
  const traversalNodes = new BigUint64Array(traversalVisitCount);
  const traversalWeights = new BigUint64Array(reconstructed.traversals.length);
  let traversalPosition = 0;
  reconstructed.traversals.forEach((traversal, index) => {
    traversalOffsets[index] = traversalPosition;
    traversalNodes.set(traversal.path, traversalPosition);
    traversalWeights[index] = traversal.weight;
    traversalPosition += traversal.path.length;
  });
  traversalOffsets[reconstructed.traversals.length] = traversalPosition;
  const encodedLength = options.encodedLength ?? bytes.byteLength;
  if (!Number.isSafeInteger(encodedLength) || encodedLength < 0) {
    throw new RangeError("encodedLength must be a non-negative safe integer");
  }
  const nodeIds = BigUint64Array.from(payload.nodeIds);
  const packedEdges = BigUint64Array.from(payload.edges);
  const edgeCount = packedEdges.length / 2;
  const edgeFrom = new BigUint64Array(edgeCount);
  const edgeTo = new BigUint64Array(edgeCount);
  for (let index = 0; index < edgeCount; index += 1) {
    edgeFrom[index] = packedEdges[index * 2] as bigint;
    edgeTo[index] = packedEdges[index * 2 + 1] as bigint;
  }
  const nodes = {
    ids: nodeIds,
    sequenceOffsets: sequences.offsets,
    sequenceBytes: sequences.bytes,
  } as const;
  const topology = { from: edgeFrom, to: edgeTo } as const;
  const haplotypes = {
    kind: "weighted-traversals",
    traversalOffsets,
    orientedNodes: traversalNodes,
    weights: traversalWeights,
  } as const;
  const archiveOffset = options.archiveOffset ?? 0n;
  const codec = options.codec ?? "none";
  return {
    reference: {
      sample: payload.sample,
      contig: payload.contig,
      start: reconstructed.referenceStart,
      end: reconstructed.referenceEnd,
      fragment: payload.fragmentStart,
      orientation: "forward",
    },
    coreStart: payload.coreStart,
    coreEnd: payload.coreEnd,
    start: payload.coreStart,
    end: payload.coreEnd,
    semantics: "anonymous-distinct-weighted-tile-paths",
    nodes,
    topology,
    haplotypes,
    provenance: {
      archiveOffset,
      compressedBytes: encodedLength,
      uncompressedBytes: bytes.byteLength,
      codec,
    },
    archiveOffset,
    encodedLength,
    nodeIds,
    nodeSequenceOffsets: sequences.offsets,
    nodeSequences: sequences.bytes,
    edges: packedEdges,
    referenceTraversal: BigUint64Array.from(reconstructed.reference),
    traversalOffsets,
    traversalNodes,
    traversalWeights,
  };
}
