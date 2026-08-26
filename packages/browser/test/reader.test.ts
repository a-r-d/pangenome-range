import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import {
  CorruptRegionalPayloadError,
  decodeRegionalPayload,
  detectArchiveVersion,
  detectRegionalPayloadVersion,
  FzstdDecompressor,
  HttpRangeResponseError,
  HttpRangeSource,
  MemoryRangeSource,
  openPangenome,
  RemoteObjectChangedError,
  TracingRangeSource,
  UnsupportedArchiveVersionError,
  validateArchiveRange,
  validateRegionQuery,
} from "../src/reader/index.js";

const conformanceDirectory = new URL(
  "../../../test-data/conformance/",
  import.meta.url,
);

interface ConformanceFixture {
  id: string;
  archiveVersion: 3 | 4;
  regionalVersion: 2 | 3 | 4;
  semantics: string;
  expected: {
    canonicalHash: string;
    query: {
      sample: string;
      contig: string;
      start: number;
      end: number;
      context: number;
    };
    tile: {
      coreStart: number;
      coreEnd: number;
      nodeIds: string[];
      edges: string[];
      referenceTraversal: string[];
      semantics: string;
      namedPathIds: string[];
      weightedTraversals: Array<{ weight: string; nodes: string[] }>;
    };
  };
}

