import { blake3 } from "@noble/hashes/blake3.js";
import {
  type FeatureCodec,
  FeatureDecodeError,
  type StoredFeaturePage,
} from "./features.js";

export const PATH_MEMBERSHIP_TYPE_ID = "path-members-v1-";
export const PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES = 4 * 1024;

const VERSION = 1;
const MAX_DESCRIPTOR_BYTES = 16 * 1024 * 1024;
const MAX_PAGE_BYTES = 64 * 1024 * 1024;
const MAX_CATALOG_PAGES = 1_000_000;
const MAX_MANIFESTS = 1_000_000;
const MAX_GROUPS_PER_TILE = 65_536;
const MAX_OCCURRENCES_PER_TILE = 16 * 1024 * 1024;
export const MAX_PATH_MEMBERSHIPS_PER_GROUP = 250_000;
export const MAX_PATH_MEMBERSHIPS_PER_TILE = 250_000;
const DIRECTORY_HEADER_BYTES = 32;
const DIRECTORY_ENTRY_BYTES = 56;
const DESCRIPTOR_HEADER_BYTES = 112;
const CATALOG_DESCRIPTOR_BYTES = 64;
const MANIFEST_BYTES = 32;
const DIRECTORY_CAPACITY = Math.floor(
  (PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES - DIRECTORY_HEADER_BYTES) /
    DIRECTORY_ENTRY_BYTES,
);
const U64_MAX = 0xffff_ffff_ffff_ffffn;
const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);
const decoder = new TextDecoder("utf-8", { fatal: true });
const encoder = new TextEncoder();

export interface PathCatalogPageDescriptor {
  firstPathId: bigint;
  recordCount: bigint;
  storage: StoredFeaturePage;
}

export interface PathMembershipManifest {
  manifestIndex: number;
  firstPageOffset: bigint;
  pageCount: bigint;
  entryCount: bigint;
}

export interface PathMembershipDescriptor {
  pathCount: bigint;
  recordsPerCatalogPage: number;
  identitySource:
    | "embedded-gbwt-da-bounded-lf-v1"
    | "prepared-authenticated-oracle-v1";
  identitySourceSha256: Uint8Array;
  groupCount: bigint;
  occurrenceTotal: bigint;
  groupUniquePathCountSum: bigint;
  deltaGroupCount: bigint;
  runGroupCount: bigint;
  catalogPages: PathCatalogPageDescriptor[];
  manifests: PathMembershipManifest[];
}

export interface PathMembershipDirectoryEntry {
  groupCount: bigint;
  storage: StoredFeaturePage;
}

export interface DecodedPathCatalogRecord {
  pathId: bigint;
  canonicalName: string;
  sample: string;
  contig: string;
  haplotype: bigint;
  fragment: bigint;
  sense: 0 | 1 | 2 | 3;
}

export interface DecodedPathMembership {
  pathId: bigint;
  multiplicity: bigint;
  reversedRelativeToGroup: boolean;
}

export interface DecodedTraversalMembershipGroup {
  traversalDigest: Uint8Array;
  occurrenceWeight: bigint;
  uniquePathCount: bigint;
  memberships: DecodedPathMembership[];
}

export interface DecodedTileMembershipPage {
  coreStart: bigint;
  coreEnd: bigint;
  regionalPayloadIntegrity: Uint8Array;
  groups: DecodedTraversalMembershipGroup[];
}

