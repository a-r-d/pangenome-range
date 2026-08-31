import type {
  FeatureQueryTrace,
  NamedTraversalGroup,
  RegionTile,
} from "../reader/types.js";
import type { LocalPattern } from "./tube-map-model.js";

export type ValidatedPresetErrorCode =
  | "tile-count-mismatch"
  | "digest-count-mismatch"
  | "missing-oriented-nodes"
  | "occurrence-weight-mismatch"
  | "membership-count-mismatch"
  | "membership-mismatch"
  | "located-group-count-mismatch"
  | "pattern-count-mismatch";

export class ValidatedPresetError extends Error {
  readonly code: ValidatedPresetErrorCode;

  constructor(code: ValidatedPresetErrorCode, message: string) {
    super(message);
    this.name = "ValidatedPresetError";
    this.code = code;
  }
}

export interface ExpectedPresetTraversalGroup {
  readonly tile: {
    readonly sample: string;
    readonly contig: string;
    readonly start: number;
    readonly end: number;
    readonly archiveOffset: string;
  };
  readonly traversalDigest: string;
  readonly occurrenceWeight: string;
  readonly memberships: readonly {
    readonly pathId: string;
    readonly multiplicity: string;
    readonly orientationRelativeToTraversal: "forward" | "reverse";
  }[];
}

export interface LocatedPresetTraversalGroup {
  readonly tile: RegionTile;
  readonly group: NamedTraversalGroup & {
    readonly orientedNodes: BigUint64Array;
  };
}

export type LocateValidatedPresetResult =
  | { readonly status: "cancelled" }
  | {
      readonly status: "validated";
      readonly groups: readonly LocatedPresetTraversalGroup[];
      readonly traces: readonly FeatureQueryTrace[];
    };

export async function locateValidatedPresetGroups(options: {
  readonly tiles: readonly RegionTile[];
  readonly expectedGroups: readonly ExpectedPresetTraversalGroup[];
  readonly loadMemberships: (
    tile: RegionTile,
    trace: (value: FeatureQueryTrace) => void,
  ) => Promise<readonly NamedTraversalGroup[]>;
  readonly isCurrent: () => boolean;
}): Promise<LocateValidatedPresetResult> {
  const located: LocatedPresetTraversalGroup[] = [];
  const traces: FeatureQueryTrace[] = [];
  for (const expected of options.expectedGroups) {
    if (!options.isCurrent()) return { status: "cancelled" };
    const matchingTiles = options.tiles.filter(
      (tile) =>
        tile.reference.sample === expected.tile.sample &&
        tile.reference.contig === expected.tile.contig &&
        tile.coreStart === expected.tile.start &&
        tile.coreEnd === expected.tile.end &&
        tile.provenance.archiveOffset === BigInt(expected.tile.archiveOffset),
    );
    if (matchingTiles.length !== 1)
      throw new ValidatedPresetError(
        "tile-count-mismatch",
        `Expected tile ${expected.tile.sample}:${expected.tile.contig}:${expected.tile.start}-${expected.tile.end} resolved ${matchingTiles.length} times`,
      );
    const tile = matchingTiles[0];
    if (tile === undefined)
      throw new ValidatedPresetError(
        "tile-count-mismatch",
        "Expected tile was not resolved",
      );
    const groups = await options.loadMemberships(tile, (trace) => {
      traces.push(trace);
    });
    if (!options.isCurrent()) return { status: "cancelled" };
    const matchingGroups = groups.filter(
      (group) => bytesToHex(group.traversalDigest) === expected.traversalDigest,
    );
    if (matchingGroups.length !== 1)
      throw new ValidatedPresetError(
        "digest-count-mismatch",
        `Expected traversal digest ${expected.traversalDigest} resolved ${matchingGroups.length} times`,
      );
    const group = matchingGroups[0];
    if (group === undefined)
      throw new ValidatedPresetError(
        "digest-count-mismatch",
        "Expected traversal group was not resolved",
      );
    if (group.orientedNodes === undefined)
      throw new ValidatedPresetError(
        "missing-oriented-nodes",
        `Traversal ${expected.traversalDigest} has no reconciled oriented nodes`,
      );
    if (group.occurrenceWeight !== BigInt(expected.occurrenceWeight))
      throw new ValidatedPresetError(
        "occurrence-weight-mismatch",
        `Traversal ${expected.traversalDigest} has the wrong occurrence weight`,
      );
    if (group.memberships.length !== expected.memberships.length)
      throw new ValidatedPresetError(
        "membership-count-mismatch",
        `Traversal ${expected.traversalDigest} has the wrong membership count`,
      );
    for (const expectedMembership of expected.memberships) {
      const matches = group.memberships.filter(
        (membership) =>
          membership.pathId === BigInt(expectedMembership.pathId) &&
          membership.multiplicity === BigInt(expectedMembership.multiplicity) &&
          membership.reversedRelativeToGroup ===
            (expectedMembership.orientationRelativeToTraversal === "reverse"),
      );
      if (matches.length !== 1)
        throw new ValidatedPresetError(
          "membership-mismatch",
          `Traversal ${expected.traversalDigest} did not reproduce path ${expectedMembership.pathId}, multiplicity, and orientation exactly`,
        );
    }
    located.push({
      tile,
      group: group as NamedTraversalGroup & {
        readonly orientedNodes: BigUint64Array;
      },
    });
  }
  if (located.length !== options.expectedGroups.length)
    throw new ValidatedPresetError(
      "located-group-count-mismatch",
      `Located ${located.length} of ${options.expectedGroups.length} expected traversal groups`,
    );
  return { status: "validated", groups: located, traces };
}

export function matchValidatedPresetPatterns(
  located: readonly LocatedPresetTraversalGroup[],
  patterns: readonly LocalPattern[],
): readonly LocalPattern[] {
  const matched: LocalPattern[] = [];
  for (const { tile, group } of located) {
    const candidates = patterns.filter(
      (pattern) =>
        pattern.source.archiveOffset === tile.provenance.archiveOffset &&
        sameHandles(group.orientedNodes, pattern.orientedNodes),
    );
    if (candidates.length !== 1)
      throw new ValidatedPresetError(
        "pattern-count-mismatch",
        `Expected traversal from tile offset ${tile.provenance.archiveOffset} resolved to ${candidates.length} displayed patterns`,
      );
    const pattern = candidates[0];
    if (pattern !== undefined) matched.push(pattern);
  }
  if (
    matched.length !== located.length ||
    new Set(matched.map((pattern) => pattern.id)).size !== located.length
  )
    throw new ValidatedPresetError(
      "pattern-count-mismatch",
      `Located ${matched.length} distinct displayed patterns for ${located.length} traversal groups`,
    );
  return matched;
}

function sameHandles(left: BigUint64Array, right: readonly bigint[]): boolean {
  return (
    left.length === right.length &&
    left.every((handle, index) => handle === right[index])
  );
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}
