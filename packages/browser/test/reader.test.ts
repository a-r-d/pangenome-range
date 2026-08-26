import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import {
  CorruptRegionalPayloadError,
  decodeRegionalPayload,
  detectArchiveVersion,
  detectRegionalPayloadVersion,
  HttpRangeResponseError,
  HttpRangeSource,
  MemoryRangeSource,
  openPangenome,
  RemoteObjectChangedError,
  TracingRangeSource,
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

const recordArchiveFixture = new Uint8Array(
  readFileSync(
    new URL(
      "../../../test-data/golden/record-archive-v4.pngr",
      import.meta.url,
    ),
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

  it("opens and queries the deterministic Rust v4 archive", async () => {
    const source = new TracingRangeSource(
      new MemoryRangeSource(recordArchiveFixture),
    );
    const archive = await openPangenome({ source });
    expect(archive.formatVersion).toBe(4);
    expect(archive.references()).toEqual([
      expect.objectContaining({
        sample: "CHM13",
        contig: "chr6",
        start: 31_350_872,
        end: 31_351_896,
      }),
    ]);
    const result = await archive.query({
      sample: "CHM13",
      contig: "chr6",
      start: 31_350_872,
      end: 31_351_896,
    });
    expect(result.semantics).toBe("anonymous-distinct-weighted-tile-paths");
    expect(result.tiles).toHaveLength(1);
    expect(result.tiles[0]).toMatchObject({
      start: 31_350_872,
      end: 31_351_896,
      semantics: "anonymous-distinct-weighted-tile-paths",
    });
    expect(result.tiles[0]?.nodeIds.length).toBeGreaterThan(0);
    expect(result.tiles[0]?.traversalWeights.length).toBeGreaterThan(0);
    expect(source.reads).toEqual([
      expect.objectContaining({
        offset: 0n,
        length: recordArchiveFixture.byteLength,
        succeeded: true,
      }),
    ]);
    await archive.close();
  });

  it("rejects archive reserved bytes and truncated stored ranges", async () => {
    const reserved = recordArchiveFixture.slice();
    reserved[48] = 1;
    await expect(
      openPangenome({ source: new MemoryRangeSource(reserved) }),
    ).rejects.toThrow("reserved bytes");

    await expect(
      openPangenome({
        source: new MemoryRangeSource(recordArchiveFixture.subarray(0, 128)),
      }),
    ).rejects.toThrow();
  });

  it("validates strict HTTP 206 ranges and stable object identity", async () => {
    const object = Uint8Array.from({ length: 32 }, (_, index) => index);
    let etag = '"fixture-v1"';
    const fetch = vi.fn<typeof globalThis.fetch>(async (_input, init) => {
      const range = new Headers(init?.headers).get("range");
      const match = /^bytes=(\d+)-(\d+)$/.exec(range ?? "");
      if (match === null) throw new Error("test request has no exact range");
      const start = Number(match[1]);
      const end = Number(match[2]);
      const body = object.slice(start, end + 1);
      return new Response(body, {
        status: 206,
        headers: {
          "Accept-Ranges": "bytes",
          "Content-Range": `bytes ${start}-${end}/${object.length}`,
          "Content-Length": String(body.length),
          ETag: etag,
        },
      });
    });
    const source = new HttpRangeSource("https://example.test/archive.pngr", {
      fetch,
    });
    expect(await source.size()).toBe(32n);
    expect(Array.from(await source.read(8n, 4))).toEqual([8, 9, 10, 11]);
    expect(source.requests).toHaveLength(2);

    etag = '"fixture-v2"';
    await expect(source.read(12n, 2)).rejects.toBeInstanceOf(
      RemoteObjectChangedError,
    );
  });

  it("refuses an origin that ignores Range", async () => {
    const source = new HttpRangeSource("https://example.test/archive.pngr", {
      fetch: async () => new Response(new Uint8Array(1024), { status: 200 }),
    });
    await expect(source.size()).rejects.toBeInstanceOf(HttpRangeResponseError);
  });

  it("reports malformed range metadata and uniquely traces parallel reads", async () => {
    const malformed = new HttpRangeSource("https://example.test/archive.pngr", {
      fetch: async () =>
        new Response(Uint8Array.of(0), {
          status: 206,
          headers: {
            "Accept-Ranges": "bytes",
            "Content-Range": "bytes 0-0/32",
            "Content-Length": "invalid",
            ETag: '"fixture-v1"',
          },
        }),
    });
    await expect(malformed.size()).rejects.toBeInstanceOf(
      HttpRangeResponseError,
    );

    const traced = new TracingRangeSource(
      new MemoryRangeSource(Uint8Array.of(0, 1, 2, 3)),
    );
    await Promise.all([traced.read(0n, 1), traced.read(1n, 1)]);
    expect(traced.reads.map(({ sequence }) => sequence).sort()).toEqual([0, 1]);
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