export function traversalMembershipDigest(
  sample: string,
  contig: string,
  coreStart: bigint,
  coreEnd: bigint,
  regionalPayloadIntegrity: Uint8Array,
  handles: readonly bigint[],
): Uint8Array {
  if (regionalPayloadIntegrity.byteLength !== 16)
    fail("invalid regional payload integrity");
  const hasher = blake3.create();
  hasher.update(
    encoder.encode("pangenome-range/path-membership/traversal/v1\0"),
  );
  hashBytes(hasher, encoder.encode(sample));
  hashBytes(hasher, encoder.encode(contig));
  hashU64(hasher, coreStart);
  hashU64(hasher, coreEnd);
  hasher.update(regionalPayloadIntegrity);
  hashU64(hasher, BigInt(handles.length));
  for (const handle of handles) hashU64(hasher, handle);
  return hasher.digest().subarray(0, 16);
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
      fail("invalid membership length");
    const end = this.#position + length;
    if (!Number.isSafeInteger(end) || end > this.#bytes.byteLength) {
      fail("unexpected end of path-membership data");
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

  finish(): void {
    if (this.remaining !== 0)
      fail(`${this.remaining} trailing path-membership bytes`);
  }
}

function header(reader: Reader, magic: string): void {
  if (decodeText(reader.take(8)) !== magic || reader.u32() !== VERSION) {
    fail("invalid path-membership header");
  }
}

function storedPage(
  reader: Reader,
  dataOffset: bigint,
  sourceSize: bigint,
): StoredFeaturePage {
  const offset = reader.u64();
  const encodedLength = reader.u64();
  const decodedLength = reader.u64();
  const codec = reader.u8();
  assertZero(reader.take(7), "path-membership page reserved bytes");
  const integrity = reader.take(16).slice();
  const end = checkedAdd(offset, encodedLength, "path-membership page end");
  if (
    ![0, 1, 3, 6].includes(codec) ||
    encodedLength === 0n ||
    decodedLength === 0n ||
    encodedLength > BigInt(MAX_PAGE_BYTES) ||
    decodedLength > BigInt(MAX_PAGE_BYTES) ||
    offset < dataOffset ||
    end > sourceSize
  ) {
    fail("invalid path-membership page range");
  }
  return {
    offset,
    encodedLength,
    decodedLength,
    codec: codec as FeatureCodec,
    integrity,
  };
}

export function decodePathMembershipDescriptor(
  bytes: Uint8Array,
  dataOffset: bigint,
  sourceSize: bigint,
): PathMembershipDescriptor {
  if (bytes.byteLength > MAX_DESCRIPTOR_BYTES)
    fail("path-membership descriptor is too large");
  if (bytes.byteLength < DESCRIPTOR_HEADER_BYTES)
    fail("invalid path-membership descriptor length");
  const reader = new Reader(bytes);
  header(reader, "PNGPMD01");
  const recordsPerCatalogPage = reader.u32();
  const pathCount = reader.u64();
  const catalogCount = reader.u32();
  const manifestCount = reader.u32();
  const identitySourceCode = reader.u8();
  assertZero(reader.take(7), "path-membership provenance reserved bytes");
  const identitySourceSha256 = reader.take(32).slice();
  const groupCount = reader.u64();
  const occurrenceTotal = reader.u64();
  const groupUniquePathCountSum = reader.u64();
  const deltaGroupCount = reader.u64();
  const runGroupCount = reader.u64();
  const identitySource =
    identitySourceCode === 1
      ? "embedded-gbwt-da-bounded-lf-v1"
      : identitySourceCode === 2
        ? "prepared-authenticated-oracle-v1"
        : undefined;
  if (
    pathCount === 0n ||
    recordsPerCatalogPage === 0 ||
    recordsPerCatalogPage > 65_536 ||
    catalogCount === 0 ||
    catalogCount > MAX_CATALOG_PAGES ||
    manifestCount === 0 ||
    manifestCount > MAX_MANIFESTS ||
    identitySource === undefined ||
    identitySourceSha256.every((byte) => byte === 0) ||
    deltaGroupCount + runGroupCount !== groupCount ||
    groupUniquePathCountSum > occurrenceTotal
  ) {
    fail("invalid path-membership descriptor dimensions");
  }
  const expectedBytes =
    DESCRIPTOR_HEADER_BYTES +
    catalogCount * CATALOG_DESCRIPTOR_BYTES +
    manifestCount * MANIFEST_BYTES;
  if (
    !Number.isSafeInteger(expectedBytes) ||
    bytes.byteLength !== expectedBytes
  ) {
    fail("invalid path-membership descriptor length");
  }
  const catalogPages: PathCatalogPageDescriptor[] = [];
  let expectedPathId = 0n;
  for (let index = 0; index < catalogCount; index += 1) {
    const firstPathId = reader.u64();
    const recordCount = reader.u64();
    const storage = storedPage(reader, dataOffset, sourceSize);
    if (
      firstPathId !== expectedPathId ||
      recordCount === 0n ||
      recordCount > BigInt(recordsPerCatalogPage)
    ) {
      fail("invalid path-catalog descriptor page");
    }
    expectedPathId = checkedAdd(expectedPathId, recordCount, "path count");
    catalogPages.push({ firstPathId, recordCount, storage });
  }
  if (expectedPathId !== pathCount)
    fail("path catalog does not cover its descriptor");
  const manifests: PathMembershipManifest[] = [];
  let previousEnd: bigint | undefined;
  let totalEntries = 0n;
  for (let index = 0; index < manifestCount; index += 1) {
    const manifestIndex = reader.u32();
    if (reader.u32() !== 0)
      fail("path-membership manifest reserved bytes are nonzero");
    const firstPageOffset = reader.u64();
    const pageCount = reader.u64();
    const entryCount = reader.u64();
    const end = checkedAdd(
      firstPageOffset,
      pageCount * BigInt(PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES),
      "path-membership directory end",
    );
    if (
      manifestIndex !== index ||
      pageCount === 0n ||
      entryCount === 0n ||
      firstPageOffset < dataOffset ||
      end > sourceSize ||
      (previousEnd !== undefined && firstPageOffset !== previousEnd)
    ) {
      fail("invalid path-membership manifest");
    }
    manifests.push({ manifestIndex, firstPageOffset, pageCount, entryCount });
    previousEnd = end;
    totalEntries = checkedAdd(
      totalEntries,
      entryCount,
      "path-membership entries",
    );
  }
  reader.finish();
  if (totalEntries === 0n) fail("path-membership descriptor has no entries");
  return {
    pathCount,
    recordsPerCatalogPage,
    identitySource,
    identitySourceSha256,
    groupCount,
    occurrenceTotal,
    groupUniquePathCountSum,
    deltaGroupCount,
    runGroupCount,
    catalogPages,
    manifests,
  };
}

export function decodePathMembershipDirectoryPage(
  bytes: Uint8Array,
  dataOffset: bigint,
  sourceSize: bigint,
): PathMembershipDirectoryEntry[] {
  if (bytes.byteLength !== PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES) {
    fail("invalid path-membership directory page size");
  }
  const reader = new Reader(bytes);
  header(reader, "PNGPMI01");
  const count = reader.u32();
  const expectedDigest = reader.take(16);
  const digest = blake3(bytes.subarray(DIRECTORY_HEADER_BYTES)).subarray(0, 16);
  if (count > DIRECTORY_CAPACITY || !equalBytes(expectedDigest, digest)) {
    fail("invalid path-membership directory page");
  }
  const entries: PathMembershipDirectoryEntry[] = [];
  for (let index = 0; index < count; index += 1) {
    const groupCount = reader.u64();
    const storage = storedPage(reader, dataOffset, sourceSize);
    if (groupCount > BigInt(MAX_GROUPS_PER_TILE)) {
      fail("invalid path-membership directory entry");
    }
    entries.push({ groupCount, storage });
  }
  assertZero(
    reader.take(reader.remaining),
    "path-membership directory padding",
  );
  return entries;
}

export function decodePathCatalogPage(
  bytes: Uint8Array,
): DecodedPathCatalogRecord[] {
  if (bytes.byteLength > MAX_PAGE_BYTES) fail("path-catalog page is too large");
  const reader = new Reader(bytes);
  header(reader, "PNGPCP01");
  const count = reader.u32();
  const first = reader.u64();
  if (
    count === 0 ||
    count > 65_536 ||
    count > Math.floor(reader.remaining / 6)
  ) {
    fail("invalid path-catalog record count");
  }
  const records: DecodedPathCatalogRecord[] = [];
  let raw: Uint8Array = new Uint8Array();
  let sample: Uint8Array = new Uint8Array();
  let contig: Uint8Array = new Uint8Array();
  let reconstructedBytes = 0;
  for (let index = 0; index < count; index += 1) {
    raw = readFrontCoded(reader, raw);
    sample = readFrontCoded(reader, sample);
    contig = readFrontCoded(reader, contig);
    reconstructedBytes +=
      raw.byteLength + sample.byteLength + contig.byteLength;
    if (
      !Number.isSafeInteger(reconstructedBytes) ||
      reconstructedBytes > MAX_PAGE_BYTES
    ) {
      fail("path catalog reconstructed strings exceed the page bound");
    }
    const haplotype = varint(reader);
    const fragment = varint(reader);
    const sense = reader.u8();
    if (sense > 3) fail("invalid path sense");
    records.push({
      pathId: checkedAdd(first, BigInt(index), "path ID"),
      canonicalName: decodeText(raw),
      sample: decodeText(sample),
      contig: decodeText(contig),
      haplotype,
      fragment,
      sense: sense as 0 | 1 | 2 | 3,
    });
  }
  reader.finish();
  return records;
}

export function decodeTileMembershipPage(
  bytes: Uint8Array,
  pathCount: bigint,
): DecodedTileMembershipPage {
  if (bytes.byteLength > MAX_PAGE_BYTES)
    fail("tile-membership page is too large");
  const reader = new Reader(bytes);
  header(reader, "PNGPMT01");
  const count = reader.u32();
  const coreStart = reader.u64();
  const coreEnd = reader.u64();
  const regionalPayloadIntegrity = reader.take(16).slice();
  if (
    count > MAX_GROUPS_PER_TILE ||
    count > Math.floor(reader.remaining / 41) ||
    coreStart >= coreEnd
  ) {
    fail("invalid tile-membership page dimensions");
  }
  const groups: DecodedTraversalMembershipGroup[] = [];
  let totalWeight = 0n;
  let totalMemberships = 0;
  let previousDigest: Uint8Array | undefined;
  for (let index = 0; index < count; index += 1) {
    const traversalDigest = reader.take(16).slice();
    const occurrenceWeight = reader.u64();
    const uniquePathCount = reader.u64();
    const encodedLength = safeNumber(reader.u64(), "membership codec length");
    const memberships = decodeMemberships(
      reader.take(encodedLength),
      pathCount,
    );
    totalMemberships += memberships.length;
    if (totalMemberships > MAX_PATH_MEMBERSHIPS_PER_TILE) {
      fail("tile path-membership record count exceeds its safety bound");
    }
    const sum = memberships.reduce(
      (total, item) => total + item.multiplicity,
      0n,
    );
    if (
      occurrenceWeight === 0n ||
      sum !== occurrenceWeight ||
      uniquePathCount !==
        BigInt(new Set(memberships.map((item) => item.pathId)).size) ||
      (previousDigest !== undefined &&
        compareBytes(previousDigest, traversalDigest) >= 0)
    ) {
      fail("tile-membership group totals or ordering are invalid");
    }
    totalWeight = checkedAdd(
      totalWeight,
      occurrenceWeight,
      "tile occurrence weight",
    );
    if (totalWeight > BigInt(MAX_OCCURRENCES_PER_TILE))
      fail("tile occurrence bound exceeded");
    groups.push({
      traversalDigest,
      occurrenceWeight,
      uniquePathCount,
      memberships,
    });
    previousDigest = traversalDigest;
  }
  reader.finish();
  return { coreStart, coreEnd, regionalPayloadIntegrity, groups };
}

function hashBytes(
  hasher: ReturnType<typeof blake3.create>,
  bytes: Uint8Array,
): void {
  hashU64(hasher, BigInt(bytes.byteLength));
  hasher.update(bytes);
}

function hashU64(
  hasher: ReturnType<typeof blake3.create>,
  value: bigint,
): void {
  if (value < 0n || value > U64_MAX)
    fail("path-membership hash value overflow");
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  hasher.update(bytes);
}

function decodeMemberships(
  bytes: Uint8Array,
  pathCount: bigint,
): DecodedPathMembership[] {
  const reader = new Reader(bytes);
  const codec = reader.u8();
  const expected = varint(reader);
  if (expected === 0n || expected > BigInt(MAX_PATH_MEMBERSHIPS_PER_GROUP)) {
    fail("path-membership entry count exceeds its bound");
  }
  const expectedCount = safeNumber(expected, "path-membership entry count");
  const result: DecodedPathMembership[] = [];
  let previousEnd = 0n;
  if (codec === 0) {
    for (let index = 0; index < expectedCount; index += 1) {
      const pathId = checkedAdd(
        previousEnd,
        varint(reader),
        "path-membership delta",
      );
      const multiplicity = varint(reader);
      const reverse = reader.u8();
      if (pathId >= pathCount || multiplicity === 0n || reverse > 1)
        fail("invalid membership");
      result.push({
        pathId,
        multiplicity,
        reversedRelativeToGroup: reverse === 1,
      });
      previousEnd = pathId;
    }
  } else if (codec === 1) {
    const recordCount = safeNumber(varint(reader), "membership run count");
    if (recordCount > Math.floor(reader.remaining / 4))
      fail("invalid membership run count");
    for (let index = 0; index < recordCount; index += 1) {
      const kind = reader.u8();
      const pathId = checkedAdd(
        previousEnd,
        varint(reader),
        "membership run delta",
      );
      const value = varint(reader);
      const reverse = reader.u8();
      if (kind > 1 || value === 0n || reverse > 1)
        fail("invalid membership run");
      if (kind === 0) {
        if (pathId >= pathCount || result.length >= expectedCount)
          fail("membership run out of bounds");
        result.push({
          pathId,
          multiplicity: value,
          reversedRelativeToGroup: reverse === 1,
        });
        previousEnd = pathId;
      } else {
        const end = checkedAdd(pathId, value, "membership run end");
        if (end > pathCount || BigInt(result.length) + value > expected) {
          fail("membership run out of bounds");
        }
        for (let id = pathId; id < end; id += 1n) {
          result.push({
            pathId: id,
            multiplicity: 1n,
            reversedRelativeToGroup: reverse === 1,
          });
        }
        previousEnd = end - 1n;
      }
    }
  } else {
    fail("unknown path-membership codec");
  }
  reader.finish();
  if (
    result.length !== expectedCount ||
    result.some(
      (item, index) =>
        index > 0 &&
        compareMembership(result[index - 1] as DecodedPathMembership, item) >=
          0,
    )
  ) {
    fail("path-membership path/orientation pairs are not unique and ordered");
  }
  return result;
}

function readFrontCoded(reader: Reader, previous: Uint8Array): Uint8Array {
  const prefix = safeNumber(varint(reader), "front-coded prefix");
  const suffixLength = safeNumber(varint(reader), "front-coded suffix");
  if (
    prefix > previous.byteLength ||
    (prefix < previous.byteLength &&
      ((previous[prefix] as number) & 0xc0) === 0x80)
  ) {
    fail("invalid front-coded prefix");
  }
  const value = new Uint8Array(prefix + suffixLength);
  value.set(previous.subarray(0, prefix));
  value.set(reader.take(suffixLength), prefix);
  decodeText(value);
  return value;
}

function compareMembership(
  left: DecodedPathMembership,
  right: DecodedPathMembership,
): number {
  if (left.pathId < right.pathId) return -1;
  if (left.pathId > right.pathId) return 1;
  return (
    Number(left.reversedRelativeToGroup) - Number(right.reversedRelativeToGroup)
  );
}

function varint(reader: Reader): bigint {
  let value = 0n;
  let shift = 0n;
  for (;;) {
    const byte = reader.u8();
    const payload = BigInt(byte & 0x7f);
    if (shift >= 64n || payload > U64_MAX >> shift)
      fail("path-membership varint overflow");
    value |= payload << shift;
    if ((byte & 0x80) === 0) {
      if (shift !== 0n && payload === 0n)
        fail("non-minimal path-membership varint");
      return value;
    }
    shift += 7n;
  }
}

function checkedAdd(left: bigint, right: bigint, label: string): bigint {
  const value = left + right;
  if (value > U64_MAX) fail(`${label} overflows u64`);
  return value;
}

function safeNumber(value: bigint, label: string): number {
  if (value < 0n || value > MAX_SAFE_BIGINT)
    fail(`${label} exceeds safe integer range`);
  return Number(value);
}

function decodeText(bytes: Uint8Array): string {
  try {
    return decoder.decode(bytes);
  } catch (error) {
    fail(`invalid UTF-8 path string: ${String(error)}`);
  }
}

function assertZero(bytes: Uint8Array, label: string): void {
  if (bytes.some((byte) => byte !== 0)) fail(`${label} must be zero`);
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.byteLength === right.byteLength &&
    left.every((byte, index) => byte === right[index])
  );
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const shared = Math.min(left.byteLength, right.byteLength);
  for (let index = 0; index < shared; index += 1) {
    const difference = (left[index] as number) - (right[index] as number);
    if (difference !== 0) return difference;
  }
  return left.byteLength - right.byteLength;
}

function fail(message: string): never {
  throw new FeatureDecodeError(message);
}
