import { describe, expect, it } from "vitest";
import {
  detectArchiveVersion,
  NotImplementedError,
  openPangenome,
  UnsupportedArchiveVersionError,
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
});
