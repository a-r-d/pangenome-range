import { blake3 } from "@noble/hashes/blake3.js";
import type {
  EdgeTable,
  NodeTable,
  ReferenceDescriptor,
  RegionGraph,
  RegionQuery,
  RegionTile,
} from "./types.js";

const textEncoder = new TextEncoder();

function corrupt(message: string): Error {
  return new Error(`cannot assemble canonical query graph: ${message}`);
}

function unpack(handle: bigint): { id: bigint; reverse: boolean } {
  return { id: handle / 2n, reverse: handle % 2n === 1n };
}

function flip(handle: bigint): bigint {
  return handle ^ 1n;
}

function sequenceAt(tile: RegionTile, index: number): Uint8Array {
  const start = tile.nodeSequenceOffsets[index];
  const end = tile.nodeSequenceOffsets[index + 1];
  if (start === undefined || end === undefined) {
    throw corrupt("node sequence offsets are truncated");
  }
  return tile.nodeSequences.subarray(start, end);
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.byteLength === right.byteLength &&
    left.every((value, index) => value === right[index])
  );
}

interface HeapItem {
  distance: number;
  id: bigint;
  right: boolean;
}

class MinHeap {
  readonly #items: HeapItem[] = [];

  get length(): number {
    return this.#items.length;
  }

