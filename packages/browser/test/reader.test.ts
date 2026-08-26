import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  CorruptRegionalPayloadError,
  decodeRegionalPayload,
  detectArchiveVersion,
  detectRegionalPayloadVersion,
  NotImplementedError,
  openPangenome,
  UnsupportedArchiveVersionError,
  UnsupportedRegionalPayloadVersionError,
  validateArchiveRange,
  validateRegionQuery,
} from "../src/reader/index.js";

function decodeHex(value: string): Uint8Array {
  const normalized = value.trim();
  if (normalized.length % 2 !== 0) throw new Error("invalid hex fixture");
  return Uint8Array.from(
    normalized.match(/../g)?.map((byte) => Number.parseInt(byte, 16)) ?? [],
  );
}

const recordRegionalFixture = decodeHex(
  readFileSync(
    new URL("../../../test-data/golden/record-region-v4.hex", import.meta.url),
    "utf8",
  ),
);

const recordRegionalExpected = JSON.parse(
  readFileSync(
    new URL(
      "../../../test-data/golden/record-region-v4.expected.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as {
  core: [number, number];
  semantics: string;
  reference: {
    sample: string;
    contig: string;
    fragment: number;
    start: number;
    end: number;
    traversal: string[];
  };
  nodeIds: string[];
  nodeSequences: string[];
  edges: string[];
  traversals: Array<{ weight: string; path: string[] }>;
};

describe("reader contract validation", () => {
  it("accepts safe genomic coordinates and bigint archive offsets", () => {
    expect(() =>
      validateRegionQuery({
        sample: "GRCh38",
        contig: "chr1",
        start: 100,
        end: 200,
      }),
    ).not.toThrow();
    expect(() => validateArchiveRange(2n ** 60n, 4096)).not.toThrow();
  });

  it("rejects unsafe genomic coordinates", () => {
    expect(() =>
      validateRegionQuery({
        sample: "GRCh38",
        contig: "chr1",
        start: Number.MAX_SAFE_INTEGER + 1,
        end: Number.MAX_SAFE_INTEGER + 2,
      }),
    ).toThrow(RangeError);
  });

  it("states clearly that archive decoding is not implemented", async () => {
    await expect(
      openPangenome({
        source: {
          size: () => Promise.resolve(0n),
          read: () => Promise.resolve(new Uint8Array()),
        },
      }),
    ).rejects.toBeInstanceOf(NotImplementedError);
  });

  it("dispatches v4 and clearly rejects legacy v3 semantics", () => {
    const v4 = new Uint8Array(12);
    v4.set(new TextEncoder().encode("PNGRNG04"));
    new DataView(v4.buffer).setUint32(8, 4, true);
    expect(detectArchiveVersion(v4)).toBe(4);

    const v3 = v4.slice();
    v3.set(new TextEncoder().encode("PNGRNG03"));
    new DataView(v3.buffer).setUint32(8, 3, true);
    expect(() => detectArchiveVersion(v3)).toThrow(
      UnsupportedArchiveVersionError,
    );
  });

  it("decodes the Rust record-preserving regional golden fixture", () => {
    expect(detectRegionalPayloadVersion(recordRegionalFixture)).toBe(4);
    const tile = decodeRegionalPayload(recordRegionalFixture, {
      archiveOffset: 2n ** 60n,
    });
    expect([tile.start, tile.end]).toEqual(recordRegionalExpected.core);
    expect(tile.semantics).toBe(recordRegionalExpected.semantics);
    expect(tile.archiveOffset).toBe(2n ** 60n);
    expect(tile.reference).toMatchObject({
      sample: recordRegionalExpected.reference.sample,
      contig: recordRegionalExpected.reference.contig,
      fragment: recordRegionalExpected.reference.fragment,
      start: recordRegionalExpected.reference.start,
      end: recordRegionalExpected.reference.end,
    });
    expect(Array.from(tile.referenceTraversal, String)).toEqual(
      recordRegionalExpected.reference.traversal,
    );
    expect(Array.from(tile.nodeIds, String)).toEqual(
      recordRegionalExpected.nodeIds,
    );
    const nodeSequences = Array.from(
      { length: tile.nodeIds.length },
      (_, index) =>
        new TextDecoder().decode(
          tile.nodeSequences.subarray(
            tile.nodeSequenceOffsets[index],
            tile.nodeSequenceOffsets[index + 1],
          ),
        ),
    );
    expect(nodeSequences).toEqual(recordRegionalExpected.nodeSequences);
    expect(Array.from(tile.edges, String)).toEqual(
      recordRegionalExpected.edges,
    );
    const traversals = Array.from(
      { length: tile.traversalWeights.length },
      (_, index) => ({
        weight: String(tile.traversalWeights[index]),
        path: Array.from(
          tile.traversalNodes.subarray(
            tile.traversalOffsets[index],
            tile.traversalOffsets[index + 1],
          ),
          String,
        ),
      }),
    );
    expect(traversals).toEqual(recordRegionalExpected.traversals);
  });

  it("rejects corrupt record payloads and explicitly dispatches legacy payloads", () => {
    expect(() =>
      decodeRegionalPayload(
        recordRegionalFixture.subarray(0, recordRegionalFixture.length - 1),
      ),
    ).toThrow(CorruptRegionalPayloadError);

    const impossibleOccurrences = recordRegionalFixture.slice();
    new DataView(impossibleOccurrences.buffer).setBigUint64(
      48,
      0xffff_ffff_ffff_ffffn,
      true,
    );
    expect(() => decodeRegionalPayload(impossibleOccurrences)).toThrow(
      CorruptRegionalPayloadError,
    );

    const legacy = recordRegionalFixture.slice();
    legacy.set(new TextEncoder().encode("PNGRGN03"));
    expect(detectRegionalPayloadVersion(legacy)).toBe(3);
    expect(() => decodeRegionalPayload(legacy)).toThrow(
      UnsupportedRegionalPayloadVersionError,
    );
  });
});
