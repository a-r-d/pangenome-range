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

/** Mirrors the Rust v4 graph merge and context-selection state machine. */
export function assembleCanonicalGraph(
  tiles: readonly RegionTile[],
  query: RegionQuery,
): RegionGraph {
  if (tiles.length === 0) throw corrupt("query selected no tiles");
  if (tiles[0]?.semantics === "named-paths-v3") {
    return assembleNamedCanonicalGraph(tiles, query);
  }
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

interface MergedNamedPath {
  sample: string;
  contig: string;
  haplotype: bigint;
  fragment: bigint;
  isReference: boolean;
  visits: Map<bigint, bigint>;
}

interface CanonicalNamedSegment {
  sample: string;
  contig: string;
  haplotype: bigint;
  fragment: bigint;
  isReference: boolean;
  traversal: bigint[];
}

function compareNamedSegments(
  left: CanonicalNamedSegment,
  right: CanonicalNamedSegment,
): number {
  const text =
    left.sample.localeCompare(right.sample) ||
    left.contig.localeCompare(right.contig);
  if (text !== 0) return text;
  if (left.haplotype !== right.haplotype)
    return left.haplotype < right.haplotype ? -1 : 1;
  if (left.fragment !== right.fragment)
    return left.fragment < right.fragment ? -1 : 1;
  if (left.isReference !== right.isReference)
    return Number(left.isReference) - Number(right.isReference);
  const length = Math.min(left.traversal.length, right.traversal.length);
  for (let index = 0; index < length; index += 1) {
    const leftNode = left.traversal[index] as bigint;
    const rightNode = right.traversal[index] as bigint;
    if (leftNode !== rightNode) return leftNode < rightNode ? -1 : 1;
  }
  return left.traversal.length - right.traversal.length;
}

function assembleNamedCanonicalGraph(
  tiles: readonly RegionTile[],
  query: RegionQuery,
): RegionGraph {
  const nodes = new Map<bigint, Uint8Array>();
  const edges = new Map<string, readonly [bigint, bigint]>();
  const paths = new Map<bigint, MergedNamedPath>();
  const referenceVisits = new Map<
    string,
    { pathId: bigint; start: number; end: number; node: bigint }
  >();
  for (const tile of tiles) {
    if (tile.semantics !== "named-paths-v3") {
      throw corrupt("cannot merge named and anonymous tiles");
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
    for (let index = 0; index < tile.edges.length; index += 2) {
      const from = tile.edges[index] as bigint;
      const to = tile.edges[index + 1] as bigint;
      edges.set(edgeKey(from, to), [from, to]);
    }
    if (tile.haplotypes.kind !== "named-paths") {
      throw corrupt("named tile has no named-path table");
    }
    const table = tile.haplotypes;
    for (let index = 0; index < table.pathIds.length; index += 1) {
      const pathId = table.pathIds[index] as bigint;
      const sample = table.samples[table.sampleIds[index] as number];
      const contig = table.contigs[table.contigIds[index] as number];
      if (sample === undefined || contig === undefined) {
        throw corrupt("named path dictionary index is out of bounds");
      }
      const metadata = {
        sample,
        contig,
        haplotype: table.haplotypes[index] as bigint,
        fragment: table.fragments[index] as bigint,
        isReference: table.referenceFlags[index] === 1,
      };
      const path = paths.get(pathId) ?? { ...metadata, visits: new Map() };
      if (
        path.sample !== metadata.sample ||
        path.contig !== metadata.contig ||
        path.haplotype !== metadata.haplotype ||
        path.fragment !== metadata.fragment ||
        path.isReference !== metadata.isReference
      ) {
        throw corrupt(`conflicting metadata for named path ${pathId}`);
      }
      const start = table.visitOffsets[index] as number;
      const end = table.visitOffsets[index + 1] as number;
      for (let visit = start; visit < end; visit += 1) {
        const visitIndex = table.visitIndices[visit] as bigint;
        const node = table.orientedNodes[visit] as bigint;
        const existing = path.visits.get(visitIndex);
        if (existing !== undefined && existing !== node) {
          throw corrupt(`conflicting visit ${visitIndex} for path ${pathId}`);
        }
        path.visits.set(visitIndex, node);
      }
      paths.set(pathId, path);
    }
    for (
      let index = 0;
      index < table.referenceVisitPathIds.length;
      index += 1
    ) {
      const pathId = table.referenceVisitPathIds[index] as bigint;
      const visitIndex = table.referenceVisitIndices[index] as bigint;
      const start = safeCoordinate(
        table.referenceVisitStarts[index] as bigint,
        "named reference start",
      );
      const end = safeCoordinate(
        table.referenceVisitEnds[index] as bigint,
        "named reference end",
      );
      const node = table.referenceVisitNodes[index] as bigint;
      referenceVisits.set(`${pathId}:${visitIndex}:${start}:${end}:${node}`, {
        pathId,
        start,
        end,
        node,
      });
    }
  }
  const referencePathIds = new Set(
    [...paths.entries()]
      .filter(
        ([, path]) =>
          path.sample === query.sample && path.contig === query.contig,
      )
      .map(([pathId]) => pathId),
  );
  if (referencePathIds.size === 0) {
    throw corrupt("named query reference is absent from fetched chunks");
  }
  const active = new MinHeap();
  for (const visit of referenceVisits.values()) {
    if (
      !referencePathIds.has(visit.pathId) ||
      visit.start >= query.end ||
      visit.end <= query.start
    ) {
      continue;
    }
    const reverse = visit.node % 2n === 1n;
    active.push({
      distance: Math.max(visit.start, query.start) - visit.start,
      id: visit.node / 2n,
      right: reverse,
    });
    const overlapEnd = Math.min(visit.end, query.end);
    active.push({
      distance: overlapEnd === visit.end ? 0 : visit.end - overlapEnd - 1,
      id: visit.node / 2n,
      right: !reverse,
    });
  }
  if (active.length === 0)
    throw corrupt("named reference visits do not overlap query");
  const adjacency = new Map<bigint, Set<bigint>>();
  const add = (from: bigint, to: bigint): void => {
    const successors = adjacency.get(from) ?? new Set<bigint>();
    successors.add(to);
    adjacency.set(from, successors);
  };
  for (const [from, to] of edges.values()) {
    if (nodes.has(from / 2n) && nodes.has(to / 2n)) {
      add(from, to);
      add(flip(to), flip(from));
    }
  }
  const selected = new Set<bigint>();
  const visitedSides = new Set<string>();
  const context = query.context ?? 100;
  while (active.length > 0) {
    const item = active.pop();
    const key = sideKey(item.id, item.right);
    if (visitedSides.has(key)) continue;
    visitedSides.add(key);
    selected.add(item.id);
    if (!visitedSides.has(sideKey(item.id, !item.right))) {
      const sequence = nodes.get(item.id);
      if (sequence === undefined)
        throw corrupt(`missing named node ${item.id}`);
      const distance = item.distance + Math.max(0, sequence.byteLength - 1);
      if (distance <= context) {
        active.push({ distance, id: item.id, right: !item.right });
      }
    }
    const edgeDistance = item.distance + 1;
    if (edgeDistance <= context) {
      const handle = item.id * 2n + BigInt(!item.right);
      for (const successor of adjacency.get(handle) ?? []) {
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
  const from = BigUint64Array.from(selectedEdges.map(([value]) => value));
  const to = BigUint64Array.from(selectedEdges.map(([, value]) => value));
  const segments: CanonicalNamedSegment[] = [];
  for (const path of paths.values()) {
    const ordered = [...path.visits.entries()].sort(([left], [right]) =>
      left < right ? -1 : left > right ? 1 : 0,
    );
    let segment: bigint[] = [];
    let previous: bigint | undefined;
    const push = (): void => {
      if (segment.length === 0) return;
      segments.push({
        sample: path.sample,
        contig: path.contig,
        haplotype: path.haplotype,
        fragment: path.fragment,
        isReference: path.isReference,
        traversal: segment,
      });
      segment = [];
    };
    for (const [index, node] of ordered) {
      if (!selected.has(node / 2n)) {
        push();
        previous = undefined;
        continue;
      }
      if (previous !== undefined && index !== previous + 1n) push();
      segment.push(node);
      previous = index;
    }
    push();
  }
  segments.sort(compareNamedSegments);
  const traversalCount = segments.reduce(
    (total, segment) => total + segment.traversal.length,
    0,
  );
  if (traversalCount > 0xffff_ffff) {
    throw corrupt("named canonical paths exceed Uint32 offsets");
  }
  const traversalOffsets = new Uint32Array(segments.length + 1);
  const orientedNodes = new BigUint64Array(traversalCount);
  let position = 0;
  segments.forEach((segment, index) => {
    traversalOffsets[index] = position;
    orientedNodes.set(segment.traversal, position);
    position += segment.traversal.length;
  });
  traversalOffsets[segments.length] = position;
  const referenceSegment = segments.find(
    (segment) =>
      segment.isReference &&
      segment.sample === query.sample &&
      segment.contig === query.contig,
  );
  return {
    reference: {
      sample: query.sample,
      contig: query.contig,
      start: query.start,
      end: query.end,
      fragment: query.start,
      orientation: "forward",
    },
    nodes: flattenNodes(selectedNodes),
    edges: { from, to },
    referenceTraversal: BigUint64Array.from(referenceSegment?.traversal ?? []),
    paths: {
      samples: segments.map(({ sample }) => sample),
      contigs: segments.map(({ contig }) => contig),
      haplotypes: BigUint64Array.from(
        segments.map(({ haplotype }) => haplotype),
      ),
      fragments: BigUint64Array.from(segments.map(({ fragment }) => fragment)),
      referenceFlags: Uint8Array.from(
        segments.map(({ isReference }) => Number(isReference)),
      ),
      traversalOffsets,
      orientedNodes,
    },
  };
}

function safeCoordinate(value: bigint, label: string): number {
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw corrupt(`${label} exceeds safe integer range`);
  }
  return Number(value);
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
    textEncoder.encode("pangenome-range canonical query graph v4\0"),
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
  if (graph.paths === undefined) {
    putU64(hasher, 1n);
    putBytes(hasher, textEncoder.encode(graph.reference.sample));
    putBytes(hasher, textEncoder.encode(graph.reference.contig));
    putU64(hasher, 0n);
    putU64(hasher, BigInt(graph.reference.fragment ?? graph.reference.start));
    hasher.update(Uint8Array.of(1));
    putU64(hasher, BigInt(graph.referenceTraversal.length));
    for (const handle of graph.referenceTraversal) putOriented(hasher, handle);
  } else {
    putU64(hasher, BigInt(graph.paths.samples.length));
    for (let index = 0; index < graph.paths.samples.length; index += 1) {
      putBytes(
        hasher,
        textEncoder.encode(graph.paths.samples[index] as string),
      );
      putBytes(
        hasher,
        textEncoder.encode(graph.paths.contigs[index] as string),
      );
      putU64(hasher, graph.paths.haplotypes[index] as bigint);
      putU64(hasher, graph.paths.fragments[index] as bigint);
      hasher.update(Uint8Array.of(graph.paths.referenceFlags[index] as number));
      const start = graph.paths.traversalOffsets[index] as number;
      const end = graph.paths.traversalOffsets[index + 1] as number;
      putU64(hasher, BigInt(end - start));
      for (let visit = start; visit < end; visit += 1) {
        putOriented(hasher, graph.paths.orientedNodes[visit] as bigint);
      }
    }
  }
  putU64(hasher, 1n);
  putBytes(hasher, textEncoder.encode(graph.reference.sample));
  putBytes(hasher, textEncoder.encode(graph.reference.contig));
  putU64(hasher, BigInt(graph.reference.start));
  putU64(hasher, BigInt(graph.reference.end));
  return hex(hasher.digest());
}
