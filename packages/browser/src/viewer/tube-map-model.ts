import type {
  HaplotypeSemantics,
  RegionQuery,
  RegionTile,
  TileProvenance,
} from "../reader/types.js";

export interface OrientedNodeRef {
  readonly id: bigint;
  readonly reverse: boolean;
}

export interface TubeMapSourceTile {
  readonly key: string;
  readonly coreStart: number;
  readonly coreEnd: number;
  readonly archiveOffset: bigint;
  readonly compressedBytes: number;
  readonly uncompressedBytes: number;
}

export interface TubeMapNode {
  readonly key: string;
  readonly id: bigint;
  readonly sequence: string;
  readonly sequenceLength: number;
  readonly reverse: boolean;
  readonly reference: boolean;
  readonly sourceTile: TubeMapSourceTile;
  readonly collapsedMembers?: readonly bigint[];
  readonly collapsedBaseLength?: number;
}

export interface TubeMapEdge {
  readonly key: string;
  readonly from: string;
  readonly to: string;
  readonly fromReverse: boolean;
  readonly toReverse: boolean;
  readonly reference: boolean;
  readonly classification: "reference" | "alternate" | "deletion" | "inversion";
}

export interface LocalPattern {
  /** A tile-local display identifier such as T2-P1. */
  readonly id: string;
  readonly tileKey: string;
  readonly tileStart: number;
  readonly tileEnd: number;
  readonly weight: bigint;
  readonly orientedNodes: readonly bigint[];
  readonly nodeKeys: readonly string[];
  readonly source: TubeMapSourceTile;
}

export interface TubeMapTileBoundary {
  readonly tileKey: string;
  readonly start: number;
  readonly end: number;
}

export interface TubeMapCounts {
  readonly tiles: number;
  readonly decodedNodes: number;
  readonly decodedEdges: number;
  readonly decodedPatterns: number;
  readonly displayedNodeGroups: number;
  readonly displayedTopologyEdges: number;
  readonly displayedPatterns: number;
  readonly collapsedNodes: number;
  readonly omittedTopologyNodes: number;
  readonly omittedTopologyEdges: number;
}

export interface TubeMapModel {
  readonly query: Readonly<RegionQuery>;
  readonly nodes: readonly TubeMapNode[];
  readonly edges: readonly TubeMapEdge[];
  readonly reference: readonly string[];
  readonly patterns: readonly LocalPattern[];
  readonly tileBoundaries: readonly TubeMapTileBoundary[];
  readonly counts: TubeMapCounts;
  readonly semantics?: HaplotypeSemantics;
  readonly withinDisplayLimits: boolean;
  readonly displayLimitMessage?: string;
}

export interface TubeMapBuildOptions {
  readonly maxPatterns?: 0 | 4 | 8 | 16;
  readonly simplifyLinearChains?: boolean;
  readonly expandedNodeGroups?: readonly string[];
  readonly maxDisplayedNodeGroups?: number;
  readonly maxDisplayedTopologyEdges?: number;
}

export const DEFAULT_TUBE_MAP_BUILD_OPTIONS = {
  maxPatterns: 8,
  simplifyLinearChains: true,
  maxDisplayedNodeGroups: 2_500,
  maxDisplayedTopologyEdges: 5_000,
} as const;

/**
 * Explicit opt-in ceiling for unusually dense desktop views. The ceiling stays
 * finite because the current renderer creates an interactive SVG scene.
 */
export const EXTENDED_TUBE_MAP_DISPLAY_LIMITS = {
  maxDisplayedNodeGroups: 10_000,
  maxDisplayedTopologyEdges: 20_000,
} as const;

interface RawNode {
  readonly id: bigint;
  readonly sequence: string;
  readonly sourceTiles: TubeMapSourceTile[];
  readonly orientations: Set<boolean>;
  reference: boolean;
}

interface RawEdge {
  readonly from: bigint;
  readonly to: bigint;
  readonly fromReverse: boolean;
  readonly toReverse: boolean;
}

interface RawPattern {
  readonly tile: TubeMapSourceTile;
  readonly weight: bigint;
  readonly orientedNodes: readonly bigint[];
  readonly localRank: number;
}