  push(item: HeapItem): void {
    let index = this.#items.length;
    this.#items.push(item);
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      const parentItem = this.#items[parent] as HeapItem;
      if (compareHeap(parentItem, item) <= 0) break;
      this.#items[index] = parentItem;
      index = parent;
    }
    this.#items[index] = item;
  }

  pop(): HeapItem {
    const first = this.#items[0];
    const last = this.#items.pop();
    if (first === undefined || last === undefined) {
      throw corrupt("context traversal heap is empty");
    }
    if (this.#items.length === 0) return first;
    let index = 0;
    while (true) {
      const left = index * 2 + 1;
      const right = left + 1;
      if (left >= this.#items.length) break;
      let child = left;
      if (
        right < this.#items.length &&
        compareHeap(
          this.#items[right] as HeapItem,
          this.#items[left] as HeapItem,
        ) < 0
      ) {
        child = right;
      }
      const childItem = this.#items[child] as HeapItem;
      if (compareHeap(last, childItem) <= 0) break;
      this.#items[index] = childItem;
      index = child;
    }
    this.#items[index] = last;
    return first;
  }
}

function compareHeap(left: HeapItem, right: HeapItem): number {
  if (left.distance !== right.distance) return left.distance - right.distance;
  if (left.id !== right.id) return left.id < right.id ? -1 : 1;
  return Number(left.right) - Number(right.right);
}

function sideKey(id: bigint, right: boolean): string {
  return `${id}:${Number(right)}`;
}

function flattenNodes(
  entries: ReadonlyArray<readonly [bigint, Uint8Array]>,
): NodeTable {
  const totalSequenceBytes = entries.reduce(
    (total, [, sequence]) => total + sequence.byteLength,
    0,
  );
  if (totalSequenceBytes > 0xffff_ffff) {
    throw corrupt("selected node sequences exceed Uint32 offsets");
  }
  const ids = new BigUint64Array(entries.length);
  const sequenceOffsets = new Uint32Array(entries.length + 1);
  const sequenceBytes = new Uint8Array(totalSequenceBytes);
  let position = 0;
  entries.forEach(([id, sequence], index) => {
    ids[index] = id;
    sequenceOffsets[index] = position;
    sequenceBytes.set(sequence, position);
    position += sequence.byteLength;
  });
  sequenceOffsets[entries.length] = position;
  return { ids, sequenceOffsets, sequenceBytes };
}

function edgeKey(from: bigint, to: bigint): string {
  return `${from}:${to}`;
}

/** Mirrors the Rust v1 graph merge and context-selection state machine. */
export function assembleCanonicalGraph(
  tiles: readonly RegionTile[],
  query: RegionQuery,
): RegionGraph {
  if (tiles.length === 0) throw corrupt("query selected no tiles");
  const nodes = new Map<bigint, Uint8Array>();
  const edges = new Map<string, readonly [bigint, bigint]>();
  const visits = new Map<number, { end: number; handle: bigint }>();

  for (const tile of tiles) {
    if (
      tile.reference.sample !== query.sample ||
      tile.reference.contig !== query.contig
    ) {
      throw corrupt("tile reference identity differs from the query");
    }
    for (let index = 0; index < tile.nodeIds.length; index += 1) {
      const id = tile.nodeIds[index] as bigint;
      const sequence = sequenceAt(tile, index);
      const existing = nodes.get(id);
      if (existing !== undefined && !equalBytes(existing, sequence)) {
        throw corrupt(`conflicting sequence for node ${id}`);
      }
      if (existing === undefined) nodes.set(id, sequence.slice());
    }
    if (tile.edges.length % 2 !== 0) {
      throw corrupt("tile edge table has an odd handle count");
    }
    for (let index = 0; index < tile.edges.length; index += 2) {
      const from = tile.edges[index] as bigint;
      const to = tile.edges[index + 1] as bigint;
      edges.set(edgeKey(from, to), [from, to]);
    }

    let coordinate = tile.reference.start;
    for (const handle of tile.referenceTraversal) {
      const id = handle / 2n;
      const sequence = nodes.get(id);
      if (sequence === undefined) {
        throw corrupt(`reference traversal uses absent node ${id}`);
      }
      const end = coordinate + sequence.byteLength;
      if (!Number.isSafeInteger(end)) {
        throw corrupt("reference coordinate exceeds safe integer range");
      }
      const existing = visits.get(coordinate);
      if (
        existing !== undefined &&
        (existing.end !== end || existing.handle !== handle)
      ) {
        throw corrupt("conflicting reference visits at one coordinate");
      }
      visits.set(coordinate, { end, handle });
      coordinate = end;
    }
    if (coordinate !== tile.reference.end) {
      throw corrupt("reference traversal length differs from its interval");
    }
  }

  const adjacency = new Map<bigint, Set<bigint>>();
  const addSuccessor = (from: bigint, to: bigint): void => {
    const values = adjacency.get(from) ?? new Set<bigint>();
    values.add(to);
    adjacency.set(from, values);
  };
  for (const [from, to] of edges.values()) {
    if (nodes.has(from / 2n) && nodes.has(to / 2n)) {
      addSuccessor(from, to);
      addSuccessor(flip(to), flip(from));
    }
  }

  const orderedVisits = [...visits.entries()]
    .map(([start, value]) => ({ start, ...value }))
    .sort((left, right) => left.start - right.start);
  const active = new MinHeap();
  orderedVisits.forEach(({ start, end, handle }) => {
    if (start >= query.end || end <= query.start) return;
    const overlapStart = Math.max(start, query.start);
    const overlapEnd = Math.min(end, query.end);
    const reverse = handle % 2n === 1n;
    active.push({
      distance: overlapStart - start,
      id: handle / 2n,
      right: reverse,
    });
    active.push({
      distance: overlapEnd === end ? 0 : end - overlapEnd - 1,
      id: handle / 2n,
      right: !reverse,
    });
  });
  if (active.length === 0)
    throw corrupt("reference walk does not overlap query");

  const context = query.context ?? 100;
  const selected = new Set<bigint>();
  const visitedSides = new Set<string>();
  while (active.length > 0) {
    const { distance, id, right } = active.pop();
    const key = sideKey(id, right);
    if (visitedSides.has(key)) continue;
    visitedSides.add(key);
    selected.add(id);
    const otherKey = sideKey(id, !right);
    if (!visitedSides.has(otherKey)) {
      const sequence = nodes.get(id);
      if (sequence === undefined) throw corrupt(`missing node ${id}`);
      const nextDistance = distance + Math.max(0, sequence.byteLength - 1);
      if (nextDistance <= context) {
        active.push({ distance: nextDistance, id, right: !right });
      }
    }
    const edgeDistance = distance + 1;
    if (edgeDistance <= context) {
      const exitHandle = id * 2n + BigInt(!right);
      for (const successor of adjacency.get(exitHandle) ?? []) {
        active.push({
          distance: edgeDistance,
          id: successor / 2n,
          right: successor % 2n === 1n,
        });
      }
    }
  }

  const selectedNodes = [...nodes.entries()]
    .filter(([id]) => selected.has(id))
    .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
  const selectedEdges = [...edges.values()]
    .filter(([from, to]) => selected.has(from / 2n) && selected.has(to / 2n))
    .sort(([leftFrom, leftTo], [rightFrom, rightTo]) =>
      leftFrom !== rightFrom
        ? leftFrom < rightFrom
          ? -1
          : 1
        : leftTo < rightTo
          ? -1
          : leftTo > rightTo
            ? 1
            : 0,
    );
  const from = new BigUint64Array(selectedEdges.length);
  const to = new BigUint64Array(selectedEdges.length);
  selectedEdges.forEach(([edgeFrom, edgeTo], index) => {
    from[index] = edgeFrom;
    to[index] = edgeTo;
  });

  const firstCore = orderedVisits.findIndex(
    ({ start, end }) => start < query.end && end > query.start,
  );
  let lastCore = -1;
  for (let index = orderedVisits.length - 1; index >= 0; index -= 1) {
    const visit = orderedVisits[index];
    if (
      visit !== undefined &&
      visit.start < query.end &&
      visit.end > query.start
    ) {
      lastCore = index;
      break;
    }
  }
  if (firstCore < 0 || lastCore < 0) throw corrupt("reference core is absent");
  let first = firstCore;
  while (
    first > 0 &&
    selected.has((orderedVisits[first - 1] as { handle: bigint }).handle / 2n)
  ) {
    first -= 1;
  }
  let last = lastCore;
  while (
    last + 1 < orderedVisits.length &&
    selected.has((orderedVisits[last + 1] as { handle: bigint }).handle / 2n)
  ) {
    last += 1;
  }
  const referenceTraversal = BigUint64Array.from(
    orderedVisits.slice(first, last + 1).map(({ handle }) => handle),
  );
  const reference: ReferenceDescriptor = {
    sample: query.sample,
    contig: query.contig,
    start: query.start,
    end: query.end,
    fragment: query.start,
    orientation: "forward",
  };
  const edgeTable: EdgeTable = { from, to };
  return {
    reference,
    nodes: flattenNodes(selectedNodes),
    edges: edgeTable,
    referenceTraversal,
  };
}

function putU64(hasher: ReturnType<typeof blake3.create>, value: bigint): void {
  if (value < 0n || value > 0xffff_ffff_ffff_ffffn) {
    throw corrupt("canonical integer is outside u64");
  }
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  hasher.update(bytes);
}

function putBytes(
  hasher: ReturnType<typeof blake3.create>,
  bytes: Uint8Array,
): void {
  putU64(hasher, BigInt(bytes.byteLength));
  hasher.update(bytes);
}

function putOriented(
  hasher: ReturnType<typeof blake3.create>,
  handle: bigint,
): void {
  const node = unpack(handle);
  putU64(hasher, node.id);
  hasher.update(Uint8Array.of(Number(node.reverse)));
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

/** Stable BLAKE3 digest shared with CanonicalSubgraph::canonical_hash in Rust. */
export function canonicalGraphHash(graph: RegionGraph): string {
  const hasher = blake3.create();
  hasher.update(
    textEncoder.encode("pangenome-range canonical query graph v1\0"),
  );
  putU64(hasher, BigInt(graph.nodes.ids.length));
  for (let index = 0; index < graph.nodes.ids.length; index += 1) {
    putU64(hasher, graph.nodes.ids[index] as bigint);
    const start = graph.nodes.sequenceOffsets[index] as number;
    const end = graph.nodes.sequenceOffsets[index + 1] as number;
    putBytes(hasher, graph.nodes.sequenceBytes.subarray(start, end));
  }
  putU64(hasher, BigInt(graph.edges.from.length));
  for (let index = 0; index < graph.edges.from.length; index += 1) {
    putOriented(hasher, graph.edges.from[index] as bigint);
    putOriented(hasher, graph.edges.to[index] as bigint);
  }
  putU64(hasher, 1n);
  putBytes(hasher, textEncoder.encode(graph.reference.sample));
  putBytes(hasher, textEncoder.encode(graph.reference.contig));
  putU64(hasher, 0n);
  putU64(hasher, BigInt(graph.reference.fragment ?? graph.reference.start));
  hasher.update(Uint8Array.of(1));
  putU64(hasher, BigInt(graph.referenceTraversal.length));
  for (const handle of graph.referenceTraversal) putOriented(hasher, handle);
  putU64(hasher, 1n);
  putBytes(hasher, textEncoder.encode(graph.reference.sample));
  putBytes(hasher, textEncoder.encode(graph.reference.contig));
  putU64(hasher, BigInt(graph.reference.start));
  putU64(hasher, BigInt(graph.reference.end));
  return hex(hasher.digest());
}
