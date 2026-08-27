export const NAMED_LOCI_TYPE_ID = "named-loci-v1---";
export const SUMMARY_PYRAMID_TYPE_ID = "summary-pyr-v1--";
export const ARCHIVE_METADATA_TYPE_ID = "archive-meta-v1-";

const FEATURE_VERSION = 1;
const MAX_DESCRIPTOR_BYTES = 16 * 1024 * 1024;
const MAX_PAGE_BYTES = 64 * 1024 * 1024;
const MAX_LOCUS_PAGES = 65_536;
const MAX_LOCUS_RECORDS = 65_536;
const MAX_SUMMARY_SERIES = 65_536;
const MAX_SUMMARY_BINS = 1024 * 1024;
const SUMMARY_BIN_BYTES = 64;
const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);
const textDecoder = new TextDecoder("utf-8", { fatal: true });
const textEncoder = new TextEncoder();

export class FeatureDecodeError extends Error {}

export type FeatureCodec = 0 | 1 | 3 | 6;

export interface StoredFeaturePage {
  offset: bigint;
  encodedLength: bigint;
  decodedLength: bigint;
  codec: FeatureCodec;
  integrity: Uint8Array;
}

export interface LocusPageDescriptor {
  firstKey: string;
  lastKey: string;
  recordCount: bigint;
  storage: StoredFeaturePage;
}

export interface NamedLociDescriptor {
  annotationSha256: Uint8Array;
  annotationName: string;
  recordCount: bigint;
  pages: LocusPageDescriptor[];
}

export interface DecodedLocusRecord {
  normalizedKey: string;
  matchedName: string;
  displayName: string;
  stableId: string;
  featureType: string;
  sample: string;
  contig: string;
  start: bigint;
  end: bigint;
  strand: 0 | 1 | 2;
}

export interface SummarySeriesDescriptor {
  manifestIndex: number;
  level: number;
  binSpan: bigint;
  firstBinStart: bigint;
  binCount: bigint;
  storage: StoredFeaturePage;
}

export interface SummaryPyramidDescriptor {
  baseBinSpan: bigint;
  series: SummarySeriesDescriptor[];
}

export function selectSummarySeries(
  descriptor: SummaryPyramidDescriptor,
  manifestIndexes: readonly number[],
  queryStart: bigint,
  queryEnd: bigint,
  maxBins: number,
): SummarySeriesDescriptor[] {
  const seriesByManifest = manifestIndexes.map((manifestIndex) =>
    descriptor.series.filter(
      (series) => series.manifestIndex === manifestIndex,
    ),
  );
  if (seriesByManifest.some((series) => series.length === 0)) {
    fail("summary descriptor does not cover the selected references");
  }
  const selectedIndexes = seriesByManifest.map(() => 0);
  const count = (series: SummarySeriesDescriptor): number => {
    const seriesBytes = series.binCount * series.binSpan;
    if (seriesBytes > 0xffff_ffff_ffff_ffffn)
      fail("summary series end overflows u64");
    const seriesEnd = checkedAdd(
      series.firstBinStart,
      seriesBytes,
      "summary series end",
    );
    const start =
      queryStart > series.firstBinStart ? queryStart : series.firstBinStart;
    const end = queryEnd < seriesEnd ? queryEnd : seriesEnd;
    if (start >= end) return 0;
    const first = (start - series.firstBinStart) / series.binSpan;
    const last = (end - 1n - series.firstBinStart) / series.binSpan;
    return safeNumber(last - first + 1n, "selected summary bin count");
  };
  const total = () =>
    seriesByManifest.reduce((sum, series, index) => {
      const selected = series[selectedIndexes[index] as number];
      if (selected === undefined) fail("summary series is missing");
      return sum + count(selected);
    }, 0);
  while (total() > maxBins) {
    let bestManifest = -1;
    let bestReduction = 0;
    for (let index = 0; index < seriesByManifest.length; index += 1) {
      const series = seriesByManifest[index] as SummarySeriesDescriptor[];
      const selectedIndex = selectedIndexes[index] as number;
      const current = series[selectedIndex];
      const next = series[selectedIndex + 1];
      if (current === undefined || next === undefined) continue;
      const reduction = count(current) - count(next);
      if (reduction > bestReduction) {
        bestReduction = reduction;
        bestManifest = index;
      }
    }
    if (bestManifest < 0) break;
    selectedIndexes[bestManifest] =
      (selectedIndexes[bestManifest] as number) + 1;
  }
  return seriesByManifest.map((series, index) => {
    const selected = series[selectedIndexes[index] as number];
    if (selected === undefined) fail("summary series is missing");
    return selected;
  });
}

