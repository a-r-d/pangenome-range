import type { PangenomeArchive, RegionQuery } from "../reader/types.js";
import type { ProgressiveQueryCallbacks } from "./types.js";

/** Owns exactly one progressive tile query and makes stale callbacks impossible. */
export class ProgressiveTileQuery {
  readonly #archive: PangenomeArchive;
  #controller: AbortController | undefined;
  #generation = 0;
  #destroyed = false;

  constructor(archive: PangenomeArchive) {
    this.#archive = archive;
  }

  async run(
    region: RegionQuery,
    callbacks: ProgressiveQueryCallbacks,
  ): Promise<void> {
    if (this.#destroyed)
      throw new Error("viewer query controller is destroyed");
    this.#controller?.abort();
    const controller = new AbortController();
    this.#controller = controller;
    const generation = ++this.#generation;
    const removeExternalAbort = forwardAbort(region.signal, controller);
    const publicRegion = copyRegion(region);
    callbacks.onStart?.(publicRegion);
    try {
      const query: RegionQuery = {
        ...publicRegion,
        signal: controller.signal,
        trace: (trace) => {
          if (generation === this.#generation && !controller.signal.aborted) {
            callbacks.onTrace?.(trace);
          }
        },
      };
      for await (const tile of this.#archive.queryTiles(query)) {
        if (generation !== this.#generation || controller.signal.aborted)
          return;
        callbacks.onTile(tile);
      }
      if (generation === this.#generation && !controller.signal.aborted) {
        callbacks.onComplete?.();
      }
    } catch (error) {
      if (
        generation !== this.#generation ||
        controller.signal.aborted ||
        isAbortError(error)
      ) {
        return;
      }
      throw error;
    } finally {
      removeExternalAbort();
      if (generation === this.#generation) this.#controller = undefined;
    }
  }

  cancel(): void {
    this.#generation += 1;
    this.#controller?.abort();
    this.#controller = undefined;
  }

  destroy(): void {
    if (this.#destroyed) return;
    this.#destroyed = true;
    this.cancel();
  }
}

function copyRegion(region: RegionQuery): Readonly<RegionQuery> {
  const copied: RegionQuery = {
    sample: region.sample,
    contig: region.contig,
    start: region.start,
    end: region.end,
  };
  if (region.context !== undefined) copied.context = region.context;
  return copied;
}

function forwardAbort(
  external: AbortSignal | undefined,
  controller: AbortController,
): () => void {
  if (external === undefined) return () => undefined;
  if (external.aborted) {
    controller.abort(external.reason);
    return () => undefined;
  }
  const abort = (): void => controller.abort(external.reason);
  external.addEventListener("abort", abort, { once: true });
  return () => external.removeEventListener("abort", abort);
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}
