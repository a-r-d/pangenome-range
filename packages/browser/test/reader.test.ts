import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { blake3 } from "@noble/hashes/blake3.js";
import { describe, expect, it, vi } from "vitest";
import {
  decodeArchiveMetadata,
  type NamedLociDescriptor,
  selectLocusPages,
  selectSummarySeries,
} from "../src/reader/features.js";
import {
  type ChunkDecompressor,
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
  type RegionTile,
  RemoteObjectChangedError,
  TracingRangeSource,
  UnsupportedArchiveVersionError,
  UnsupportedRegionalPayloadVersionError,
  validateArchiveRange,
  validateRegionQuery,
} from "../src/reader/index.js";
import {
  decodePathCatalogPage,
  decodePathMembershipDescriptor,
  decodeTileMembershipPage,
  traversalMembershipDigest,
} from "../src/reader/path-membership.js";

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
    inputKind:
      | "archive"
      | "regional-payload"
      | "zstd-frame"
      | "archive-metadata";
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

const micbKirArchiveFixture = new Uint8Array(
  readFileSync(
    new URL(
      "../../../test-data/conformance/micb-kir3dl1-reader-v1.pngr",
      import.meta.url,
    ),
  ),
);

const pathMembershipArchiveFixture = new Uint8Array(
  readFileSync(
    new URL(
      "../../../test-data/golden/path-membership-v1.pngr",
      import.meta.url,
    ),
  ),
);

function mismatchedPathMembershipProvenance(): Uint8Array {
  const bytes = pathMembershipArchiveFixture.slice();
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const extensionOffset = Number(view.getBigUint64(48, true));
  const extensionCount = Number(view.getBigUint64(extensionOffset + 16, true));
  const typeId = new TextEncoder().encode("archive-meta-v1-");
  let metadataEntry = -1;
  for (let index = 0; index < extensionCount; index += 1) {
    const offset = extensionOffset + 32 + index * 64;
    if (typeId.every((byte, byteIndex) => bytes[offset + byteIndex] === byte)) {
      metadataEntry = offset;
      break;
    }
  }
  if (metadataEntry < 0 || bytes[metadataEntry + 20] !== 0) {
    throw new Error("golden archive metadata entry is missing or compressed");
  }
  const payloadOffset = Number(view.getBigUint64(metadataEntry + 24, true));
  const payloadLength = Number(view.getBigUint64(metadataEntry + 32, true));
  bytes[payloadOffset + 24] = (bytes[payloadOffset + 24] ?? 0) ^ 1;
  const digest = blake3(
    bytes.subarray(payloadOffset, payloadOffset + payloadLength),
  );
  bytes.set(digest.subarray(0, 16), metadataEntry + 48);
  return bytes;
}