export interface DecodedSummaryBin {
  coveredBases: bigint;
  tileCount: bigint;
  encodedBytes: bigint;
  decodedBytes: bigint;
  nodeRecords: bigint;
  edgeRecords: bigint;
  gbwtRecords: bigint;
  occurrences: bigint;
}

export interface DecodedArchiveMetadata {
  sourceGbzBytes: bigint;
  sourceGbzSha256: Uint8Array;
  encoderPackageVersion: string;
  formatImplementation: string;
  regionalWindowSize: bigint;
  constructionContext: bigint;
  payloadCodec: FeatureCodec;
  referenceSample?: string;
  referenceAssembly?: string;
  datasetTitle?: string;
  datasetDescription?: string;
  sourceUri?: string;
  annotationFilename?: string;
  annotationSha256?: Uint8Array;
  annotationRelease?: string;
  annotationAssembly?: string;
}

class Reader {
  readonly #bytes: Uint8Array;
  readonly #view: DataView;
  #position = 0;

  constructor(bytes: Uint8Array) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  get remaining(): number {
    return this.#bytes.byteLength - this.#position;
  }

  take(length: number): Uint8Array {
    if (!Number.isSafeInteger(length) || length < 0)
      fail("invalid feature length");
    const end = this.#position + length;
    if (!Number.isSafeInteger(end) || end > this.#bytes.byteLength) {
      fail("unexpected end of feature data");
    }
    const value = this.#bytes.subarray(this.#position, end);
    this.#position = end;
    return value;
  }

  u8(): number {
    return this.take(1)[0] as number;
  }

  u32(): number {
    const position = this.#position;
    this.take(4);
    return this.#view.getUint32(position, true);
  }

  u64(): bigint {
    const position = this.#position;
    this.take(8);
    return this.#view.getBigUint64(position, true);
  }

  string(): string {
    try {
      return textDecoder.decode(
        this.take(safeNumber(this.u64(), "feature string length")),
      );
    } catch (error) {
      if (error instanceof FeatureDecodeError) throw error;
      fail(`invalid UTF-8 feature string: ${String(error)}`);
    }
  }

  finish(): void {
    if (this.remaining !== 0) fail(`${this.remaining} trailing feature bytes`);
  }
}

export function normalizeLocusKey(value: string): string {
  return value
    .replace(/^[\t\n\v\f\r ]+|[\t\n\v\f\r ]+$/g, "")
    .replace(/[A-Z]/g, (character) => character.toLowerCase());
}

