import type {
  ArchiveInfo,
  LocusHit,
  QueryTrace,
  RegionPlan,
  RegionQuery,
} from "pangenome-range/reader";
import type {
  LocalPattern,
  TubeMapModel,
  TubeMapNode,
} from "pangenome-range/viewer";

export type BrowserPhase =
  | "opening"
  | "planning"
  | "streaming"
  | "ready"
  | "error";

export interface GraphOptions {
  patternCount: 0 | 4 | 8 | 16;
  simplifyLinearChains: boolean;
  showBases: "automatic" | "on" | "off";
  showTileBoundaries: boolean;
}

export interface GraphViewport {
  readonly zoom: number;
  /** Normalized horizontal position placed at the viewport center. */
  readonly center: number;
  readonly verticalScale: number;
}

export type DemoArchiveId =
  | "hprc"
  | "1000g"
  | "rice"
  | "fixture"
  | "custom"
  | "local";

export type BrowserSelection =
  | { readonly kind: "node"; readonly node: TubeMapNode }
  | { readonly kind: "pattern"; readonly pattern: LocalPattern };

export interface BrowserMetrics {
  readonly openMs?: number;
  readonly firstTileMs?: number;
  readonly completeMs?: number;
  readonly layoutMs?: number;
  readonly svgElements?: number;
}

export interface BrowserStateSnapshot {
  readonly phase: BrowserPhase;
  readonly info?: ArchiveInfo;
  readonly region?: RegionQuery;
  readonly locus?: LocusHit;
  readonly plan?: RegionPlan;
  readonly model?: TubeMapModel;
  readonly trace?: QueryTrace;
  readonly metrics: BrowserMetrics;
}

export interface ArchiveSourceSelection {
  readonly id: DemoArchiveId;
  readonly source: string | File;
  readonly label: string;
  readonly key: string;
  readonly description?: string;
}
