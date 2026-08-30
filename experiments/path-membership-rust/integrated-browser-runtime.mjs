import { decompress } from "/fzstd.mjs";
import { blake3 } from "/noble/blake3.js";

const decoder = new TextDecoder("utf-8", { fatal: true });

function fail(message) {
  throw new Error(message);
}

function view(bytes) {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}

function u32(bytes, offset) {
  return view(bytes).getUint32(offset, true);
}

function u64(bytes, offset) {
  return view(bytes).getBigUint64(offset, true);
}

function safe(value, field) {
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) fail(`${field} is unsafe`);
  return Number(value);
}

function ascii(bytes) {
  return String.fromCharCode(...bytes);
}

function equal(left, right) {
  return left.length === right.length && left.every((v, i) => v === right[i]);
}

function varint(bytes, state) {
  let value = 0n;
  for (let shift = 0n; shift <= 63n; shift += 7n) {
    if (state.cursor >= bytes.length) fail("truncated varint");
    const byte = bytes[state.cursor++];
    value |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return value;
  }
  return fail("varint overflow");
}

function string(bytes, state) {
  const length = safe(u64(bytes, state.cursor), "string length");
  state.cursor += 8;
  const end = state.cursor + length;
  if (end > bytes.length) fail("truncated string");
  const result = decoder.decode(bytes.subarray(state.cursor, end));
  state.cursor = end;
  return result;
}

function frontString(bytes, state, previous) {
  const prefix = safe(varint(bytes, state), "string prefix");
  const suffixLength = safe(varint(bytes, state), "string suffix");
  if (prefix > previous.length || state.cursor + suffixLength > bytes.length) {
    fail("invalid front-coded string");
  }
  const result = new Uint8Array(prefix + suffixLength);
  result.set(previous.subarray(0, prefix));
  result.set(bytes.subarray(state.cursor, state.cursor + suffixLength), prefix);
  state.cursor += suffixLength;
  return result;
}

async function readRange(url, offset, length, fileBytes, expectedEtag) {
  const end = offset + BigInt(length) - 1n;
  const response = await fetch(url, {
    cache: "no-store",
    headers: { Range: `bytes=${offset}-${end}` },
  });
  if (response.status !== 206) fail(`expected 206, got ${response.status}`);
  if (response.headers.get("accept-ranges") !== "bytes")
    fail("missing Accept-Ranges");
  if (
    response.headers.get("content-range") !==
    `bytes ${offset}-${end}/${fileBytes}`
  ) {
    fail("invalid Content-Range");
  }
  const etag = response.headers.get("etag");
  if (!etag || (expectedEtag !== undefined && etag !== expectedEtag)) {
    fail("archive identity changed");
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length !== length) fail("short range response");
  return { bytes, etag };
}

function extensionPage(bytes, state) {
  const offset = u64(bytes, state.cursor);
  const encodedLength = safe(u64(bytes, state.cursor + 8), "encoded length");
  const decodedLength = safe(u64(bytes, state.cursor + 16), "decoded length");
  const codec = bytes[state.cursor + 24];
  if (
    bytes
      .subarray(state.cursor + 25, state.cursor + 32)
      .some((value) => value !== 0)
  ) {
    fail("nonzero page reserved bytes");
  }
  const digest = bytes.slice(state.cursor + 32, state.cursor + 48);
  state.cursor += 48;
  return { offset, encodedLength, decodedLength, codec, digest };
}

function parseArchiveRoot(bytes, fileBytes) {
  if (ascii(bytes.subarray(0, 8)) !== "PNGRNG01" || u32(bytes, 8) !== 1) {
    fail("invalid archive header");
  }
  const extensionOffset = safe(u64(bytes, 48), "extension offset");
  const extensionLength = safe(u64(bytes, 56), "extension length");
  const extension = bytes.subarray(
    extensionOffset,
    extensionOffset + extensionLength,
  );
  if (
    ascii(extension.subarray(0, 8)) !== "PNGEXT01" ||
    u32(extension, 8) !== 1
  ) {
    fail("invalid extension directory");
  }
  const count = safe(u64(extension, 16), "extension count");
  for (let index = 0; index < count; index += 1) {
    const offset = 32 + index * 64;
    if (ascii(extension.subarray(offset, offset + 16)) !== "path-members-v1-")
      continue;
    const entry = {
      codec: extension[offset + 20],
      offset: u64(extension, offset + 24),
      encodedLength: safe(
        u64(extension, offset + 32),
        "descriptor encoded length",
      ),
      decodedLength: safe(
        u64(extension, offset + 40),
        "descriptor decoded length",
      ),
      digest: extension.slice(offset + 48, offset + 64),
    };
    if (entry.offset + BigInt(entry.encodedLength) > BigInt(fileBytes)) {
      fail("descriptor outside archive");
    }
    return entry;
  }
  return fail("archive has no path-membership extension");
}