export function decodeArchiveMetadata(
  bytes: Uint8Array,
): DecodedArchiveMetadata {
  if (bytes.byteLength > 1024 * 1024)
    fail("archive metadata exceeds its limit");
  const reader = new Reader(bytes);
  if (
    textDecoder.decode(reader.take(8)) !== "PNGMET01" ||
    reader.u32() !== 1 ||
    reader.u32() !== 112
  ) {
    fail("invalid archive metadata header");
  }
  const sourceGbzBytes = reader.u64();
  const sourceGbzSha256 = reader.take(32).slice();
  const regionalWindowSize = reader.u64();
  const constructionContext = reader.u64();
  const payloadCodec = reader.u8();
  if (![0, 1, 3, 6].includes(payloadCodec)) fail("invalid metadata codec");
  if (reader.u8() !== 2) fail("invalid metadata haplotype semantics");
  const annotationPresent = reader.u8();
  if (annotationPresent > 1 || reader.take(5).some((value) => value !== 0)) {
    fail("invalid archive metadata flags or reserved bytes");
  }
  const annotationBytes = reader.take(32).slice();
  const encoderPackageVersion = reader.string();
  const formatImplementation = reader.string();
  const fields = Array.from({ length: 8 }, () => reader.string());
  reader.finish();
  const annotationAllZero = annotationBytes.every((value) => value === 0);
  if (
    sourceGbzBytes === 0n ||
    sourceGbzSha256.every((value) => value === 0) ||
    regionalWindowSize === 0n ||
    constructionContext !== 100n ||
    encoderPackageVersion.length === 0 ||
    formatImplementation.length === 0 ||
    (annotationPresent === 0) !== annotationAllZero ||
    (annotationPresent === 1) !== (fields[5]?.length !== 0) ||
    (annotationPresent === 0 &&
      ((fields[6]?.length ?? 0) !== 0 || (fields[7]?.length ?? 0) !== 0))
  ) {
    fail("invalid archive metadata");
  }
  const optional = (value: string | undefined): string | undefined =>
    value === undefined || value.length === 0 ? undefined : value;
  const referenceSample = optional(fields[0]);
  const referenceAssembly = optional(fields[1]);
  const datasetTitle = optional(fields[2]);
  const datasetDescription = optional(fields[3]);
  const sourceUri = optional(fields[4]);
  const annotationRelease = optional(fields[6]);
  const annotationAssembly = optional(fields[7]);
  return {
    sourceGbzBytes,
    sourceGbzSha256,
    encoderPackageVersion,
    formatImplementation,
    regionalWindowSize,
    constructionContext,
    payloadCodec: payloadCodec as FeatureCodec,
    ...(referenceSample === undefined ? {} : { referenceSample }),
    ...(referenceAssembly === undefined ? {} : { referenceAssembly }),
    ...(datasetTitle === undefined ? {} : { datasetTitle }),
    ...(datasetDescription === undefined ? {} : { datasetDescription }),
    ...(sourceUri === undefined ? {} : { sourceUri }),
    ...(annotationPresent === 0
      ? {}
      : {
          annotationFilename: fields[5] as string,
          annotationSha256: annotationBytes,
        }),
    ...(annotationRelease === undefined ? {} : { annotationRelease }),
    ...(annotationAssembly === undefined ? {} : { annotationAssembly }),
  };
}

export function compareFeatureKeys(left: string, right: string): number {
  return compareBytes(textEncoder.encode(left), textEncoder.encode(right));
}

