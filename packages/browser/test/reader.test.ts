import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import {
  type NamedLociDescriptor,
  selectLocusPages,
} from "../src/reader/features.js";
import {
  CorruptRegionalPayloadError,
  canonicalGraphHash,
  canonicalHaplotypeTileHash,
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
  UnsupportedRegionalPayloadVersionError,
  validateArchiveRange,
  validateRegionQuery,
} from "../src/reader/index.js";

const conformanceDirectory = new URL(
  "../../../test-data/conformance/",
  import.meta.url,
);

interface ConformanceFixture {
  id: string;
  archiveVersion: 1;
  regionalVersion: 1;
  semantics: string;
  expected: {
    canonicalHash: string;
    graphHash: string;
    tileLocalHaplotypeHash: string;
    references: Array<{
      sample: string;
      contig: string;
      start: number;
      end: number;
    }>;
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
      nodeSequences: string[];
      edges: string[];
      referenceTraversal: string[];
      semantics: string;
      weightedTraversals: Array<{ weight: string; nodes: string[] }>;
    };
  };
  files: Record<string, { bytes: number; sha256: string }>;
  sections: {
    archiveHeader: { offset: number; length: number };
    rootIndex: { offset: number; length: number };
    directoryPages: { offset: number; length: number; pageCount: number };
    regionalPayload: {
      offset: number;
      encodedLength: number;
      decodedLength: number;
      codec: string;
    };
    extensionDirectory: null | {
      offset: number;
      length: number;
      entryCount: number;
    };
    extensionPayload: null | {
      offset: number;
      encodedLength: number;
      decodedLength: number;
      codec: string;
      required: boolean;
    };
  };
}

const conformanceManifest = JSON.parse(
  readFileSync(new URL("manifest.json", conformanceDirectory), "utf8"),
) as {
  schemaVersion: number;
  format: {
    archiveMagic: string;
    archiveVersion: number;
    regionalMagic: string;
    regionalVersion: number;
    headerBytes: number;
    directoryPageBytes: number;
    directoryEntryBytes: number;
    maximumDirectoryEntriesPerPage: number;
  };
  fixtures: ConformanceFixture[];
  expectedFailures: Array<{
    id: string;
    file: string;
    inputKind: "archive" | "regional-payload";
    expected: "reject";
    rejectionStage: string;
    bytes: number;
    sha256: string;
  }>;
};

