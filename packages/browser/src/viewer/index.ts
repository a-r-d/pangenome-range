export type {
  PangenomeViewer,
  PangenomeViewerOptions,
  ViewerBudgets,
  ViewerCounts,
  ViewerErrorDetail,
  ViewerEventMap,
  ViewerProgress,
  ViewerSnapshot,
} from "./types.js";

import { createViewerController } from "./controller.js";
import type { PangenomeViewer, PangenomeViewerOptions } from "./types.js";

export function createPangenomeViewer(
  container: HTMLElement,
  options: PangenomeViewerOptions,
): PangenomeViewer {
  return createViewerController(container, options);
}
