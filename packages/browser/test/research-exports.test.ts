import { describe, expect, it } from "vitest";
import type {
  NamedSourcePath,
  NamedTraversalGroup,
  RegionTile,
} from "../src/reader/types.js";
import {
  LocalTraversalExportError,
  localTraversalFasta,
  localTraversalSequence,
  namedPathMembershipTsv,
} from "../src/viewer/research-exports.js";

describe("research exports", () => {
  it("reconstructs mixed forward and reverse node sequence", () => {
    const tile = makeTile();
    expect(localTraversalSequence(tile, [2n, 5n])).toBe("ACGGCAT");
    const fasta = localTraversalFasta({
      archiveIdentity: "sha256:test",
      tile,
      traversalDigest: "aabb",
      occurrenceWeight: 3n,
      orientedNodes: [2n, 5n],
    });
    expect(fasta).toContain(
      ">local_traversal archive=sha256:test interval=ref:chr1:10-20 traversal_digest=aabb occurrence_weight=3 scope=tile-local-not-complete-assembly-path",
    );
    expect(fasta).toContain("\nACGGCAT\n");
  });

  it("supports the complete uppercase and lowercase IUPAC DNA complement", () => {
    const sequence = "ACGTRYSWKMBDHVNacgtryswkmbdhvn";
    const tile = makeTileFromSequences([sequence]);
    expect(localTraversalSequence(tile, [3n])).toBe(
      "nbdhvkmwsryacgtNBDHVKMWSRYACGT",
    );
  });

  it("preserves forward bytes exactly across multiple nodes", () => {
    const tile = makeTileFromSequences(["aCgTRy", "SWkmbDHvN"]);
    expect(localTraversalSequence(tile, [2n, 4n])).toBe("aCgTRySWkmbDHvN");
  });

  it("reconstructs mixed forward and reverse IUPAC nodes", () => {
    const tile = makeTileFromSequences(["ACry", "BDhv"]);
    expect(localTraversalSequence(tile, [2n, 5n])).toBe("ACrybdHV");
  });

  it.each([
    ["U", 0x55],
    ["u", 0x75],
    ["unsupported ASCII", 0x3f],
    ["invalid UTF-8", 0xff],
  ])("fails closed for %s bytes", (_label, byte) => {
    const tile = makeTileFromBytes([Uint8Array.of(0x41, byte, 0x4e)]);
    expect(() => localTraversalSequence(tile, [2n])).toThrowError(
      LocalTraversalExportError,
    );
    try {
      localTraversalSequence(tile, [2n]);
    } catch (cause) {
      expect(cause).toMatchObject({
        code: "unsupported-sequence-byte",
        byteValue: byte,
      });
      expect((cause as Error).message).not.toContain("replacement");
    }
  });

  it("never substitutes unsupported bytes with N", () => {
    const tile = makeTileFromBytes([Uint8Array.of(0x41, 0x3f, 0x54)]);
    expect(() => localTraversalSequence(tile, [3n])).toThrow(
      "unsupported DNA byte 0x3f",
    );
  });

  it("wraps FASTA at 80 columns and preserves the tile-local header", () => {
    const tile = makeTileFromSequences(["A".repeat(81)]);
    const fasta = localTraversalFasta({
      archiveIdentity: "sha256:test archive",
      tile,
      traversalDigest: "aabb",
      occurrenceWeight: 1n,
      orientedNodes: [2n],
    });
    const lines = fasta.trimEnd().split("\n");
    expect(lines[0]).toContain("archive=sha256:test_archive");
    expect(lines[0]).toContain("scope=tile-local-not-complete-assembly-path");
    expect(lines.slice(1).map((line) => line.length)).toEqual([80, 1]);
    expect(lines.slice(1).join("")).toBe("A".repeat(81));
  });

  it("exports exact membership fields as TSV", () => {
    const group: NamedTraversalGroup = {
      traversalDigest: Uint8Array.of(0xaa),
      occurrenceWeight: 2n,
      uniquePathCount: 1n,
      memberships: [
        {
          pathId: 7n,
          multiplicity: 2n,
          reversedRelativeToGroup: true,
        },
      ],
    };
    const paths: NamedSourcePath[] = [
      {
        pathId: 7n,
        canonicalName: "sample#2#contig#fragment=9",
        sample: "sample",
        contig: "contig",
        haplotype: 2n,
        fragment: 9n,
        sense: "haplotype",
      },
    ];
    const tsv = namedPathMembershipTsv({
      tile: makeTile(),
      traversalDigest: "aa",
      group,
      paths,
    });
    expect(tsv.split("\n")[0]).toContain("traversal digest");
    expect(tsv).toContain(
      "aa\tref\tchr1\t10\t20\t7\tsample#2#contig#fragment=9\tsample\tcontig\t2\t9\thaplotype\t2\treverse",
    );
  });
});

function makeTile(): RegionTile {
  return makeTileFromSequences(["ACG", "ATGC"]);
}

function makeTileFromSequences(sequences: readonly string[]): RegionTile {
  return makeTileFromBytes(
    sequences.map((sequence) => new TextEncoder().encode(sequence)),
  );
}

function makeTileFromBytes(sequences: readonly Uint8Array[]): RegionTile {
  const ids = BigUint64Array.from(
    sequences.map((_sequence, index) => BigInt(index + 1)),
  );
  const sequenceOffsets = new Uint32Array(sequences.length + 1);
  let byteLength = 0;
  for (const [index, sequence] of sequences.entries()) {
    byteLength += sequence.length;
    sequenceOffsets[index + 1] = byteLength;
  }
  const sequenceBytes = new Uint8Array(byteLength);
  let offset = 0;
  for (const sequence of sequences) {
    sequenceBytes.set(sequence, offset);
    offset += sequence.length;
  }
  return {
    reference: { sample: "ref", contig: "chr1", start: 0, end: 100 },
    coreStart: 10,
    coreEnd: 20,
    start: 10,
    end: 20,
    semantics: "anonymous-distinct-weighted-tile-paths",
    nodes: { ids, sequenceOffsets, sequenceBytes },
    topology: { from: new BigUint64Array(), to: new BigUint64Array() },
    haplotypes: {
      kind: "weighted-traversals",
      traversalOffsets: Uint32Array.from([0]),
      orientedNodes: new BigUint64Array(),
      weights: new BigUint64Array(),
    },
    provenance: {
      archiveOffset: 1n,
      compressedBytes: 1,
      uncompressedBytes: 1,
      codec: "none",
    },
    archiveOffset: 1n,
    encodedLength: 1,
    nodeIds: ids,
    nodeSequenceOffsets: sequenceOffsets,
    nodeSequences: sequenceBytes,
    edges: new BigUint64Array(),
    referenceTraversal: new BigUint64Array(),
    traversalOffsets: Uint32Array.from([0]),
    traversalNodes: new BigUint64Array(),
    traversalWeights: new BigUint64Array(),
  };
}
