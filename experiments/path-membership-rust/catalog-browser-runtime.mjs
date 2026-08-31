import { decompress } from "/fzstd.mjs";
import { blake3 } from "/noble/blake3.js";

const textDecoder = new TextDecoder("utf-8", { fatal: true });
const catalogMagic = "PMPC0001";
const pageMagic = "PMCPAGE1";
const headerBytes = 64;
const directoryEntryBytes = 48;
const maxRecordsPerPage = 65536;
const maxPageBytes = 64 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

function ascii(bytes) {
  return String.fromCharCode(...bytes);
}

function u32(bytes, offset) {
  return new DataView(
    bytes.buffer,
    bytes.byteOffset,
    bytes.byteLength,
  ).getUint32(offset, true);
}

function u64(bytes, offset) {
  return new DataView(
    bytes.buffer,
    bytes.byteOffset,
    bytes.byteLength,
  ).getBigUint64(offset, true);
}

function safeNumber(value, field) {
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) {
    fail(`${field} exceeds JavaScript's safe integer range`);
  }
  return Number(value);
}

function equalBytes(left, right) {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function readVarint(bytes, state) {
  let value = 0n;
  for (let shift = 0n; shift <= 63n; shift += 7n) {
    if (state.cursor >= bytes.length) fail("truncated varint");
    const byte = bytes[state.cursor++];
    if (shift === 63n && byte > 1) fail("varint overflow");
    value |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return value;
  }
  return fail("varint overflow");
}

function decodeString(bytes, state, previous) {
  const prefix = safeNumber(readVarint(bytes, state), "string prefix");
  const suffixLength = safeNumber(
    readVarint(bytes, state),
    "string suffix length",
  );
  if (prefix > previous.length || state.cursor + suffixLength > bytes.length) {
    fail("invalid front-coded string");
  }
  const result = new Uint8Array(prefix + suffixLength);
  result.set(previous.subarray(0, prefix));
  result.set(bytes.subarray(state.cursor, state.cursor + suffixLength), prefix);
  state.cursor += suffixLength;
  return result;
}

function senseName(value) {
  const names = ["unknown", "generic", "reference", "haplotype"];
  if (value >= names.length) fail(`unknown path sense code ${value}`);
  return names[value];
}

function decodePage(raw, expectedFirst, expectedCount) {
  if (ascii(raw.subarray(0, 8)) !== pageMagic) fail("invalid page magic");
  const first = u64(raw, 8);
  const count = u32(raw, 16);
  if (
    first !== expectedFirst ||
    count !== expectedCount ||
    u32(raw, 20) !== 0
  ) {
    fail("page identity differs from directory");
  }
  const state = { cursor: 24 };
  let previousRaw = new Uint8Array();
  let previousSample = new Uint8Array();
  let previousContig = new Uint8Array();
  const paths = [];
  for (let index = 0; index < count; index += 1) {
    const rawName = decodeString(raw, state, previousRaw);
    const sample = decodeString(raw, state, previousSample);
    const contig = decodeString(raw, state, previousContig);
    const haplotype = readVarint(raw, state);
    const fragment = readVarint(raw, state);
    if (state.cursor >= raw.length) fail("truncated path sense");
    const pathSense = senseName(raw[state.cursor++]);
    paths.push({
      canonical_path_id: safeNumber(first + BigInt(index), "path ID"),
      raw_name: textDecoder.decode(rawName),
      sample: textDecoder.decode(sample),
      contig: textDecoder.decode(contig),
      haplotype: safeNumber(haplotype, "haplotype"),
      fragment: safeNumber(fragment, "fragment"),
      path_sense: pathSense,
    });
    previousRaw = rawName;
    previousSample = sample;
    previousContig = contig;
  }
  if (state.cursor !== raw.length) fail("page has trailing bytes");
  return paths;
}

async function rangeRead(url, offset, length, expectedFileBytes, expectedEtag) {
  const end = offset + BigInt(length) - 1n;
  const response = await fetch(url, {
    cache: "no-store",
    headers: { Range: `bytes=${offset}-${end}` },
  });
  if (response.status !== 206)
    fail(`expected 206, received ${response.status}`);
  if (response.headers.get("accept-ranges") !== "bytes")
    fail("missing Accept-Ranges");
  const expectedRange = `bytes ${offset}-${end}/${expectedFileBytes}`;
  if (response.headers.get("content-range") !== expectedRange) {
    fail("invalid Content-Range");
  }
  const etag = response.headers.get("etag");
  if (!etag) fail("missing ETag");
  if (expectedEtag !== undefined && etag !== expectedEtag) {
    fail("catalog object changed during range reads");
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length !== length) fail("range response length mismatch");
  return { bytes, etag };
}

function parseRoot(root, expectedFileBytes) {
  if (
    root.length < headerBytes ||
    ascii(root.subarray(0, 8)) !== catalogMagic
  ) {
    fail("invalid catalog header");
  }
  if (u32(root, 8) !== 1) fail("unsupported catalog version");
  const recordsPerPage = u32(root, 12);
  const pathCount = safeNumber(u64(root, 16), "path count");
  const pageCount = safeNumber(u64(root, 24), "page count");
  const directoryBytes = safeNumber(u64(root, 32), "directory bytes");
  const fileBytes = u64(root, 40);
  if (fileBytes !== BigInt(expectedFileBytes))
    fail("catalog file length mismatch");
  if (
    recordsPerPage === 0 ||
    recordsPerPage > maxRecordsPerPage ||
    pageCount !== Math.ceil(pathCount / recordsPerPage) ||
    directoryBytes !== pageCount * directoryEntryBytes ||
    root.length !== headerBytes + directoryBytes
  ) {
    fail("catalog root dimensions are invalid");
  }
  const directory = root.subarray(headerBytes);
  if (!equalBytes(blake3(directory).subarray(0, 16), root.subarray(48, 64))) {
    fail("catalog directory digest mismatch");
  }
  const entries = [];
  let expectedOffset = BigInt(root.length);
  for (let pageId = 0; pageId < pageCount; pageId += 1) {
    const offset = pageId * directoryEntryBytes;
    const recordCount = u32(directory, offset);
    const codec = directory[offset + 4];
    const pageOffset = u64(directory, offset + 8);
    const encodedLength = safeNumber(
      u64(directory, offset + 16),
      "encoded length",
    );
    const decodedLength = safeNumber(
      u64(directory, offset + 24),
      "decoded length",
    );
    const expectedCount =
      pageId + 1 === pageCount
        ? pathCount - pageId * recordsPerPage
        : recordsPerPage;
    if (
      directory.subarray(offset + 5, offset + 8).some((value) => value !== 0) ||
      (codec !== 0 && codec !== 1) ||
      recordCount !== expectedCount ||
      encodedLength === 0 ||
      encodedLength > maxPageBytes ||
      decodedLength < 24 ||
      decodedLength > maxPageBytes ||
      pageOffset !== expectedOffset
    ) {
      fail("invalid catalog directory entry");
    }
    expectedOffset += BigInt(encodedLength);
    entries.push({
      recordCount,
      codec,
      offset: pageOffset,
      encodedLength,
      decodedLength,
      digest: directory.slice(offset + 32, offset + 48),
    });
  }
  if (expectedOffset !== fileBytes) fail("catalog payload length mismatch");
  return { recordsPerPage, pathCount, entries };
}

function planRanges(entries, pageIds, maxRanges) {
  const ranges = pageIds.map((pageId) => ({
    offset: entries[pageId].offset,
    length: BigInt(entries[pageId].encodedLength),
  }));
  while (ranges.length > maxRanges) {
    let mergeAt = 0;
    let smallestGap = null;
    for (let index = 0; index + 1 < ranges.length; index += 1) {
      const gap =
        ranges[index + 1].offset -
        (ranges[index].offset + ranges[index].length);
      if (smallestGap === null || gap < smallestGap) {
        smallestGap = gap;
        mergeAt = index;
      }
    }
    const right = ranges.splice(mergeAt + 1, 1)[0];
    ranges[mergeAt].length =
      right.offset + right.length - ranges[mergeAt].offset;
  }
  return ranges;
}

function decodeEncodedPage(entry, pageId, recordsPerPage, encoded) {
  if (!equalBytes(blake3(encoded).subarray(0, 16), entry.digest)) {
    fail("catalog page digest mismatch");
  }
  const raw =
    entry.codec === 0
      ? encoded
      : decompress(encoded, new Uint8Array(entry.decodedLength));
  if (raw.length !== entry.decodedLength) fail("decoded page length mismatch");
  return decodePage(raw, BigInt(pageId * recordsPerPage), entry.recordCount);
}

export async function runCatalogBenchmark(config) {
  const started = performance.now();
  const rootRead = await rangeRead(
    config.url,
    0n,
    config.rootBytes,
    config.fileBytes,
  );
  const parsed = parseRoot(rootRead.bytes, config.fileBytes);
  const queryIds = [...new Set(config.queryIds)].sort(
    (left, right) => left - right,
  );
  if (
    queryIds.length === 0 ||
    queryIds.some(
      (id) => !Number.isSafeInteger(id) || id < 0 || id >= parsed.pathCount,
    )
  ) {
    fail("query path IDs are empty or outside the catalog");
  }
  const pageIds = [
    ...new Set(queryIds.map((id) => Math.floor(id / parsed.recordsPerPage))),
  ];
  const ranges = planRanges(parsed.entries, pageIds, config.maxDataRanges);
  const fetched = await Promise.all(
    ranges.map(async (range) => ({
      ...range,
      bytes: (
        await rangeRead(
          config.url,
          range.offset,
          Number(range.length),
          config.fileBytes,
          rootRead.etag,
        )
      ).bytes,
    })),
  );
  const fetchedWallMs = performance.now() - started;

  const decodeOnce = () => {
    const pages = new Map();
    for (const pageId of pageIds) {
      const entry = parsed.entries[pageId];
      const range = fetched.find(
        (candidate) =>
          entry.offset >= candidate.offset &&
          entry.offset + BigInt(entry.encodedLength) <=
            candidate.offset + candidate.length,
      );
      if (!range) fail("range plan does not cover selected page");
      const start = Number(entry.offset - range.offset);
      const encoded = range.bytes.subarray(start, start + entry.encodedLength);
      pages.set(
        pageId,
        decodeEncodedPage(entry, pageId, parsed.recordsPerPage, encoded),
      );
    }
    return queryIds.map(
      (id) =>
        pages.get(Math.floor(id / parsed.recordsPerPage))[
          id % parsed.recordsPerPage
        ],
    );
  };

  const firstDecodeStarted = performance.now();
  const records = decodeOnce();
  const firstDecodeWallMs = performance.now() - firstDecodeStarted;
  const repeatedStarted = performance.now();
  for (let iteration = 0; iteration < config.iterations; iteration += 1) {
    decodeOnce();
  }
  const repeatedDecodeWallMs = performance.now() - repeatedStarted;
  const dataBytes = ranges.reduce(
    (total, range) => total + Number(range.length),
    0,
  );
  return {
    records,
    recordsPerPage: parsed.recordsPerPage,
    pageCount: parsed.entries.length,
    selectedPages: pageIds.length,
    ranges: [
      { offset: 0, length: config.rootBytes },
      ...ranges.map((range) => ({
        offset: Number(range.offset),
        length: Number(range.length),
      })),
    ],
    totalBytes: config.rootBytes + dataBytes,
    fetchedWallMs,
    firstDecodeWallMs,
    iterations: config.iterations,
    repeatedDecodeWallMs,
    perDecodeMs: repeatedDecodeWallMs / config.iterations,
  };
}
