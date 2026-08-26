import type {
  PangenomeArchive,
  QueryTrace,
  RegionQuery,
  RegionTile,
} from "../reader/types.js";

/** Hard limits applied before graph data is retained for layout. */
export interface ViewerBudgets {
  readonly maxRenderedNodes: number;
  readonly maxRenderedEdges: number;
  readonly maxHaplotypeLanes: number;
}

export interface ViewerCounts {
  readonly tiles: number;
  readonly decodedNodes: number;
  readonly decodedEdges: number;
  readonly decodedTraversals: number;
  readonly renderedNodes: number;
  readonly renderedEdges: number;
  readonly renderedHaplotypeLanes: number;
  readonly omittedNodes: number;
  readonly omittedEdges: number;
  readonly omittedTraversals: number;
}

export interface ViewerProgress {
  readonly region: Readonly<RegionQuery>;
  readonly counts: ViewerCounts;
  readonly summarized: boolean;
  readonly summary?: string;
}

export interface ViewerErrorDetail {
  readonly error: Error;
  readonly region?: Readonly<RegionQuery>;
}

export interface ViewerEventMap {
  readonly regionchange: Readonly<RegionQuery>;
  readonly progress: ViewerProgress;
  readonly querytrace: QueryTrace;
  readonly error: ViewerErrorDetail;
}

export interface PangenomeViewerOptions {
  readonly archive: PangenomeArchive;
  readonly initialRegion?: RegionQuery;
  readonly maxRenderedNodes?: number;
  readonly maxRenderedEdges?: number;
  readonly maxHaplotypeLanes?: number;
  readonly showRequestTrace?: boolean;
  readonly background?: string;
}

export interface ViewerSnapshot {
  readonly region?: Readonly<RegionQuery>;
  readonly counts: ViewerCounts;
  readonly loading: boolean;
  readonly zoom: number;
  readonly panX: number;
  readonly selectedNodeId?: bigint;
  readonly trace?: QueryTrace;
}

export interface PangenomeViewer {
  setRegion(region: RegionQuery): Promise<void>;
  getRegion(): Readonly<RegionQuery> | undefined;
  getSnapshot(): ViewerSnapshot;
  resize(): void;
  zoomBy(factor: number): void;
  panBy(pixels: number): void;
  resetView(): void;
  on<K extends keyof ViewerEventMap>(
    event: K,
    listener: (detail: ViewerEventMap[K]) => void,
  ): () => void;
  destroy(): void;
}

export interface ViewerTileBoundary {
  readonly coreStart: number;
  readonly coreEnd: number;
  readonly archiveOffset: bigint;
}

export interface ViewerModelNode {
  readonly id: bigint;
  readonly sequence: string;
  readonly sequenceLength: number;
  readonly reference: boolean;
  readonly reverse: boolean;
  readonly tileStart: number;
  readonly tileEnd: number;
}

export interface ViewerModelEdge {
  readonly from: bigint;
  readonly to: bigint;
  readonly fromReverse: boolean;
  readonly toReverse: boolean;
}

export interface ViewerModelTraversal {
  readonly tileStart: number;
  readonly tileEnd: number;
  readonly orientedNodes: readonly bigint[];
  readonly weight: bigint;
}

export interface ViewerModel {
  readonly query: Readonly<RegionQuery>;
  readonly budgets: ViewerBudgets;
  readonly nodes: ReadonlyMap<bigint, ViewerModelNode>;
  readonly edges: readonly ViewerModelEdge[];
  readonly referenceTraversal: readonly bigint[];
  readonly traversals: readonly ViewerModelTraversal[];
  readonly tileBoundaries: readonly ViewerTileBoundary[];
  readonly counts: ViewerCounts;
  readonly semantics: RegionTile["semantics"] | undefined;
}

export interface ViewerLayoutNode extends ViewerModelNode {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly visible: boolean;
}

export interface ViewerLayoutEdge extends ViewerModelEdge {
  readonly fromX: number;
  readonly fromY: number;
  readonly toX: number;
  readonly toY: number;
  readonly reference: boolean;
}

export interface ViewerLayoutTraversal {
  readonly points: readonly { readonly x: number; readonly y: number }[];
  readonly weight: bigint;
  readonly tileStart: number;
  readonly tileEnd: number;
  readonly lane: number;
}

export interface ViewerLayout {
  readonly width: number;
  readonly height: number;
  readonly query: Readonly<RegionQuery>;
  readonly nodes: readonly ViewerLayoutNode[];
  readonly edges: readonly ViewerLayoutEdge[];
  readonly traversals: readonly ViewerLayoutTraversal[];
  readonly tileBoundaries: readonly (ViewerTileBoundary & {
    readonly x: number;
  })[];
  readonly counts: ViewerCounts;
  readonly semantics: ViewerModel["semantics"];
  readonly zoom: number;
  readonly panX: number;
}

export interface ViewerLayoutOptions {
  readonly width: number;
  readonly height: number;
  readonly zoom?: number;
  readonly panX?: number;
}

export interface ProgressiveQueryCallbacks {
  readonly onStart?: (region: Readonly<RegionQuery>) => void;
  readonly onTile: (tile: RegionTile) => void;
  readonly onTrace?: (trace: QueryTrace) => void;
  readonly onComplete?: () => void;
}
