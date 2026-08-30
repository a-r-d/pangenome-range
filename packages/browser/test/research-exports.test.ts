import { describe, expect, it } from "vitest";
import type {
  NamedSourcePath,
  NamedTraversalGroup,
  RegionTile,
} from "../src/reader/types.js";
import {
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
  const ids = BigUint64Array.from([1n, 2n]);
  const sequenceOffsets = Uint32Array.from([0, 3, 7]);
  const sequenceBytes = new TextEncoder().encode("ACGATGC");
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