const conformanceManifest = JSON.parse(
  readFileSync(new URL("manifest.json", conformanceDirectory), "utf8"),
) as { fixtures: ConformanceFixture[] };

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
    expect(() =>
      validateRegionQuery({
        sample: "GRCh38",
        contig: "chr1",
        start: 100,
        end: 200,
        context: 101,
      }),
    ).toThrow("construction halo 100");
  });

  it("dispatches every Rust-supported archive version", () => {
    const v4 = new Uint8Array(12);
    v4.set(new TextEncoder().encode("PNGRNG04"));
    new DataView(v4.buffer).setUint32(8, 4, true);
    expect(detectArchiveVersion(v4)).toBe(4);

    const v3 = v4.slice();
    v3.set(new TextEncoder().encode("PNGRNG03"));
    new DataView(v3.buffer).setUint32(8, 3, true);
    expect(detectArchiveVersion(v3)).toBe(3);
  });

  it("matches the complete Rust conformance fixture matrix", async () => {
    for (const fixture of conformanceManifest.fixtures) {
      const archiveBytes = new Uint8Array(
        readFileSync(new URL(`${fixture.id}.pngr`, conformanceDirectory)),
      );
      const archive = await openPangenome(new MemoryRangeSource(archiveBytes));
      expect(archive.formatVersion).toBe(fixture.archiveVersion);
      expect(archive.semantics).toBe(fixture.semantics);
      expect(archive.references()).toMatchObject([
        { sample: "GRCh38", contig: "chr1", start: 100, end: 102 },
      ]);
      const result = await archive.query({
        ...fixture.expected.query,
        trace: true,
      });
      expect(result.trace?.canonicalHash).toBe(fixture.expected.canonicalHash);
      expect(result.tiles).toHaveLength(1);
      const tile = result.tiles[0] as (typeof result.tiles)[number];
      expect(tile.semantics).toBe(fixture.expected.tile.semantics);
      expect([tile.coreStart, tile.coreEnd]).toEqual([
        fixture.expected.tile.coreStart,
        fixture.expected.tile.coreEnd,
      ]);
      expect(Array.from(tile.nodeIds, String)).toEqual(
        fixture.expected.tile.nodeIds,
      );
      expect(Array.from(tile.edges, String)).toEqual(
        fixture.expected.tile.edges,
      );
      expect(Array.from(tile.referenceTraversal, String)).toEqual(
        fixture.expected.tile.referenceTraversal,
      );
      if (tile.haplotypes.kind === "named-paths") {
        expect(Array.from(tile.haplotypes.pathIds, String)).toEqual(
          fixture.expected.tile.namedPathIds,
        );
      } else {
        expect(Array.from(tile.haplotypes.weights, String)).toEqual(
          fixture.expected.tile.weightedTraversals.map(({ weight }) => weight),
        );
      }
      await archive.close();
    }
  });

  it("decompresses Rust zstd levels 1, 3, and 6 to exact lengths", async () => {
    const decompressor = new FzstdDecompressor();
    for (const fixture of conformanceManifest.fixtures) {
      const raw = new Uint8Array(
        readFileSync(
          new URL(`${fixture.id}.payload.raw`, conformanceDirectory),
        ),
      );
      for (const level of [1, 3, 6]) {
        const compressed = new Uint8Array(
          readFileSync(
            new URL(`${fixture.id}.payload.zstd${level}`, conformanceDirectory),
          ),
        );
        expect(
          await decompressor.decompress(compressed, raw.byteLength),
        ).toEqual(raw);
        expect(() =>
          decompressor.decompress(compressed, raw.byteLength + 1),
        ).toThrow("declares");
      }
    }
  });

  it("fails closed for malformed archive and regional fixture classes", async () => {
    const fixture = conformanceManifest.fixtures.find(
      ({ id }) => id === "archive-v4-record-v4",
    ) as ConformanceFixture;
    const archiveBytes = new Uint8Array(
      readFileSync(new URL(`${fixture.id}.pngr`, conformanceDirectory)),
    );
    await expect(
      openPangenome(new MemoryRangeSource(archiveBytes.subarray(0, 63))),
    ).rejects.toThrow("shorter than its header");
    await expect(
      openPangenome(new MemoryRangeSource(archiveBytes.subarray(0, 100))),
    ).rejects.toThrow();
    await expect(
      openPangenome(new MemoryRangeSource(archiveBytes.subarray(0, 4_265))),
    ).rejects.toThrow("root/directory offsets");

    const badMagic = archiveBytes.slice();
    badMagic[0] = 0;
    await expect(
      openPangenome(new MemoryRangeSource(badMagic)),
    ).rejects.toBeInstanceOf(UnsupportedArchiveVersionError);

    const badVersion = archiveBytes.slice();
    new DataView(badVersion.buffer).setUint32(8, 99, true);
    await expect(
      openPangenome(new MemoryRangeSource(badVersion)),
    ).rejects.toBeInstanceOf(UnsupportedArchiveVersionError);

    const rootOverflow = archiveBytes.slice();
    new DataView(rootOverflow.buffer).setBigUint64(
      24,
      0xffff_ffff_ffff_ffffn,
      true,
    );
    await expect(
      openPangenome(new MemoryRangeSource(rootOverflow)),
    ).rejects.toThrow("maxRootBytes");

    const invalidUtf8 = archiveBytes.slice();
    invalidUtf8[80] = 0xff;
    await expect(
      openPangenome(new MemoryRangeSource(invalidUtf8)),
    ).rejects.toThrow("UTF-8");

    const unknownCodec = archiveBytes.slice();
    unknownCodec[64 + 98] = 99;
    await expect(
      openPangenome(new MemoryRangeSource(unknownCodec)),
    ).rejects.toThrow("codec");

    const outOfFile = archiveBytes.slice();
    const directoryOffset = 64 + 106;
    new DataView(outOfFile.buffer).setBigUint64(
      directoryOffset + 32,
      0xffff_ffff_ffff_fff0n,
      true,
    );
    const outOfFileArchive = await openPangenome(
      new MemoryRangeSource(outOfFile),
    );
    await expect(
      outOfFileArchive.query(fixture.expected.query),
    ).rejects.toThrow();

    const namedRaw = new Uint8Array(
      readFileSync(
        new URL("archive-v3-named-v2.payload.raw", conformanceDirectory),
      ),
    );
    const badDictionary = namedRaw.slice();
    new DataView(badDictionary.buffer).setUint32(173, 99, true);
    expect(() => decodeRegionalPayload(badDictionary)).toThrow(
      "dictionary index",
    );

    for (const current of conformanceManifest.fixtures) {
      const raw = new Uint8Array(
        readFileSync(
          new URL(`${current.id}.payload.raw`, conformanceDirectory),
        ),
      );
      expect(() =>
        decodeRegionalPayload(raw.subarray(0, raw.length - 1)),
      ).toThrow();
    }
    const unreasonable = recordRegionalFixture.slice();
    new DataView(unreasonable.buffer).setBigUint64(
      24,
      0xffff_ffff_ffff_ffffn,
      true,
    );
    expect(() => decodeRegionalPayload(unreasonable)).toThrow();
  });

  it("bounds parser behavior over generated versioned byte arrays", () => {
    let state = 0x9e37_79b9;
    const next = (): number => {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      return state >>> 0;
    };
    const encoder = new TextEncoder();
    for (const version of [2, 3, 4] as const) {
      for (let sample = 0; sample < 200; sample += 1) {
        const bytes = new Uint8Array(24 + (next() % 489));
        for (let index = 0; index < bytes.length; index += 1) {
          bytes[index] = next() & 0xff;
        }
        bytes.set(encoder.encode(`PNGRGN0${version}`));
        new DataView(bytes.buffer).setUint32(8, version, true);
        new DataView(bytes.buffer).setUint32(12, 1, true);
        try {
          const tile = decodeRegionalPayload(bytes);
          expect(tile.nodeIds.length).toBeLessThanOrEqual(bytes.length);
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }
      }
    }
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
    expect(archive.cacheStats()).toMatchObject({
      directoryEntries: 1,
      payloadEntries: 1,
    });
    archive.clearCaches();
    expect(archive.cacheStats()).toEqual({
      directoryBytes: 0,
      directoryEntries: 0,
      payloadBytes: 0,
      payloadEntries: 0,
    });
    await archive.close();

    const blobArchive = await openPangenome(new Blob([recordArchiveFixture]));
    expect(
      (
        await blobArchive.query({
          sample: "CHM13",
          contig: "chr6",
          start: 31_351_000,
          end: 31_351_500,
          trace: true,
        })
      ).trace?.canonicalHash,
    ).toBe("1a04302d90bc504962c8961792797f3a148f4e8cb6c48af0d4e04937224835e3");
    await blobArchive.close();
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

  it("uses usable HEAD metadata, If-Range, and explicit cache policy", async () => {
    const object = Uint8Array.from({ length: 32 }, (_, index) => index);
    const calls: RequestInit[] = [];
    const fetch = vi.fn<typeof globalThis.fetch>(async function (
      this: unknown,
      _input,
      init = {},
    ) {
      expect(this).toBeUndefined();
      calls.push(init);
      if (init.method === "HEAD") {
        return new Response(null, {
          status: 200,
          headers: {
            "Accept-Ranges": "bytes",
            "Content-Length": String(object.length),
            ETag: '"head-v1"',
          },
        });
      }
      const headers = new Headers(init.headers);
      expect(headers.get("if-range")).toBe('"head-v1"');
      const match = /^bytes=(\d+)-(\d+)$/.exec(headers.get("range") ?? "");
      if (match === null) throw new Error("test request has no range");
      const start = Number(match[1]);
      const end = Number(match[2]);
      const body = object.slice(start, end + 1);
      return new Response(body, {
        status: 206,
        headers: {
          "Accept-Ranges": "bytes",
          "Content-Range": `bytes ${start}-${end}/${object.length}`,
          "Content-Length": String(body.length),
          ETag: '"head-v1"',
        },
      });
    });
    const source = new HttpRangeSource("https://example.test/archive.pngr", {
      fetch,
      cache: "no-store",
    });
    expect(await source.size()).toBe(32n);
    expect(Array.from(await source.read(4n, 3))).toEqual([4, 5, 6]);
    expect(source.requests.map(({ method }) => method)).toEqual([
      "HEAD",
      "GET",
    ]);
    expect(calls.every(({ cache }) => cache === "no-store")).toBe(true);
  });

  it("accepts incorrect 200 responses only below an explicit byte cap", async () => {
    const object = Uint8Array.from({ length: 32 }, (_, index) => index);
    const fullResponse = async (): Promise<Response> =>
      new Response(object, {
        status: 200,
        headers: {
          "Content-Length": String(object.length),
          ETag: '"small-v1"',
        },
      });
    const accepted = new HttpRangeSource("https://example.test/small.pngr", {
      fetch: fullResponse,
      useHead: false,
      maxFullResponseBytes: 32,
    });
    expect(Array.from(await accepted.read(8n, 4))).toEqual([8, 9, 10, 11]);

    const rejected = new HttpRangeSource("https://example.test/large.pngr", {
      fetch: fullResponse,
      useHead: false,
    });
    await expect(rejected.read(0n, 1)).rejects.toThrow(
      "maxFullResponseBytes is 0",
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
    await Promise.all([traced.read(0n, 2), traced.read(1n, 2)]);
    await expect(traced.read(4n, 1)).rejects.toThrow("source size");
    expect(traced.reads.map(({ sequence }) => sequence).sort()).toEqual([
      0, 1, 2,
    ]);
    expect(traced.reads[2]).toMatchObject({
      sequence: 2,
      succeeded: false,
      bytesReturned: 0,
      layer: "source",
    });
    expect(traced.summary()).toMatchObject({
      calls: 3,
      successfulCalls: 2,
      totalRequestedBytes: 5,
      returnedBytes: 4,
      uniqueBytes: 4,
      duplicateBytes: 1,
      coalescedRanges: [
        { offset: 0n, length: 3 },
        { offset: 4n, length: 1 },
      ],
    });
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

  it("rejects corrupt record payloads and dispatches retained payload versions", () => {
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
      CorruptRegionalPayloadError,
    );
  });
});