export function selectLocusPages(
  descriptor: NamedLociDescriptor,
  key: string,
  mode: "exact" | "prefix",
): LocusPageDescriptor[] {
  let low = 0;
  let high = descriptor.pages.length;
  while (low < high) {
    const middle = low + Math.floor((high - low) / 2);
    const page = descriptor.pages[middle];
    if (page === undefined)
      fail("named-locus binary search left its descriptor");
    if (compareFeatureKeys(page.lastKey, key) < 0) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  const first = descriptor.pages[low];
  if (first === undefined) return [];
  if (mode === "exact") {
    return compareFeatureKeys(first.firstKey, key) <= 0 ? [first] : [];
  }
  const selected: LocusPageDescriptor[] = [];
  for (let index = low; index < descriptor.pages.length; index += 1) {
    const page = descriptor.pages[index];
    if (page === undefined) fail("named-locus descriptor page is missing");
    if (
      compareFeatureKeys(page.firstKey, key) > 0 &&
      !page.firstKey.startsWith(key)
    ) {
      break;
    }
    selected.push(page);
  }
  return selected;
}

export function decodeNamedLociDescriptor(
  bytes: Uint8Array,
  dataOffset: bigint,
  sourceSize: bigint,
): NamedLociDescriptor {
  if (bytes.byteLength > MAX_DESCRIPTOR_BYTES)
    fail("named-locus descriptor is too large");
  const reader = new Reader(bytes);
  if (
    textDecoder.decode(reader.take(8)) !== "PNGLOC01" ||
    reader.u32() !== FEATURE_VERSION
  ) {
    fail("invalid named-locus descriptor header");
  }
  const pageCount = reader.u32();
  const recordCount = reader.u64();
  const annotationSha256 = reader.take(32).slice();
  const annotationName = reader.string();
  if (pageCount > MAX_LOCUS_PAGES) fail("named-locus page count is too large");
  const pages: LocusPageDescriptor[] = [];
  let total = 0n;
  let previousLast: string | undefined;
  for (let index = 0; index < pageCount; index += 1) {
    const firstKey = reader.string();
    const lastKey = reader.string();
    const pageRecords = reader.u64();
    const storage = readStoredPage(reader, dataOffset, sourceSize);
    if (
      firstKey.length === 0 ||
      compareFeatureKeys(lastKey, firstKey) < 0 ||
      (previousLast !== undefined &&
        compareFeatureKeys(firstKey, previousLast) <= 0) ||
      pageRecords === 0n
    ) {
      fail("invalid named-locus page ordering");
    }
    total = checkedAdd(total, pageRecords, "named-locus record count");
    previousLast = lastKey;
    pages.push({ firstKey, lastKey, recordCount: pageRecords, storage });
  }
  reader.finish();
  if ((recordCount === 0n) !== (pages.length === 0) || total !== recordCount) {
    fail("named-locus descriptor count mismatch");
  }
  return { annotationSha256, annotationName, recordCount, pages };
}

export function decodeLocusPage(bytes: Uint8Array): DecodedLocusRecord[] {
  if (bytes.byteLength > MAX_PAGE_BYTES) fail("named-locus page is too large");
  const reader = new Reader(bytes);
  if (
    textDecoder.decode(reader.take(8)) !== "PNGLPG01" ||
    reader.u32() !== FEATURE_VERSION
  ) {
    fail("invalid named-locus page header");
  }
  const count = reader.u32();
  if (
    count === 0 ||
    count > MAX_LOCUS_RECORDS ||
    count > Math.floor(reader.remaining / 72)
  ) {
    fail("invalid named-locus record count");
  }
  const records: DecodedLocusRecord[] = [];
  let previous: DecodedLocusRecord | undefined;
  for (let index = 0; index < count; index += 1) {
    const normalizedKey = reader.string();
    const matchedName = reader.string();
    const record: DecodedLocusRecord = {
      normalizedKey,
      matchedName,
      displayName: reader.string(),
      stableId: reader.string(),
      featureType: reader.string(),
      sample: reader.string(),
      contig: reader.string(),
      start: reader.u64(),
      end: reader.u64(),
      strand: reader.u8() as 0 | 1 | 2,
    };
    assertZero(reader.take(7), "named-locus reserved bytes");
    if (
      normalizedKey.length === 0 ||
      normalizedKey !== normalizeLocusKey(matchedName) ||
      record.displayName.length === 0 ||
      record.stableId.length === 0 ||
      record.featureType.length === 0 ||
      record.sample.length === 0 ||
      record.contig.length === 0 ||
      record.start >= record.end ||
      ![0, 1, 2].includes(record.strand) ||
      (previous !== undefined && compareLocusRecords(record, previous) < 0)
    ) {
      fail("invalid or unordered named-locus record");
    }
    previous = record;
    records.push(record);
  }
  reader.finish();
  return records;
}

export function decodeSummaryDescriptor(
  bytes: Uint8Array,
  dataOffset: bigint,
  sourceSize: bigint,
): SummaryPyramidDescriptor {
  if (bytes.byteLength > MAX_DESCRIPTOR_BYTES)
    fail("summary descriptor is too large");
  const reader = new Reader(bytes);
  if (
    textDecoder.decode(reader.take(8)) !== "PNGSUM01" ||
    reader.u32() !== FEATURE_VERSION
  ) {
    fail("invalid summary descriptor header");
  }
  const count = reader.u32();
  const baseBinSpan = reader.u64();
  assertZero(reader.take(8), "summary descriptor reserved bytes");
  if (count === 0 || count > MAX_SUMMARY_SERIES || baseBinSpan === 0n) {
    fail("invalid summary descriptor dimensions");
  }
  const series: SummarySeriesDescriptor[] = [];
  let previous: readonly [number, number] | undefined;
  for (let index = 0; index < count; index += 1) {
    const manifestIndex = reader.u32();
    const level = reader.u32();
    const binSpan = reader.u64();
    const firstBinStart = reader.u64();
    const binCount = reader.u64();
    const storage = readStoredPage(reader, dataOffset, sourceSize);
    if (
      binSpan === 0n ||
      binCount === 0n ||
      firstBinStart % binSpan !== 0n ||
      (previous !== undefined &&
        (manifestIndex < previous[0] ||
          (manifestIndex === previous[0] && level <= previous[1])))
    ) {
      fail("invalid summary series ordering");
    }
    previous = [manifestIndex, level];
    series.push({
      manifestIndex,
      level,
      binSpan,
      firstBinStart,
      binCount,
      storage,
    });
  }
  reader.finish();
  return { baseBinSpan, series };
}

export function decodeSummaryPage(
  bytes: Uint8Array,
  expected: SummarySeriesDescriptor,
): DecodedSummaryBin[] {
  if (bytes.byteLength > MAX_PAGE_BYTES) fail("summary page is too large");
  const reader = new Reader(bytes);
  if (
    textDecoder.decode(reader.take(8)) !== "PNGSMP01" ||
    reader.u32() !== FEATURE_VERSION ||
    reader.u32() !== SUMMARY_BIN_BYTES ||
    reader.u32() !== expected.manifestIndex ||
    reader.u32() !== expected.level ||
    reader.u64() !== expected.binSpan ||
    reader.u64() !== expected.firstBinStart
  ) {
    fail("summary page differs from its descriptor");
  }
  const count = reader.u64();
  if (
    count === 0n ||
    count !== expected.binCount ||
    count > BigInt(MAX_SUMMARY_BINS) ||
    count > BigInt(Math.floor(reader.remaining / SUMMARY_BIN_BYTES))
  ) {
    fail("summary page bin count mismatch");
  }
  const bins: DecodedSummaryBin[] = [];
  for (
    let index = 0;
    index < safeNumber(count, "summary bin count");
    index += 1
  ) {
    bins.push({
      coveredBases: reader.u64(),
      tileCount: reader.u64(),
      encodedBytes: reader.u64(),
      decodedBytes: reader.u64(),
      nodeRecords: reader.u64(),
      edgeRecords: reader.u64(),
      gbwtRecords: reader.u64(),
      occurrences: reader.u64(),
    });
  }
  reader.finish();
  return bins;
}

function readStoredPage(
  reader: Reader,
  dataOffset: bigint,
  sourceSize: bigint,
): StoredFeaturePage {
  const offset = reader.u64();
  const encodedLength = reader.u64();
  const decodedLength = reader.u64();
  const codec = reader.u8();
  assertZero(reader.take(7), "extension page reserved bytes");
  const integrity = reader.take(16).slice();
  const end = checkedAdd(offset, encodedLength, "extension page end");
  if (
    ![0, 1, 3, 6].includes(codec) ||
    encodedLength === 0n ||
    decodedLength === 0n ||
    encodedLength > BigInt(MAX_PAGE_BYTES) ||
    decodedLength > BigInt(MAX_PAGE_BYTES) ||
    offset < dataOffset ||
    end > sourceSize
  ) {
    fail("invalid extension page range");
  }
  return {
    offset,
    encodedLength,
    decodedLength,
    codec: codec as FeatureCodec,
    integrity,
  };
}

function compareLocusRecords(
  left: DecodedLocusRecord,
  right: DecodedLocusRecord,
): number {
  for (const [leftValue, rightValue] of [
    [left.normalizedKey, right.normalizedKey],
    [left.sample, right.sample],
    [left.contig, right.contig],
  ] as const) {
    const result = compareFeatureKeys(leftValue, rightValue);
    if (result !== 0) return result;
  }
  if (left.start !== right.start) return left.start < right.start ? -1 : 1;
  if (left.end !== right.end) return left.end < right.end ? -1 : 1;
  return compareFeatureKeys(left.stableId, right.stableId);
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const shared = Math.min(left.byteLength, right.byteLength);
  for (let index = 0; index < shared; index += 1) {
    const difference = (left[index] as number) - (right[index] as number);
    if (difference !== 0) return difference;
  }
  return left.byteLength - right.byteLength;
}

function assertZero(bytes: Uint8Array, label: string): void {
  if (bytes.some((byte) => byte !== 0)) fail(`${label} must be zero`);
}

function safeNumber(value: bigint, label: string): number {
  if (value < 0n || value > MAX_SAFE_BIGINT)
    fail(`${label} exceeds the safe integer range`);
  return Number(value);
}

function checkedAdd(left: bigint, right: bigint, label: string): bigint {
  const value = left + right;
  if (value > 0xffff_ffff_ffff_ffffn) fail(`${label} overflows u64`);
  return value;
}

function fail(message: string): never {
  throw new FeatureDecodeError(message);
}
