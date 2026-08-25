import { describe, expect, it } from "vitest";
import {
  NotImplementedError,
  openPangenome,
  validateArchiveRange,
  validateRegionQuery,
} from "../src/reader/index.js";

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
});