function decodeStored(entry, encoded) {
  if (!equal(blake3(encoded).subarray(0, 16), entry.digest))
    fail("page digest mismatch");
  const raw =
    entry.codec === 0
      ? encoded
      : decompress(encoded, new Uint8Array(entry.decodedLength));
  if (raw.length !== entry.decodedLength) fail("decoded length mismatch");
  return raw;
}

function parseDescriptor(bytes, query) {
  if (ascii(bytes.subarray(0, 8)) !== "PNGPMD01" || u32(bytes, 8) !== 1) {
    fail("invalid membership descriptor");
  }
  const recordsPerPage = u32(bytes, 12);
  const pathCount = safe(u64(bytes, 16), "path count");
  const catalogCount = u32(bytes, 24);
  const tileCount = u32(bytes, 28);
  const state = { cursor: 32 };
  const catalog = [];
  for (let index = 0; index < catalogCount; index += 1) {
    const firstPathId = safe(u64(bytes, state.cursor), "first path ID");
    const recordCount = safe(
      u64(bytes, state.cursor + 8),
      "catalog record count",
    );
    state.cursor += 16;
    catalog.push({ firstPathId, recordCount, ...extensionPage(bytes, state) });
  }
  let selected;
  for (let index = 0; index < tileCount; index += 1) {
    const sample = string(bytes, state);
    const contig = string(bytes, state);
    const coreStart = safe(u64(bytes, state.cursor), "core start");
    const coreEnd = safe(u64(bytes, state.cursor + 8), "core end");
    const groupCount = safe(u64(bytes, state.cursor + 16), "group count");
    state.cursor += 24;
    const page = extensionPage(bytes, state);
    if (
      sample === query.sample &&
      contig === query.contig &&
      coreStart === query.start &&
      coreEnd === query.end
    ) {
      selected = { sample, contig, coreStart, coreEnd, groupCount, ...page };
    }
  }
  if (state.cursor !== bytes.length || !selected)
    fail("membership tile not found");
  return { recordsPerPage, pathCount, catalog, selected };
}

function decodeMemberships(bytes, pathCount) {
  const state = { cursor: 0 };
  const codec = bytes[state.cursor++];
  const expected = safe(varint(bytes, state), "membership count");
  const result = [];
  let previous = 0n;
  if (codec === 0) {
    for (let index = 0; index < expected; index += 1) {
      const pathId = previous + varint(bytes, state);
      const multiplicity = varint(bytes, state);
      const reverse = bytes[state.cursor++];
      result.push({
        pathId: safe(pathId, "path ID"),
        multiplicity: safe(multiplicity, "multiplicity"),
        reverse: reverse !== 0,
      });
      previous = pathId;
    }
  } else if (codec === 1) {
    const records = safe(varint(bytes, state), "run count");
    for (let index = 0; index < records; index += 1) {
      const kind = bytes[state.cursor++];
      const pathId = previous + varint(bytes, state);
      const value = safe(varint(bytes, state), "run value");
      const reverse = bytes[state.cursor++] !== 0;
      if (kind === 0) {
        result.push({
          pathId: safe(pathId, "path ID"),
          multiplicity: value,
          reverse,
        });
        previous = pathId;
      } else {
        for (let offset = 0; offset < value; offset += 1) {
          result.push({
            pathId: safe(pathId + BigInt(offset), "path ID"),
            multiplicity: 1,
            reverse,
          });
        }
        previous = pathId + BigInt(value - 1);
      }
    }
  } else {
    fail("unknown membership codec");
  }
  if (
    state.cursor !== bytes.length ||
    result.length !== expected ||
    result.some((item) => item.pathId >= pathCount)
  ) {
    fail("invalid membership payload");
  }
  return result;
}

function parseTilePage(bytes, descriptor) {
  if (ascii(bytes.subarray(0, 8)) !== "PNGPMT01" || u32(bytes, 8) !== 1) {
    fail("invalid tile-membership page");
  }
  const groupCount = u32(bytes, 12);
  if (groupCount !== descriptor.groupCount) fail("group count mismatch");
  const state = { cursor: 32 };
  const memberships = [];
  for (let index = 0; index < groupCount; index += 1) {
    state.cursor += 16;
    const weight = safe(u64(bytes, state.cursor), "group weight");
    const unique = safe(u64(bytes, state.cursor + 8), "group unique paths");
    const length = safe(u64(bytes, state.cursor + 16), "membership length");
    state.cursor += 24;
    const values = decodeMemberships(
      bytes.subarray(state.cursor, state.cursor + length),
      descriptor.pathCount,
    );
    state.cursor += length;
    if (
      values.reduce((sum, item) => sum + item.multiplicity, 0) !== weight ||
      new Set(values.map((item) => item.pathId)).size !== unique
    ) {
      fail("membership totals differ from group");
    }
    memberships.push(...values);
  }
  if (state.cursor !== bytes.length) fail("membership page trailing bytes");
  return memberships;
}

