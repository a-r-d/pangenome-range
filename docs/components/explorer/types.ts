import type { LocusHit, OverviewBin } from "pangenome-range/reader";
import type { ViewerSelectionDetail } from "pangenome-range/viewer";

export type ArchiveChoice =
  | "fixture"
  | "configured"
  | "population"
  | "custom"
  | "local";

export type ExplorerPhase =
  | "idle"
  | "opening"
  | "summary"
  | "graph"
  | "ready"
  | "error";

export type ExplorerScreen = "showcase" | "explorer";

export type SearchState =
  | "index-absent"
  | "index-empty"
  | "ready"
  | "searching"
  | "no-matches"
  | "results"
  | "truncated"
  | "failed";

export type SummaryMetric =
  | "coveredBases"
  | "tileCount"
  | "encodedBytes"
  | "decodedBytes"
  | "nodeRecords"
  | "edgeRecords"
  | "gbwtRecords"
  | "occurrences";

export type SummaryScale = "linear" | "log" | "normalized";

export type InspectorSelection =
  | { kind: "archive" }
  | ViewerSelectionDetail
  | { kind: "summary"; bin: OverviewBin }
  | { kind: "locus"; hit: LocusHit; matchedAlias: boolean };
