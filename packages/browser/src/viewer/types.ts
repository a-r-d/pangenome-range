import type {
  PangenomeArchive,
  QueryTrace,
  RegionQuery,
  RegionTile,
} from "../reader/types.js";
import type { ViewerDisplayMode } from "./lod.js";

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

export interface ViewerLayerState {
  readonly reference: boolean;
  readonly topology: boolean;
  readonly traversals: boolean;
  readonly tileBoundaries: boolean;
  readonly sequenceLabels: boolean;
}

export type ViewerTheme = "light" | "dark";

export interface ViewerViewportChange {
  readonly visualRegion: Readonly<RegionQuery>;
  readonly source: "pointer" | "keyboard" | "api";
}

export type ViewerSelectionDetail =
  | {
      readonly kind: "node";
      readonly node: ViewerLayoutNode;
      readonly incoming: readonly ViewerLayoutEdge[];
      readonly outgoing: readonly ViewerLayoutEdge[];
      readonly localTraversalWeights: readonly {
        readonly tileStart: number;
        readonly tileEnd: number;
        readonly weight: bigint;
      }[];
    }
  | { readonly kind: "edge"; readonly edge: ViewerLayoutEdge }
  | {
      readonly kind: "traversal";
      readonly traversal: ViewerLayoutTraversal;
    };

export interface ViewerPerformanceSnapshot {
  readonly modelUpdateMs: number;
  readonly layoutMs: number;
  readonly paintMs: number;
  readonly firstTilePaintMs?: number;
  readonly queryCompleteMs?: number;
  readonly frameP95Ms: number;
  readonly sampledFrames: number;
}

export interface ViewerEventMap {
  readonly regionchange: Readonly<RegionQuery>;
  readonly progress: ViewerProgress;
  readonly querytrace: QueryTrace;
  readonly error: ViewerErrorDetail;
  readonly viewportchange: ViewerViewportChange;
  readonly selectionchange: ViewerSelectionDetail | undefined;
  readonly lodchange: ViewerDisplayMode;
}

export interface PangenomeViewerOptions {
  readonly archive: PangenomeArchive;
  readonly initialRegion?: RegionQuery;
  readonly maxRenderedNodes?: number;
  readonly maxRenderedEdges?: number;
  readonly maxHaplotypeLanes?: number;
  readonly showRequestTrace?: boolean;
  readonly background?: string;
  readonly initialDisplayMode?: ViewerDisplayMode;
  readonly initialLayers?: Partial<ViewerLayerState>;
  readonly initialTheme?: ViewerTheme;
}

export interface ViewerSnapshot {
  readonly region?: Readonly<RegionQuery>;
  readonly counts: ViewerCounts;
  readonly loading: boolean;
  readonly zoom: number;
  readonly panX: number;
  readonly selectedNodeId?: bigint;
  readonly trace?: QueryTrace;
  readonly displayMode: ViewerDisplayMode;
  readonly layers: ViewerLayerState;
  readonly theme: ViewerTheme;
}

export interface PangenomeViewer {
  setRegion(region: RegionQuery): Promise<void>;
  setViewport(region: RegionQuery): Promise<void>;
  getRegion(): Readonly<RegionQuery> | undefined;
  getSnapshot(): ViewerSnapshot;
  resize(): void;
  zoomBy(factor: number): void;
  panBy(pixels: number): void;
  resetView(): void;
  setDisplayMode(mode: ViewerDisplayMode): void;
  setLayers(layers: Partial<ViewerLayerState>): void;
  setTheme(theme: ViewerTheme): void;
  getPerformanceSnapshot(): ViewerPerformanceSnapshot;
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
  readonly sourceTiles: readonly ViewerNodeSource[];
}

export interface ViewerNodeSource {
  readonly coreStart: number;
  readonly coreEnd: number;
  readonly archiveOffset: bigint;
  readonly compressedBytes: number;
  readonly uncompressedBytes: number;
}

export interface ViewerModelEdge {
  readonly from: bigint;
  readonly to: bigint;
  readonly fromReverse: boolean;
  readonly toReverse: boolean;
  readonly sourceTiles: readonly ViewerNodeSource[];
}

export interface ViewerModelTraversal {
  readonly tileStart: number;
  readonly tileEnd: number;
  readonly orientedNodes: readonly bigint[];
  readonly weight: bigint;
  readonly source: ViewerNodeSource;
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
  readonly lane: number;
  readonly branchKind:
    | "reference"
    | "alternate"
    | "insertion"
    | "inversion"
    | "unanchored";
  readonly anchorStart: number;
  readonly anchorEnd: number;
}

export interface ViewerLayoutEdge extends ViewerModelEdge {
  readonly fromX: number;
  readonly fromY: number;
  readonly toX: number;
  readonly toY: number;
  readonly reference: boolean;
  readonly classification: "reference" | "alternate" | "deletion" | "inversion";
}

export interface ViewerLayoutTraversal {
  readonly points: readonly { readonly x: number; readonly y: number }[];
  readonly orientedNodes: readonly bigint[];
  readonly weight: bigint;
  readonly tileStart: number;
  readonly tileEnd: number;
  readonly lane: number;
  readonly source: ViewerNodeSource;
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
