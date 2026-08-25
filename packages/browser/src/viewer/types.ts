import type { RegionResult, RegionTile } from "../reader/types.js";

export interface PangenomeViewerOptions {
  canvas: HTMLCanvasElement;
  maxNodes?: number;
  maxEdges?: number;
}

export interface PangenomeViewer {
  render(result: RegionResult): void;
  appendTile(tile: RegionTile): void;
  destroy(): void;
}