function decodeCatalogPage(bytes, entry) {
  if (ascii(bytes.subarray(0, 8)) !== "PNGPCP01" || u32(bytes, 8) !== 1)
    fail("invalid catalog page");
  const count = u32(bytes, 12);
  const first = safe(u64(bytes, 16), "first path ID");
  if (count !== entry.recordCount || first !== entry.firstPathId)
    fail("catalog page identity mismatch");
  const state = { cursor: 24 };
  let raw = new Uint8Array();
  let sample = new Uint8Array();
  let contig = new Uint8Array();
  const result = [];
  const senses = ["unknown", "generic", "reference", "haplotype"];
  for (let index = 0; index < count; index += 1) {
    raw = frontString(bytes, state, raw);
    sample = frontString(bytes, state, sample);
    contig = frontString(bytes, state, contig);
    const haplotype = safe(varint(bytes, state), "haplotype");
    const fragment = safe(varint(bytes, state), "fragment");
    const pathSense = senses[bytes[state.cursor++]];
    if (!pathSense) fail("invalid path sense");
    result.push({
      canonical_path_id: first + index,
      raw_name: decoder.decode(raw),
      sample: decoder.decode(sample),
      contig: decoder.decode(contig),
      haplotype,
      fragment,
      path_sense: pathSense,
    });
  }
  if (state.cursor !== bytes.length) fail("catalog page trailing bytes");
  return result;
}

export async function runIntegratedMembership(config) {
  const ranges = [];
  const bootstrap = await readRange(
    config.url,
    0n,
    config.bootstrapBytes,
    config.fileBytes,
  );
  ranges.push({ offset: 0, length: config.bootstrapBytes });
  const extension = parseArchiveRoot(bootstrap.bytes, config.fileBytes);
  const descriptorRead = await readRange(
    config.url,
    extension.offset,
    extension.encodedLength,
    config.fileBytes,
    bootstrap.etag,
  );
  ranges.push({
    offset: safe(extension.offset, "descriptor offset"),
    length: extension.encodedLength,
  });
  const descriptor = parseDescriptor(
    decodeStored(extension, descriptorRead.bytes),
    config.query,
  );
  descriptor.selected.pathCount = descriptor.pathCount;
  const tileRead = await readRange(
    config.url,
    descriptor.selected.offset,
    descriptor.selected.encodedLength,
    config.fileBytes,
    bootstrap.etag,
  );
  ranges.push({
    offset: safe(descriptor.selected.offset, "tile offset"),
    length: descriptor.selected.encodedLength,
  });
  const memberships = parseTilePage(
    decodeStored(descriptor.selected, tileRead.bytes),
    descriptor.selected,
  );
  const pathIds = [...new Set(memberships.map((item) => item.pathId))].sort(
    (a, b) => a - b,
  );
  const pageIds = [
    ...new Set(pathIds.map((id) => Math.floor(id / descriptor.recordsPerPage))),
  ];
  const planned = pageIds.map((pageId) => {
    const entry = descriptor.catalog[pageId];
    return { offset: entry.offset, length: BigInt(entry.encodedLength) };
  });
  const catalogRanges = [];
  for (const range of planned) {
    const previous = catalogRanges.at(-1);
    if (previous && previous.offset + previous.length === range.offset) {
      previous.length += range.length;
    } else {
      catalogRanges.push({ ...range });
    }
  }
  const fetchedCatalog = await Promise.all(
    catalogRanges.map(async (range) => ({
      ...range,
      bytes: (
        await readRange(
          config.url,
          range.offset,
          safe(range.length, "catalog range"),
          config.fileBytes,
          bootstrap.etag,
        )
      ).bytes,
    })),
  );
  for (const range of catalogRanges) {
    ranges.push({
      offset: safe(range.offset, "catalog offset"),
      length: safe(range.length, "catalog length"),
    });
  }
  const pages = new Map();
  for (const pageId of pageIds) {
    const entry = descriptor.catalog[pageId];
    const fetched = fetchedCatalog.find(
      (range) =>
        entry.offset >= range.offset &&
        entry.offset + BigInt(entry.encodedLength) <=
          range.offset + range.length,
    );
    if (!fetched) fail("catalog range plan does not cover page");
    const start = safe(entry.offset - fetched.offset, "catalog page offset");
    pages.set(
      pageId,
      decodeCatalogPage(
        decodeStored(
          entry,
          fetched.bytes.subarray(start, start + entry.encodedLength),
        ),
        entry,
      ),
    );
  }
  const records = pathIds.map(
    (id) =>
      pages.get(Math.floor(id / descriptor.recordsPerPage))[
        id % descriptor.recordsPerPage
      ],
  );
  return {
    groups: descriptor.selected.groupCount,
    memberships: memberships.length,
    uniquePathIds: pathIds.length,
    records,
    selectedCatalogPages: pageIds.length,
    ranges,
    totalBytes: ranges.reduce((sum, range) => sum + range.length, 0),
  };
}