const decoder = new TextDecoder();
const MAX_COLLAPSED_CHAIN_NODES = 64;

/**
 * Build the deterministic, display-bounded model consumed by the tube-map
 * renderer. Anonymous paths retain their source tile and are never joined.
 */
export function buildTubeMapModel(
  inputTiles: readonly RegionTile[],
  query: RegionQuery,
  options: TubeMapBuildOptions = {},
): TubeMapModel {
  const maxPatterns =
    options.maxPatterns ?? DEFAULT_TUBE_MAP_BUILD_OPTIONS.maxPatterns;
  const simplify =
    options.simplifyLinearChains ??
    DEFAULT_TUBE_MAP_BUILD_OPTIONS.simplifyLinearChains;
  const maxNodeGroups = positiveInteger(
    options.maxDisplayedNodeGroups,
    DEFAULT_TUBE_MAP_BUILD_OPTIONS.maxDisplayedNodeGroups,
  );
  const maxEdges = positiveInteger(
    options.maxDisplayedTopologyEdges,
    DEFAULT_TUBE_MAP_BUILD_OPTIONS.maxDisplayedTopologyEdges,
  );
  const expanded = new Set(options.expandedNodeGroups ?? []);
  const tiles = [...inputTiles].sort(compareTiles);
  const rawNodes = new Map<bigint, RawNode>();
  const rawEdges = new Map<string, RawEdge>();
  const rawPatterns: RawPattern[] = [];
  const referenceHandles: bigint[] = [];
  const referenceSeen = new Set<bigint>();
  let decodedNodes = 0;
  let decodedEdges = 0;
  let decodedPatterns = 0;

  for (const [tileIndex, tile] of tiles.entries()) {
    const source = sourceTile(tile, tileIndex);
    decodedNodes += tile.nodes.ids.length;
    decodedEdges += Math.min(
      tile.topology.from.length,
      tile.topology.to.length,
    );
    decodedPatterns += tile.haplotypes.weights.length;
    const referenceIds = new Set<bigint>();
    for (const handle of tile.referenceTraversal) {
      const id = nodeId(handle);
      referenceIds.add(id);
      if (!referenceSeen.has(id)) {
        referenceSeen.add(id);
        referenceHandles.push(handle);
      }
    }
    for (let index = 0; index < tile.nodes.ids.length; index += 1) {
      const id = tile.nodes.ids[index];
      const start = tile.nodes.sequenceOffsets[index];
      const end = tile.nodes.sequenceOffsets[index + 1];
      if (id === undefined || start === undefined || end === undefined)
        continue;
      const existing = rawNodes.get(id);
      if (existing === undefined) {
        rawNodes.set(id, {
          id,
          sequence: decoder.decode(
            tile.nodes.sequenceBytes.subarray(start, end),
          ),
          sourceTiles: [source],
          orientations: new Set(),
          reference: referenceIds.has(id),
        });
      } else {
        existing.reference ||= referenceIds.has(id);
        if (
          !existing.sourceTiles.some(
            (candidate) => candidate.key === source.key,
          )
        ) {
          existing.sourceTiles.push(source);
          existing.sourceTiles.sort(compareSources);
        }
      }
    }
    for (const handle of tile.referenceTraversal) {
      rawNodes.get(nodeId(handle))?.orientations.add(isReverse(handle));
    }
    const edgeCount = Math.min(
      tile.topology.from.length,
      tile.topology.to.length,
    );
    for (let index = 0; index < edgeCount; index += 1) {
      const fromHandle = tile.topology.from[index];
      const toHandle = tile.topology.to[index];
      if (fromHandle === undefined || toHandle === undefined) continue;
      const edge: RawEdge = {
        from: nodeId(fromHandle),
        to: nodeId(toHandle),
        fromReverse: isReverse(fromHandle),
        toReverse: isReverse(toHandle),
      };
      rawEdges.set(rawEdgeKey(edge), edge);
    }
    const tilePatterns: Omit<RawPattern, "localRank">[] = [];
    for (let index = 0; index < tile.haplotypes.weights.length; index += 1) {
      const start = tile.haplotypes.traversalOffsets[index];
      const end = tile.haplotypes.traversalOffsets[index + 1];
      const weight = tile.haplotypes.weights[index];
      if (start === undefined || end === undefined || weight === undefined)
        continue;
      tilePatterns.push({
        tile: source,
        weight,
        orientedNodes: Array.from(
          tile.haplotypes.orientedNodes.subarray(start, end),
        ),
      });
    }
    tilePatterns.sort(comparePatternShape);
    for (const [patternIndex, pattern] of tilePatterns.entries()) {
      rawPatterns.push({ ...pattern, localRank: patternIndex + 1 });
    }
  }

  const selectedPatterns = rawPatterns
    .sort(compareRawPatterns)
    .slice(0, maxPatterns);
  for (const pattern of selectedPatterns) {
    for (const handle of pattern.orientedNodes) {
      rawNodes.get(nodeId(handle))?.orientations.add(isReverse(handle));
    }
  }

  // The primary product displays the real reference plus selected local
  // traversal evidence. Remaining topology is counted and disclosed rather
  // than silently painted as a hairball.
  const retainedIds = new Set(referenceHandles.map(nodeId));
  for (const pattern of selectedPatterns) {
    for (const handle of pattern.orientedNodes) retainedIds.add(nodeId(handle));
  }
  const retainedNodes = new Map(
    [...rawNodes.entries()].filter(([id]) => retainedIds.has(id)),
  );
  const selectedPathEdges = new Set<string>();
  addPathEdges(selectedPathEdges, referenceHandles);
  for (const pattern of selectedPatterns) {
    addPathEdges(selectedPathEdges, pattern.orientedNodes);
  }
  const retainedEdges = [...rawEdges.values()].filter(
    (edge) =>
      retainedIds.has(edge.from) &&
      retainedIds.has(edge.to) &&
      selectedPathEdges.has(nodePairKey(edge.from, edge.to)),
  );
  const referenceIds = referenceHandles
    .map(nodeId)
    .filter((id) => retainedNodes.has(id));
  const groups = simplify
    ? buildCollapsedGroups(
        retainedNodes,
        referenceIds,
        selectedPatterns.map((pattern) => pattern.orientedNodes.map(nodeId)),
        expanded,
      )
    : [...retainedNodes.keys()].sort(compareBigints).map((id) => [id]);
  const groupByNode = new Map<bigint, string>();
  const nodes = groups.map((members) => {
    const key = nodeGroupKey(members);
    for (const member of members) groupByNode.set(member, key);
    return createTubeMapNode(key, members, retainedNodes);
  });
  const reference = dedupeAdjacent(
    referenceIds.flatMap((id) => {
      const key = groupByNode.get(id);
      return key === undefined ? [] : [key];
    }),
  );
  const referenceEdgeKeys = new Set<string>();
  for (let index = 1; index < reference.length; index += 1) {
    const from = reference[index - 1];
    const to = reference[index];
    if (from !== undefined && to !== undefined) {
      referenceEdgeKeys.add(`${from}->${to}`);
      referenceEdgeKeys.add(`${to}->${from}`);
    }
  }
  const edges = collapseEdges(
    retainedEdges,
    groupByNode,
    reference,
    referenceEdgeKeys,
  );
  const patterns: LocalPattern[] = selectedPatterns.map((pattern) => ({
    id: `${pattern.tile.key}-P${pattern.localRank}`,
    tileKey: pattern.tile.key,
    tileStart: pattern.tile.coreStart,
    tileEnd: pattern.tile.coreEnd,
    weight: pattern.weight,
    orientedNodes: pattern.orientedNodes,
    nodeKeys: dedupeAdjacent(
      pattern.orientedNodes.flatMap((handle) => {
        const key = groupByNode.get(nodeId(handle));
        return key === undefined ? [] : [key];
      }),
    ),
    source: pattern.tile,
  }));
  const collapsedNodes = nodes.reduce(
    (count, node) =>
      count + Math.max(0, (node.collapsedMembers?.length ?? 1) - 1),
    0,
  );
  const counts: TubeMapCounts = {
    tiles: tiles.length,
    decodedNodes,
    decodedEdges,
    decodedPatterns,
    displayedNodeGroups: nodes.length,
    displayedTopologyEdges: edges.length,
    displayedPatterns: patterns.length,
    collapsedNodes,
    omittedTopologyNodes: Math.max(0, rawNodes.size - retainedNodes.size),
    omittedTopologyEdges: Math.max(0, rawEdges.size - retainedEdges.length),
  };
  const violations: string[] = [];
  if (nodes.length > maxNodeGroups) {
    violations.push(`${nodes.length} node groups (limit ${maxNodeGroups})`);
  }
  if (edges.length > maxEdges) {
    violations.push(`${edges.length} topology edges (limit ${maxEdges})`);
  }
  return {
    query: copyQuery(query),
    nodes,
    edges,
    reference,
    patterns,
    tileBoundaries: tiles.map((tile, index) => ({
      tileKey: `T${index + 1}`,
      start: tile.coreStart,
      end: tile.coreEnd,
    })),
    counts,
    ...(tiles[0]?.semantics === undefined
      ? {}
      : { semantics: tiles[0].semantics }),
    withinDisplayLimits: violations.length === 0,
    ...(violations.length === 0
      ? {}
      : {
          displayLimitMessage: `This interval contains ${violations.join(
            ", ",
          )} ${simplify ? "after linear-chain simplification" : "with linear chains expanded"}. Rendering is paused to protect browser responsiveness; the graph data is intact.`,
        }),
  };
}