const pathMembershipExpected = JSON.parse(
  readFileSync(
    new URL(
      "../../../test-data/golden/path-membership-v1.expected.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as {
  archiveBytes: number;
  archiveSha256: string;
  query: { sample: string; contig: string; start: number; end: number };
  catalogPathCount: string;
  referencedPaths: number;
  tiles: number;
  groups: number;
  memberships: number;
  occurrenceWeight: string;
  membershipMultiplicity: string;
  firstReferencedPath: {
    pathId: string;
    canonicalName: string;
    sample: string;
    contig: string;
    haplotype: string;
    fragment: string;
    sense: string;
  };
};

class CompletionOrderDecompressor implements ChunkDecompressor {
  readonly #inner = new FzstdDecompressor();
  readonly #reverse: boolean;

  constructor(reverse: boolean) {
    this.#reverse = reverse;
  }

  async decompress(
    compressed: Uint8Array,
    expectedLength: number,
    options?: { signal?: AbortSignal },
  ): Promise<Uint8Array> {
    const discriminator =
      ((compressed[compressed.byteLength - 1] ?? 0) + compressed.byteLength) %
      7;
    const delayMs = (this.#reverse ? 6 - discriminator : discriminator) * 2;
    await new Promise((resolve) => setTimeout(resolve, delayMs));
    options?.signal?.throwIfAborted();
    return this.#inner.decompress(compressed, expectedLength, options);
  }
}

function duplicateGoldenLogicalEntry(): Uint8Array {
  const bytes = recordArchiveFixture.slice();
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  view.setBigUint64(32, 2n, true);
  let position = 64;
  position += 8;
  for (let index = 0; index < 2; index += 1) {
    const length = Number(view.getBigUint64(position, true));
    position += 8 + length;
  }
  const firstPageOffset = Number(view.getBigUint64(position + 5 * 8, true));
  view.setBigUint64(position + 7 * 8, 2n, true);
  view.setUint32(firstPageOffset, 2, true);
  bytes.copyWithin(
    firstPageOffset + 16 + 56,
    firstPageOffset + 16,
    firstPageOffset + 16 + 56,
  );
  return bytes;
}

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
    provenance: {
      encoderPackageVersion: string;
      formatImplementation: string;
      referenceSample: string;
      referenceAssembly: string;
      datasetTitle: string;
      sourceUri: string;
      annotationRelease: string;
      annotationAssembly: string;
    };
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

  it("selects valid summary levels independently for unequal fragments", () => {
    const storage = {
      offset: 10_000n,
      encodedLength: 10n,
      decodedLength: 20n,
      codec: 3 as const,
      integrity: new Uint8Array(16),
    };
    const descriptor = {
      baseBinSpan: 1_000n,
      series: [
        ...[100n, 25n, 7n, 2n, 1n].map((binCount, level) => ({
          manifestIndex: 0,
          level,
          binSpan: 1_000n * 4n ** BigInt(level),
          firstBinStart: 0n,
          binCount,
          storage,
        })),
        ...[2n, 1n].map((binCount, level) => ({
          manifestIndex: 1,
          level,
          binSpan: 1_000n * 4n ** BigInt(level),
          firstBinStart: 0n,
          binCount,
          storage,
        })),
      ],
    };
    const selected = selectSummarySeries(descriptor, [0, 1], 0n, 100_000n, 5);
    expect(
      selected.map(({ manifestIndex, level }) => [manifestIndex, level]),
    ).toEqual([
      [0, 3],
      [1, 0],
    ]);
    expect(
      selectSummarySeries(descriptor, [0, 1], 0n, 100_000n, 1).map(
        ({ level }) => level,
      ),
    ).toEqual([4, 1]);
    expect(() => selectSummarySeries(descriptor, [2], 0n, 1n, 10)).toThrow(
      "does not cover",
    );
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
        } else if (failure.inputKind === "zstd-frame") {
          new FzstdDecompressor().decompress(
            bytes,
            readFileSync(new URL("format-v1.payload.raw", conformanceDirectory))
              .byteLength,
          );
        } else if (failure.inputKind === "archive-metadata") {
          decodeArchiveMetadata(bytes);
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
      pathMembership: false,
    });
    const info = await archive.info();
    expect(info).toMatchObject({
      formatVersion: 1,
      haplotypeSemantics: "anonymous-distinct-weighted-tile-paths",
      archiveBytes: BigInt(recordArchiveFixture.byteLength),
      extensions: recordArchiveMetadata.features.typeIds,
      namedLoci: {
        state: "present-populated",
        recordCount: BigInt(
          recordArchiveMetadata.features.namedLoci.recordCount,
        ),
      },
      provenance: {
        sourceGbzBytes: 73_920n,
        sourceGbzSha256:
          "1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788",
        ...recordArchiveMetadata.features.provenance,
      },
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
      fullBinStart: expect.any(Number),
      fullBinEnd: expect.any(Number),
      coveredBases: BigInt(recordArchiveMetadata.features.summary.coveredBases),
      tileCount: BigInt(recordArchiveMetadata.features.summary.tileCount),
    });
    expect(overview.bins[0]?.coverageFraction).toBeGreaterThan(0);
    expect(overview.bins[0]?.coverageFraction).toBeLessThanOrEqual(1);
    expect(overview.bins[0]?.nodeRecords).toBeGreaterThan(0n);
    expect(overview.bins[0]?.edgeRecords).toBeGreaterThan(0n);
    expect(overview.bins[0]?.gbwtRecords).toBeGreaterThan(0n);
    expect(overview.bins[0]?.occurrences).toBeGreaterThan(0n);
    const plan = await archive.planRegion({
      sample: "CHM13",
      contig: "chr6",
      start: 31_350_872,
      end: 31_351_896,
    });
    expect(plan).toMatchObject({
      sample: "CHM13",
      contig: "chr6",
      start: 31_350_872,
      end: 31_351_896,
      selectedChunks: 1,
    });
    expect(plan.ranges).toHaveLength(1);
    expect(plan.compressedBytes).toBeGreaterThan(0n);
    expect(plan.decodedBytes).toBeGreaterThan(plan.compressedBytes);
    expect(plan.ranges[0]?.offset).toBeGreaterThan(0n);
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
        3 +
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
      decodedFeatureBytes: 0,
      decodedFeatureEntries: 0,
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

  it("decodes production named-path membership and rejects directory corruption", async () => {
    expect(pathMembershipArchiveFixture.byteLength).toBe(
      pathMembershipExpected.archiveBytes,
    );
    expect(sha256(pathMembershipArchiveFixture)).toBe(
      pathMembershipExpected.archiveSha256,
    );
    const archive = await openPangenome(
      new MemoryRangeSource(pathMembershipArchiveFixture),
    );
    expect(archive.capabilities().pathMembership).toBe(true);
    expect((await archive.info()).pathMembership).toEqual({
      state: "present",
      pathCount: BigInt(pathMembershipExpected.catalogPathCount),
    });
    const result = await archive.pathMembership({
      ...pathMembershipExpected.query,
      trace: true,
    });
    const groups = result.tiles.flatMap((tile) => tile.groups);
    const memberships = groups.flatMap((group) => group.memberships);
    expect(result.tiles).toHaveLength(pathMembershipExpected.tiles);
    expect(groups).toHaveLength(pathMembershipExpected.groups);
    expect(memberships).toHaveLength(pathMembershipExpected.memberships);
    expect(result.paths).toHaveLength(pathMembershipExpected.referencedPaths);
    expect(
      groups.reduce((total, group) => total + group.occurrenceWeight, 0n),
    ).toBe(BigInt(pathMembershipExpected.occurrenceWeight));
    expect(
      memberships.reduce(
        (total, membership) => total + membership.multiplicity,
        0n,
      ),
    ).toBe(BigInt(pathMembershipExpected.membershipMultiplicity));
    const first = result.paths[0];
    expect(first).toBeDefined();
    expect({
      pathId: first?.pathId.toString(),
      canonicalName: first?.canonicalName,
      sample: first?.sample,
      contig: first?.contig,
      haplotype: first?.haplotype.toString(),
      fragment: first?.fragment.toString(),
      sense: first?.sense,
    }).toEqual(pathMembershipExpected.firstReferencedPath);
    expect(result.trace?.requestRanges.length).toBeGreaterThan(0);
    expect(await archive.pathCatalogInfo()).toEqual({
      pathCount: BigInt(pathMembershipExpected.catalogPathCount),
      recordsPerPage: 1_024,
      pageCount: 1,
      identitySource: "embedded-gbwt-da-bounded-lf-v1",
      identitySourceSha256:
        "1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788",
      membershipGroupCount: 79n,
      membershipOccurrenceTotal: 180n,
      membershipGroupUniquePathCountSum: 180n,
      codecDistribution: { deltaGroups: 79n, runGroups: 0n },
    });
    expect(await archive.pathById(first?.pathId ?? -1n)).toEqual(first);
    expect(await archive.pathById(10_000n)).toBeUndefined();
    const batched = await archive.pathsByIds([
      first?.pathId ?? -1n,
      first?.pathId ?? -1n,
      10_000n,
    ]);
    expect(batched).toEqual([first, first, undefined]);
    const aborted = new AbortController();
    aborted.abort();
    await expect(
      archive.pathsByIds([first?.pathId ?? -1n], {
        signal: aborted.signal,
      }),
    ).rejects.toThrow();
    const searched = await archive.searchPaths({
      sample: "GRCh38",
      limit: 1,
      trace: true,
    });
    expect(searched.paths).toHaveLength(1);
    expect(searched.paths.every((path) => path.sample === "GRCh38")).toBe(true);
    expect(searched.truncated).toBe(true);
    const region = await archive.query(pathMembershipExpected.query);
    const tileGroups = await archive.tilePathMemberships(
      region.tiles[0] as RegionTile,
    );
    expect(tileGroups.length).toBeGreaterThan(0);
    expect(tileGroups.every((group) => group.orientedNodes !== undefined)).toBe(
      true,
    );
    const combined = await archive.queryWithPathMembership(
      pathMembershipExpected.query,
    );
    expect(combined.region.tiles).toHaveLength(pathMembershipExpected.tiles);
    expect(combined.pathMembership.tiles).toHaveLength(
      pathMembershipExpected.tiles,
    );
    expect(combined.trace.graph).toBeDefined();
    expect(combined.trace.membership.dependencyRounds).toBeGreaterThanOrEqual(
      0,
    );
    expect(combined.trace.catalog.dependencyRounds).toBeGreaterThanOrEqual(0);
    await archive.close();

    const corruptBytes = pathMembershipArchiveFixture.slice();
    const magic = new TextEncoder().encode("PNGPMI01");
    let directoryOffset = -1;
    for (
      let offset = 0;
      offset <= corruptBytes.length - magic.length;
      offset += 1
    ) {
      if (magic.every((byte, index) => corruptBytes[offset + index] === byte)) {
        directoryOffset = offset;
        break;
      }
    }
    expect(directoryOffset).toBeGreaterThanOrEqual(0);
    const corruptionOffset = directoryOffset + 32;
    corruptBytes[corruptionOffset] = (corruptBytes[corruptionOffset] ?? 0) ^ 1;
    const corruptArchive = await openPangenome(
      new MemoryRangeSource(corruptBytes),
    );
    await expect(
      corruptArchive.pathMembership(pathMembershipExpected.query),
    ).rejects.toThrow(/path-membership directory/);
    await corruptArchive.close();

    const provenanceMismatch = await openPangenome(
      new MemoryRangeSource(mismatchedPathMembershipProvenance()),
    );
    await expect(provenanceMismatch.pathCatalogInfo()).rejects.toThrow(
      /differs from archive provenance/,
    );
    await provenanceMismatch.close();
  });

  it("enforces bounded path-membership decoding and preserves dual orientations", () => {
    expect(
      Buffer.from(
        traversalMembershipDigest(
          "sample",
          "chr1",
          100n,
          200n,
          new Uint8Array(16).fill(9),
          [2n, 4n, 6n],
        ),
      ).toString("hex"),
    ).toBe("670d47d546abb21bce595ee9813eb7a0");
    const bytes: number[] = [];
    const text = (value: string) =>
      bytes.push(...new TextEncoder().encode(value));
    const u32 = (value: number) => {
      for (let shift = 0; shift < 32; shift += 8)
        bytes.push((value >>> shift) & 0xff);
    };
    const u64 = (value: bigint) => {
      for (let shift = 0n; shift < 64n; shift += 8n)
        bytes.push(Number((value >> shift) & 0xffn));
    };

    text("PNGPMT01");
    u32(1);
    u32(1);
    u64(100n);
    u64(200n);
    bytes.push(...new Uint8Array(16).fill(9));
    bytes.push(...new Uint8Array(16).fill(7));
    u64(2n);
    u64(1n);
    u64(8n);
    bytes.push(0, 2, 1, 1, 0, 0, 1, 1);
    const page = decodeTileMembershipPage(new Uint8Array(bytes), 2n);
    expect(page.groups[0]?.uniquePathCount).toBe(1n);
    expect(page.groups[0]?.memberships).toEqual([
      { pathId: 1n, multiplicity: 1n, reversedRelativeToGroup: false },
      { pathId: 1n, multiplicity: 1n, reversedRelativeToGroup: true },
    ]);
    const duplicatePair = new Uint8Array(bytes);
    duplicatePair[duplicatePair.length - 1] = 0;
    expect(() => decodeTileMembershipPage(duplicatePair, 2n)).toThrow(
      /path\/orientation pairs/,
    );

    const oversizedMemberships = new Uint8Array(bytes.slice(0, 88));
    const oversizedCount: number[] = [];
    let remaining = 250_001n;
    do {
      let byte = Number(remaining & 0x7fn);
      remaining >>= 7n;
      if (remaining !== 0n) byte |= 0x80;
      oversizedCount.push(byte);
    } while (remaining !== 0n);
    new DataView(oversizedMemberships.buffer).setBigUint64(
      80,
      BigInt(1 + oversizedCount.length),
      true,
    );
    const oversizedPage = new Uint8Array(
      oversizedMemberships.length + 1 + oversizedCount.length,
    );
    oversizedPage.set(oversizedMemberships);
    oversizedPage[88] = 0;
    oversizedPage.set(oversizedCount, 89);
    expect(() => decodeTileMembershipPage(oversizedPage, 300_000n)).toThrow(
      /entry count exceeds its bound/,
    );

    const catalog: number[] = [];
    catalog.push(...new TextEncoder().encode("PNGPCP01"));
    const catalogU32 = (value: number) => {
      for (let shift = 0; shift < 32; shift += 8)
        catalog.push((value >>> shift) & 0xff);
    };
    catalogU32(1);
    catalogU32(2);
    catalog.push(...new Uint8Array(8));
    catalog.push(0, 2, 0xc3, 0xa9, 0, 0, 0, 0, 0, 0, 0);
    catalog.push(1, 1, 0xa9, 0, 0, 0, 0, 0, 0, 0);
    expect(() => decodePathCatalogPage(new Uint8Array(catalog))).toThrow(
      /front-coded prefix/,
    );

    const descriptor = new Uint8Array(32);
    descriptor.set(new TextEncoder().encode("PNGPMD01"));
    const descriptorView = new DataView(descriptor.buffer);
    descriptorView.setUint32(8, 1, true);
    descriptorView.setUint32(12, 1_024, true);
    descriptorView.setBigUint64(16, 1n, true);
    descriptorView.setUint32(24, 1_000_000, true);
    descriptorView.setUint32(28, 1, true);
    expect(() =>
      decodePathMembershipDescriptor(descriptor, 64n, 1_000n),
    ).toThrow(/descriptor length/);
  });

  it("keeps semantic results deterministic across payload completion order", async () => {
    const query = {
      sample: "GRCh38",
      contig: "chr19",
      start: 54_816_468,
      end: 54_830_778,
      context: 100,
    } as const;
    const run = async (reverse: boolean) => {
      const archive = await openPangenome({
        source: new MemoryRangeSource(micbKirArchiveFixture),
        decompressor: new CompletionOrderDecompressor(reverse),
      });
      const streamed: number[] = [];
      for await (const tile of archive.queryTiles(query))
        streamed.push(tile.start);
      archive.clearCaches();
      const result = await archive.query({ ...query, trace: true });
      await archive.close();
      return {
        streamed,
        ordered: result.tiles.map((tile) => tile.start),
        graphHash: result.trace?.canonicalHash,
        decompressionWallMs: result.trace?.decompressionMs ?? 0,
        decompressionTaskMs: result.trace?.decompressionTaskMs ?? 0,
        tileHashes: result.tiles.map(canonicalHaplotypeTileHash),
        referenceTraversal: Array.from(result.graph.referenceTraversal, String),
      };
    };
    const forward = await run(false);
    const reverse = await run(true);
    expect(forward.ordered).toEqual([...forward.ordered].sort((a, b) => a - b));
    expect(reverse.ordered).toEqual(forward.ordered);
    expect(reverse.graphHash).toBe(forward.graphHash);
    expect(forward.decompressionTaskMs).toBeGreaterThanOrEqual(
      forward.decompressionWallMs,
    );
    expect(reverse.decompressionTaskMs).toBeGreaterThanOrEqual(
      reverse.decompressionWallMs,
    );
    expect(reverse.tileHashes).toEqual(forward.tileHashes);
    expect(reverse.referenceTraversal).toEqual(forward.referenceTraversal);
    expect([...reverse.streamed].sort((a, b) => a - b)).toEqual(
      [...forward.streamed].sort((a, b) => a - b),
    );
  });

  it("deduplicates multiple logical entries for one physical payload", async () => {
    const query = {
      sample: "CHM13",
      contig: "chr6",
      start: 31_350_872,
      end: 31_351_896,
      trace: true,
    } as const;
    const original = await openPangenome(
      new MemoryRangeSource(recordArchiveFixture),
    );
    const duplicated = await openPangenome(
      new MemoryRangeSource(duplicateGoldenLogicalEntry()),
    );
    const [left, right] = await Promise.all([
      original.query(query),
      duplicated.query(query),
    ]);
    expect(right.tiles).toHaveLength(1);
    expect(right.trace?.canonicalHash).toBe(left.trace?.canonicalHash);
    expect(Array.from(right.graph.referenceTraversal, String)).toEqual(
      Array.from(left.graph.referenceTraversal, String),
    );
    await original.close();
    await duplicated.close();
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
    const unknown = bytes.slice();
    const unknownView = new DataView(unknown.buffer);
    const extensionOffset = Number(unknownView.getBigUint64(48, true));
    unknown.set(
      new TextEncoder().encode("unknown-meta-v1-"),
      extensionOffset + 32,
    );
    const optional = await openPangenome(new MemoryRangeSource(unknown));
    expect(optional.references()).toEqual(
      fixture.expected.references.map((reference) =>
        expect.objectContaining(reference),
      ),
    );
    await optional.close();

    const required = unknown.slice();
    const view = new DataView(required.buffer);
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