function sha256(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function decodeHex(value: string): Uint8Array {
  const normalized = value.trim();
  if (normalized.length % 2 !== 0) throw new Error("invalid hex fixture");
  return Uint8Array.from(
    normalized.match(/../g)?.map((byte) => Number.parseInt(byte, 16)) ?? [],
  );
}

const recordRegionalFixture = decodeHex(
  readFileSync(
    new URL("../../../test-data/golden/record-region-v1.hex", import.meta.url),
    "utf8",
  ),
);

const recordArchiveFixture = new Uint8Array(
  readFileSync(
    new URL(
      "../../../test-data/golden/record-archive-v1.pngr",
      import.meta.url,
    ),
  ),
);

const recordArchiveMetadata = JSON.parse(
  readFileSync(
    new URL(
      "../../../test-data/golden/record-archive-v1.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as {
  features: {
    typeIds: string[];
    namedLoci: {
      recordCount: number;
      pageCount: number;
      query: string;
      mode: "prefix";
      expected: {
        matchedName: string;
        displayName: string;
        stableId: string;
        featureType: string;
        sample: string;
        contig: string;
        start: number;
        end: number;
        strand: "forward";
      };
    };
    summary: {
      seriesCount: number;
      binCount: number;
      coveredBases: number;
      tileCount: number;
      baseBinSpan: number;
    };
  };
};

const recordRegionalExpected = JSON.parse(
  readFileSync(
    new URL(
      "../../../test-data/golden/record-region-v1.expected.json",
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
  it("binary-searches sorted locus leaf fences", () => {
    const storage = {
      offset: 10_000n,
      encodedLength: 10n,
      decodedLength: 20n,
      codec: 3 as const,
      integrity: new Uint8Array(16),
    };
    const descriptor: NamedLociDescriptor = {
      annotationSha256: new Uint8Array(32),
      annotationName: "genes.gff3",
      recordCount: 6n,
      pages: [
        { firstKey: "a", lastKey: "bq", recordCount: 2n, storage },
        {
          firstKey: "brca1",
          lastKey: "brca2",
          recordCount: 2n,
          storage,
        },
        { firstKey: "brcb", lastKey: "z", recordCount: 2n, storage },
      ],
    };
    expect(selectLocusPages(descriptor, "brca1", "exact")).toEqual([
      descriptor.pages[1],
    ]);
    expect(selectLocusPages(descriptor, "brca", "prefix")).toEqual([
      descriptor.pages[1],
    ]);
    expect(selectLocusPages(descriptor, "zzzz", "exact")).toEqual([]);
  });

  it("consumes the normative machine-readable format constants and checksums", () => {
    expect(conformanceManifest).toMatchObject({
      schemaVersion: 2,
      format: {
        archiveMagic: "PNGRNG01",
        archiveVersion: 1,
        regionalMagic: "PNGRGN01",
        regionalVersion: 1,
        headerBytes: 64,
        directoryPageBytes: 4096,
        directoryEntryBytes: 56,
        maximumDirectoryEntriesPerPage: 72,
      },
    });
    for (const fixture of conformanceManifest.fixtures) {
      for (const [name, metadata] of Object.entries(fixture.files)) {
        const bytes = new Uint8Array(
          readFileSync(new URL(name, conformanceDirectory)),
        );
        expect(bytes.byteLength, name).toBe(metadata.bytes);
        expect(sha256(bytes), name).toBe(metadata.sha256);
      }
      expect(fixture.sections.archiveHeader).toEqual({ offset: 0, length: 64 });
      expect(fixture.sections.rootIndex).toEqual({ offset: 64, length: 106 });
      const extensionLength = fixture.sections.extensionDirectory?.length ?? 0;
      expect(fixture.sections.directoryPages).toEqual({
        offset: 170 + extensionLength,
        length: 4096,
        pageCount: 1,
      });
      expect(fixture.sections.regionalPayload).toEqual({
        offset: 4266 + extensionLength,
        encodedLength: 122,
        decodedLength: 316,
        codec: "zstd-3",
      });
      if (fixture.id === "format-v1") {
        expect(fixture.sections.extensionDirectory).toBeNull();
        expect(fixture.sections.extensionPayload).toBeNull();
      } else {
        expect(fixture.sections.extensionDirectory).toMatchObject({
          offset: 170,
          entryCount: 1,
        });
        expect(fixture.sections.extensionPayload).toMatchObject({
          offset: fixture.sections.regionalPayload.offset + 122,
          codec: "none",
          required: false,
        });
      }
    }
  });

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

  it("accepts only the current Rust archive version", () => {
    const current = new Uint8Array(12);
    current.set(new TextEncoder().encode("PNGRNG01"));
    new DataView(current.buffer).setUint32(8, 1, true);
    expect(detectArchiveVersion(current)).toBe(1);

    for (const version of [3, 4]) {
      const obsolete = current.slice();
      obsolete.set(new TextEncoder().encode(`PNGRNG0${version}`));
      new DataView(obsolete.buffer).setUint32(8, version, true);
      expect(() => detectArchiveVersion(obsolete)).toThrow(
        UnsupportedArchiveVersionError,
      );
    }
  });

  it("matches the complete Rust conformance fixture matrix", async () => {
    for (const fixture of conformanceManifest.fixtures) {
      const archiveBytes = new Uint8Array(
        readFileSync(new URL(`${fixture.id}.pngr`, conformanceDirectory)),
      );
      const archive = await openPangenome(new MemoryRangeSource(archiveBytes));
      expect(archive.formatVersion).toBe(fixture.archiveVersion);
      expect(archive.semantics).toBe(fixture.semantics);
      expect(archive.references()).toMatchObject(fixture.expected.references);
      const result = await archive.query({
        ...fixture.expected.query,
        trace: true,
      });
      expect(result.trace?.canonicalHash).toBe(fixture.expected.canonicalHash);
      expect(canonicalGraphHash(result.graph)).toBe(fixture.expected.graphHash);
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
      expect(
        Array.from({ length: tile.nodeIds.length }, (_, index) =>
          new TextDecoder().decode(
            tile.nodeSequences.subarray(
              tile.nodeSequenceOffsets[index],
              tile.nodeSequenceOffsets[index + 1],
            ),
          ),
        ),
      ).toEqual(fixture.expected.tile.nodeSequences);
      expect(Array.from(tile.edges, String)).toEqual(
        fixture.expected.tile.edges,
      );
      expect(Array.from(tile.referenceTraversal, String)).toEqual(
        fixture.expected.tile.referenceTraversal,
      );
      expect(Array.from(tile.haplotypes.weights, String)).toEqual(
        fixture.expected.tile.weightedTraversals.map(({ weight }) => weight),
      );
      expect(
        Array.from({ length: tile.haplotypes.weights.length }, (_, index) => ({
          weight: String(tile.haplotypes.weights[index]),
          nodes: Array.from(
            tile.haplotypes.orientedNodes.subarray(
              tile.haplotypes.traversalOffsets[index],
              tile.haplotypes.traversalOffsets[index + 1],
            ),
            String,
          ),
        })),
      ).toEqual(fixture.expected.tile.weightedTraversals);
      expect(Array.from(result.graph.nodes.ids, String)).toEqual(
        fixture.expected.tile.nodeIds,
      );
      expect(Array.from(result.graph.edges.from, String)).toEqual(
        fixture.expected.tile.edges.filter((_, index) => index % 2 === 0),
      );
      expect(Array.from(result.graph.edges.to, String)).toEqual(
        fixture.expected.tile.edges.filter((_, index) => index % 2 === 1),
      );
      expect(Array.from(result.graph.referenceTraversal, String)).toEqual(
        fixture.expected.tile.referenceTraversal,
      );
      expect(canonicalHaplotypeTileHash(tile)).toBe(
        fixture.expected.tileLocalHaplotypeHash,
      );
      await archive.close();
    }
  });

  it("rejects every corrupt fixture declared by the shared manifest", async () => {
    const query = conformanceManifest.fixtures[0]?.expected.query;
    if (query === undefined) throw new Error("conformance query is absent");
    for (const failure of conformanceManifest.expectedFailures) {
      const bytes = new Uint8Array(
        readFileSync(new URL(failure.file, conformanceDirectory)),
      );
      expect(bytes.byteLength, failure.id).toBe(failure.bytes);
      expect(sha256(bytes), failure.id).toBe(failure.sha256);
      let rejected = false;
      try {
        if (failure.inputKind === "regional-payload") {
          decodeRegionalPayload(bytes);
        } else {
          const archive = await openPangenome(new MemoryRangeSource(bytes));
          try {
            await archive.query(query);
          } finally {
            await archive.close();
          }
        }
      } catch (error) {
        expect(error, `${failure.id} must reject`).toBeInstanceOf(Error);
        rejected = true;
      }
      expect(rejected, `${failure.id} unexpectedly decoded`).toBe(true);
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
        const trailing = new Uint8Array(compressed.byteLength + 1);
        trailing.set(compressed);
        expect(() => decompressor.decompress(trailing, raw.byteLength)).toThrow(
          "exactly one frame",
        );

        const dictionary = compressed.slice();
        dictionary[4] = (dictionary[4] as number) | 0x01;
        expect(() =>
          decompressor.decompress(dictionary, raw.byteLength),
        ).toThrow("dictionaries");

        const reserved = compressed.slice();
        reserved[4] = (reserved[4] as number) | 0x08;
        expect(() => decompressor.decompress(reserved, raw.byteLength)).toThrow(
          "reserved descriptor",
        );

        const skippable = compressed.slice();
        skippable.set([0x50, 0x2a, 0x4d, 0x18]);
        expect(() =>
          decompressor.decompress(skippable, raw.byteLength),
        ).toThrow("standard frame");
      }
    }
  });

  it("fails closed for malformed archive and regional fixture classes", async () => {
    const fixture = conformanceManifest.fixtures.find(
      ({ id }) => id === "format-v1",
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
    for (const version of [1] as const) {
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

  it("opens and queries the deterministic Rust v1 archive", async () => {
    const source = new TracingRangeSource(
      new MemoryRangeSource(recordArchiveFixture),
    );
    const archive = await openPangenome({ source });
    expect(archive.formatVersion).toBe(1);
    expect(archive.references()).toEqual([
      expect.objectContaining({
        sample: "CHM13",
        contig: "chr6",
        start: 31_350_872,
        end: 31_351_896,
      }),
    ]);
    expect(archive.capabilities()).toEqual({
      namedLoci: true,
      multiscaleSummaries: true,
    });
    const loci = await archive.searchLoci({
      name: recordArchiveMetadata.features.namedLoci.query,
      mode: recordArchiveMetadata.features.namedLoci.mode,
      trace: true,
    });
    expect(loci).toMatchObject({
      normalizedQuery: recordArchiveMetadata.features.namedLoci.query,
      annotationName: "record-annotations-v1.gff3",
      totalIndexedRecords: BigInt(
        recordArchiveMetadata.features.namedLoci.recordCount,
      ),
      truncated: false,
      hits: [
        {
          matchedName:
            recordArchiveMetadata.features.namedLoci.expected.matchedName,
          displayName:
            recordArchiveMetadata.features.namedLoci.expected.displayName,
          stableId: recordArchiveMetadata.features.namedLoci.expected.stableId,
          featureType:
            recordArchiveMetadata.features.namedLoci.expected.featureType,
          reference: {
            sample: recordArchiveMetadata.features.namedLoci.expected.sample,
            contig: recordArchiveMetadata.features.namedLoci.expected.contig,
            start: recordArchiveMetadata.features.namedLoci.expected.start,
            end: recordArchiveMetadata.features.namedLoci.expected.end,
          },
          strand: recordArchiveMetadata.features.namedLoci.expected.strand,
        },
      ],
    });
    expect(loci.annotationSha256).toMatch(/^[0-9a-f]{64}$/);
    const overview = await archive.summary({
      sample: "CHM13",
      contig: "chr6",
      start: 31_350_872,
      end: 31_351_896,
      maxBins: 10,
      trace: true,
    });
    expect(overview.bins).toHaveLength(
      recordArchiveMetadata.features.summary.binCount,
    );
    expect(overview.bins[0]).toMatchObject({
      level: 0,
      binSpan: recordArchiveMetadata.features.summary.baseBinSpan,
      coveredBases: BigInt(recordArchiveMetadata.features.summary.coveredBases),
      tileCount: BigInt(recordArchiveMetadata.features.summary.tileCount),
    });
    expect(overview.bins[0]?.nodeRecords).toBeGreaterThan(0n);
    expect(overview.bins[0]?.edgeRecords).toBeGreaterThan(0n);
    expect(overview.bins[0]?.gbwtRecords).toBeGreaterThan(0n);
    expect(overview.bins[0]?.occurrences).toBeGreaterThan(0n);
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
      extensionEntries:
        2 +
        recordArchiveMetadata.features.namedLoci.pageCount +
        recordArchiveMetadata.features.summary.seriesCount,
    });
    archive.clearCaches();
    expect(archive.cacheStats()).toEqual({
      directoryBytes: 0,
      directoryEntries: 0,
      payloadBytes: 0,
      payloadEntries: 0,
      extensionBytes: 0,
      extensionEntries: 0,
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
    ).toBe("3674cc04aea1d17ab4440075089d437cb642702661a454ea764f46866a41e251");
    await blobArchive.close();
  });

  it("rejects incomplete extension pointers and truncated stored ranges", async () => {
    const incompleteExtension = recordArchiveFixture.slice();
    incompleteExtension[48] = 1;
    await expect(
      openPangenome({ source: new MemoryRangeSource(incompleteExtension) }),
    ).rejects.toThrow("extension directory pointer");

    await expect(
      openPangenome({
        source: new MemoryRangeSource(recordArchiveFixture.subarray(0, 128)),
      }),
    ).rejects.toThrow();
  });

  it("skips unknown optional extensions and rejects unknown required extensions", async () => {
    const fixture = conformanceManifest.fixtures.find(
      ({ id }) => id === "format-v1-optional-extension",
    ) as ConformanceFixture;
    const bytes = new Uint8Array(
      readFileSync(new URL(`${fixture.id}.pngr`, conformanceDirectory)),
    );
    const optional = await openPangenome(new MemoryRangeSource(bytes));
    expect(optional.references()).toEqual(
      fixture.expected.references.map((reference) =>
        expect.objectContaining(reference),
      ),
    );
    await optional.close();

    const required = bytes.slice();
    const view = new DataView(required.buffer);
    const extensionOffset = Number(view.getBigUint64(48, true));
    view.setUint32(extensionOffset + 32 + 16, 1, true);
    await expect(
      openPangenome(new MemoryRangeSource(required)),
    ).rejects.toThrow("unknown required extension");
  });

  it("verifies stored payload integrity once before caching", async () => {
    const archive = await openPangenome(
      new MemoryRangeSource(recordArchiveFixture),
    );
    const query = {
      sample: "CHM13",
      contig: "chr6",
      start: 31_350_872,
      end: 31_351_896,
      trace: true,
    } as const;
    const cold = await archive.query(query);
    const warm = await archive.query(query);
    expect(cold.trace?.integrityMs).toBeGreaterThan(0);
    expect(warm.trace?.integrityMs).toBe(0);
    await archive.close();
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
    expect(detectRegionalPayloadVersion(recordRegionalFixture)).toBe(1);
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

  it("rejects corrupt record payloads and obsolete payload versions", () => {
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

    const obsolete = recordRegionalFixture.slice();
    obsolete.set(new TextEncoder().encode("PNGRGN03"));
    expect(() => detectRegionalPayloadVersion(obsolete)).toThrow(
      UnsupportedRegionalPayloadVersionError,
    );
  });
});