export function patternThickness(weight: bigint): number {
  if (weight <= 0n) return 2;
  const bits = weight.toString(2).length;
  return clamp(2 + bits - 1, 2, 9);
}

function buildCollapsedGroups(
  nodes: ReadonlyMap<bigint, RawNode>,
  reference: readonly bigint[],
  patterns: readonly (readonly bigint[])[],
  expanded: ReadonlySet<string>,
): bigint[][] {
  const groups: bigint[][] = [];
  const grouped = new Set<bigint>();
  addLinearRuns(reference, nodes, expanded, grouped, groups);
  const referenceGroupByNode = new Map<bigint, string>();
  for (const group of groups) {
    const key = nodeGroupKey(group);
    for (const id of group) referenceGroupByNode.set(id, key);
  }
  for (const pattern of patterns) {
    addPatternRuns(
      pattern,
      nodes,
      referenceGroupByNode,
      expanded,
      grouped,
      groups,
    );
  }
  for (const id of [...nodes.keys()].sort(compareBigints)) {
    if (!grouped.has(id)) {
      grouped.add(id);
      groups.push([id]);
    }
  }
  return groups.sort(compareGroupsByReference(reference));
}

function addPatternRuns(
  sequence: readonly bigint[],
  nodes: ReadonlyMap<bigint, RawNode>,
  referenceGroupByNode: ReadonlyMap<bigint, string>,
  expanded: ReadonlySet<string>,
  grouped: Set<bigint>,
  groups: bigint[][],
): void {
  let run: bigint[] = [];
  let referenceAnchor: string | undefined;
  const flush = (): void => {
    addChainOrIndividuals(run, expanded, grouped, groups);
    run = [];
  };
  for (const id of sequence) {
    const nextReferenceAnchor = referenceGroupByNode.get(id);
    if (nextReferenceAnchor !== undefined) {
      if (
        referenceAnchor !== undefined &&
        nextReferenceAnchor !== referenceAnchor
      ) {
        flush();
      }
      referenceAnchor = nextReferenceAnchor;
      continue;
    }
    if (grouped.has(id)) continue;
    const node = nodes.get(id);
    const previous = run.at(-1);
    if (
      node === undefined ||
      (previous !== undefined &&
        (primaryTile(nodes.get(previous)) !== primaryTile(node) ||
          orientation(nodes.get(previous)) !== orientation(node)))
    ) {
      flush();
    }
    if (node !== undefined && !grouped.has(id)) run.push(id);
  }
  flush();
}

