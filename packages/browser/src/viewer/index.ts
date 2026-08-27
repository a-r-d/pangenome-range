export type {
  ViewerComplexityBudgets,
  ViewerDisplayMode,
  ViewerLodDecision,
} from "./lod.js";
export {
  chooseViewerLod,
  DEFAULT_COMPLEXITY_BUDGETS,
  recommendedSummaryBins,
} from "./lod.js";
export type { GenomicCommand } from "./navigation.js";
export {
  formatGenomicCoordinate,
  parseGenomicCommand,
} from "./navigation.js";
export type {
  PangenomeViewer,
  PangenomeViewerOptions,
  ViewerBudgets,
  ViewerCounts,
  ViewerErrorDetail,
  ViewerEventMap,
  ViewerLayerState,
  ViewerPerformanceSnapshot,
  ViewerProgress,
  ViewerSelectionDetail,
  ViewerSnapshot,
  ViewerTheme,
  ViewerViewportChange,
} from "./types.js";

import { createViewerController } from "./controller.js";
import type { PangenomeViewer, PangenomeViewerOptions } from "./types.js";

export function createPangenomeViewer(
  container: HTMLElement,
  options: PangenomeViewerOptions,
): PangenomeViewer {
  return createViewerController(container, options);
}