function addLinearRuns(
  sequence: readonly bigint[],
  nodes: ReadonlyMap<bigint, RawNode>,
  expanded: ReadonlySet<string>,
  grouped: Set<bigint>,
  groups: bigint[][],
): void {
  let run: bigint[] = [];
  const flush = (): void => {
    addChainOrIndividuals(run, expanded, grouped, groups);
    run = [];
  };
  for (const id of sequence) {
    const node = nodes.get(id);
    const previous = run.at(-1);
    const compatible =
      node !== undefined &&
      !grouped.has(id) &&
      (previous === undefined ||
        (primaryTile(nodes.get(previous)) === primaryTile(node) &&
          orientation(nodes.get(previous)) === orientation(node)));
    if (!compatible) {
      flush();
      if (node !== undefined && !grouped.has(id)) run.push(id);
      continue;
    }
    run.push(id);
  }
  flush();
}

function addChainOrIndividuals(
  chain: readonly bigint[],
  expanded: ReadonlySet<string>,
  grouped: Set<bigint>,
  groups: bigint[][],
): void {
  if (chain.length === 0) return;
  const available = chain.filter((id) => !grouped.has(id));
  for (
    let offset = 0;
    offset < available.length;
    offset += MAX_COLLAPSED_CHAIN_NODES
  ) {
    const segment = available.slice(offset, offset + MAX_COLLAPSED_CHAIN_NODES);
    const key = nodeGroupKey(segment);
    if (segment.length < 2 || expanded.has(key)) {
      for (const id of segment) {
        if (grouped.has(id)) continue;
        grouped.add(id);
        groups.push([id]);
      }
      continue;
    }
    for (const id of segment) {
      if (grouped.has(id)) continue;
      grouped.add(id);
    }
    groups.push(segment);
  }
}

function createTubeMapNode(
  key: string,
  members: readonly bigint[],
  nodes: ReadonlyMap<bigint, RawNode>,
): TubeMapNode {
  const raw = members.flatMap((id) => {
    const node = nodes.get(id);
    return node === undefined ? [] : [node];
  });
  const first = raw[0];
  if (first === undefined) throw new Error("tube-map group contains no nodes");
  const sequence = raw.map((node) => node.sequence).join("");
  const memberIds = members.length > 1 ? [...members] : undefined;
  return {
    key,
    id: first.id,
    sequence,
    sequenceLength: sequence.length,
    reverse: orientation(first),
    reference: raw.some((node) => node.reference),
    sourceTile: first.sourceTiles[0] as TubeMapSourceTile,
    ...(memberIds === undefined
      ? {}
      : {
          collapsedMembers: memberIds,
          collapsedBaseLength: sequence.length,
        }),
  };
}

function collapseEdges(
  rawEdges: readonly RawEdge[],
  groupByNode: ReadonlyMap<bigint, string>,
  reference: readonly string[],
  referenceEdgeKeys: ReadonlySet<string>,
): TubeMapEdge[] {
  const referenceIndex = new Map(reference.map((key, index) => [key, index]));
  const result = new Map<string, TubeMapEdge>();
  for (const edge of rawEdges) {
    const from = groupByNode.get(edge.from);
    const to = groupByNode.get(edge.to);
    if (from === undefined || to === undefined || from === to) continue;
    const key = `${from}:${edge.fromReverse ? 1 : 0}->${to}:${edge.toReverse ? 1 : 0}`;
    if (result.has(key)) continue;
    const isReference = referenceEdgeKeys.has(`${from}->${to}`);
    const fromIndex = referenceIndex.get(from);
    const toIndex = referenceIndex.get(to);
    const classification: TubeMapEdge["classification"] = isReference
      ? "reference"
      : edge.fromReverse !== edge.toReverse
        ? "inversion"
        : fromIndex !== undefined &&
            toIndex !== undefined &&
            Math.abs(fromIndex - toIndex) > 1
          ? "deletion"
          : "alternate";
    result.set(key, {
      key,
      from,
      to,
      fromReverse: edge.fromReverse,
      toReverse: edge.toReverse,
      reference: isReference,
      classification,
    });
  }
  return [...result.values()].sort((left, right) =>
    left.key.localeCompare(right.key),
  );
}

function compareGroupsByReference(reference: readonly bigint[]) {
  const order = new Map(reference.map((id, index) => [id, index]));
  return (left: readonly bigint[], right: readonly bigint[]): number => {
    const leftOrder = Math.min(
      ...left.map((id) => order.get(id) ?? Number.MAX_SAFE_INTEGER),
    );
    const rightOrder = Math.min(
      ...right.map((id) => order.get(id) ?? Number.MAX_SAFE_INTEGER),
    );
    return (
      leftOrder - rightOrder || compareBigints(left[0] ?? 0n, right[0] ?? 0n)
    );
  };
}

function compareTiles(left: RegionTile, right: RegionTile): number {
  return (
    left.coreStart - right.coreStart ||
    left.coreEnd - right.coreEnd ||
    compareBigints(
      left.provenance.archiveOffset,
      right.provenance.archiveOffset,
    )
  );
}

function compareSources(
  left: TubeMapSourceTile,
  right: TubeMapSourceTile,
): number {
  return (
    left.coreStart - right.coreStart ||
    left.coreEnd - right.coreEnd ||
    left.key.localeCompare(right.key)
  );
}

function comparePatternShape(
  left: { readonly weight: bigint; readonly orientedNodes: readonly bigint[] },
  right: { readonly weight: bigint; readonly orientedNodes: readonly bigint[] },
): number {
  if (left.weight !== right.weight) return left.weight > right.weight ? -1 : 1;
  return compareBigintArrays(left.orientedNodes, right.orientedNodes);
}

function compareRawPatterns(left: RawPattern, right: RawPattern): number {
  if (left.weight !== right.weight) return left.weight > right.weight ? -1 : 1;
  return (
    left.tile.coreStart - right.tile.coreStart ||
    compareBigintArrays(left.orientedNodes, right.orientedNodes) ||
    left.tile.key.localeCompare(right.tile.key)
  );
}

function compareBigintArrays(
  left: readonly bigint[],
  right: readonly bigint[],
): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const comparison = compareBigints(left[index] ?? 0n, right[index] ?? 0n);
    if (comparison !== 0) return comparison;
  }
  return left.length - right.length;
}

function sourceTile(tile: RegionTile, tileIndex: number): TubeMapSourceTile {
  return {
    key: `T${tileIndex + 1}`,
    coreStart: tile.coreStart,
    coreEnd: tile.coreEnd,
    ...copyProvenance(tile.provenance),
  };
}

function copyProvenance(provenance: TileProvenance) {
  return {
    archiveOffset: provenance.archiveOffset,
    compressedBytes: provenance.compressedBytes,
    uncompressedBytes: provenance.uncompressedBytes,
  };
}

function rawEdgeKey(edge: RawEdge): string {
  return `${edge.from}:${edge.fromReverse ? 1 : 0}->${edge.to}:${edge.toReverse ? 1 : 0}`;
}

function addPathEdges(edges: Set<string>, handles: readonly bigint[]): void {
  for (let index = 1; index < handles.length; index += 1) {
    const from = handles[index - 1];
    const to = handles[index];
    if (from !== undefined && to !== undefined) {
      edges.add(nodePairKey(nodeId(from), nodeId(to)));
    }
  }
}

function nodePairKey(from: bigint, to: bigint): string {
  return `${from}->${to}`;
}

function nodeGroupKey(members: readonly bigint[]): string {
  return members.length === 1
    ? `n:${members[0]?.toString() ?? "0"}`
    : `c:${members.map((id) => id.toString()).join(",")}`;
}

function primaryTile(node: RawNode | undefined): string {
  return node?.sourceTiles[0]?.key ?? "";
}

function orientation(node: RawNode | undefined): boolean {
  return node?.orientations.size === 1 && node.orientations.has(true);
}

function nodeId(handle: bigint): bigint {
  return handle >> 1n;
}

function isReverse(handle: bigint): boolean {
  return (handle & 1n) === 1n;
}

function copyQuery(query: RegionQuery): Readonly<RegionQuery> {
  return {
    sample: query.sample,
    contig: query.contig,
    start: query.start,
    end: query.end,
    ...(query.context === undefined ? {} : { context: query.context }),
  };
}

function dedupeAdjacent<T>(values: readonly T[]): T[] {
  const result: T[] = [];
  for (const value of values) {
    if (result.at(-1) !== value) result.push(value);
  }
  return result;
}

function positiveInteger(value: number | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError(
      `tube-map limits must be positive safe integers; got ${value}`,
    );
  }
  return value;
}

function compareBigints(left: bigint, right: bigint): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}
