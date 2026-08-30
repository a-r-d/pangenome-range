#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { chromium } from "../../packages/benchmark/node_modules/@playwright/test/index.mjs";

const corpusPath = process.argv[2];
const iterations = Number(process.argv[3] ?? 1000);
if (!corpusPath || !Number.isSafeInteger(iterations) || iterations < 1) {
  throw new Error("usage: browser-benchmark.mjs CORPUS [ITERATIONS]");
}

const corpus = await readFile(corpusPath);
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  const result = await page.evaluate(
    ({ bytes, iterations }) => {
      const input = Uint8Array.from(bytes);
      let position = 0;
      const readU64 = () => {
        let value = 0;
        let scale = 1;
        for (let index = 0; index < 8; index += 1) {
          value += input[position++] * scale;
          scale *= 256;
        }
        if (!Number.isSafeInteger(value))
          throw new Error("unsafe u64 in corpus");
        return value;
      };
      const magic = new TextDecoder().decode(input.subarray(0, 8));
      if (magic !== "PMAC0001") throw new Error("invalid corpus magic");
      position = 8;
      const count = readU64();
      const groups = [];
      for (let index = 0; index < count; index += 1) {
        const length = readU64();
        groups.push(input.slice(position, position + length));
        position += length;
      }
      if (position !== input.length) throw new Error("trailing corpus bytes");

      const varint = (data, state) => {
        let value = 0;
        let scale = 1;
        for (;;) {
          const byte = data[state.position++];
          value += (byte & 0x7f) * scale;
          if ((byte & 0x80) === 0) return value;
          scale *= 128;
        }
      };
      const decode = (data) => {
        const adaptiveTag = data[0];
        const codecTag = data[1];
        const state = { position: 2 };
        let checksum = 0;
        if (adaptiveTag === 0 && codecTag === 0xa0) {
          const count = varint(data, state);
          let pathId = 0;
          for (let index = 0; index < count; index += 1) {
            pathId += varint(data, state);
            const multiplicity = varint(data, state);
            const orientation = data[state.position++];
            checksum += pathId + multiplicity + orientation;
          }
        } else if (adaptiveTag === 1 && codecTag === 0xb0) {
          const expected = varint(data, state);
          const records = varint(data, state);
          let previousEnd = 0;
          let decoded = 0;
          for (let index = 0; index < records; index += 1) {
            const kind = data[state.position++];
            const pathId = previousEnd + varint(data, state);
            if (kind === 0) {
              checksum += pathId + varint(data, state) + data[state.position++];
              previousEnd = pathId;
              decoded += 1;
            } else {
              const length = varint(data, state);
              checksum += pathId + length + data[state.position++];
              previousEnd = pathId + length - 1;
              decoded += length;
            }
          }
          if (decoded !== expected) throw new Error("run count mismatch");
        } else if (adaptiveTag === 2 && codecTag === 0xc0) {
          const universe = varint(data, state);
          const bitBytes = Math.ceil(universe / 8);
          for (let index = 0; index < bitBytes; index += 1)
            checksum += data[state.position++];
          const exceptions = varint(data, state);
          for (let index = 0; index < exceptions; index += 1) {
            checksum += varint(data, state);
            const entries = varint(data, state);
            for (let entry = 0; entry < entries; entry += 1) {
              checksum += varint(data, state) + data[state.position++];
            }
          }
        } else if (adaptiveTag === 3 && codecTag === 0xd0) {
          const blocks = varint(data, state);
          for (let block = 0; block < blocks; block += 1) {
            checksum += varint(data, state);
            if (data[state.position++] !== 0)
              throw new Error("unsupported roaring block");
            const entries = varint(data, state);
            for (let entry = 0; entry < entries; entry += 1) {
              checksum +=
                data[state.position] | (data[state.position + 1] << 8);
              state.position += 2;
              checksum += varint(data, state) + data[state.position++];
            }
          }
        } else {
          throw new Error(
            `unsupported adaptive/codec tag ${adaptiveTag}/${codecTag}`,
          );
        }
        if (state.position !== data.length)
          throw new Error("codec trailing bytes");
        return checksum;
      };

      let checksum = 0;
      const started = performance.now();
      for (let iteration = 0; iteration < iterations; iteration += 1) {
        for (const group of groups) checksum += decode(group);
      }
      const wallMs = performance.now() - started;
      return {
        browser: "chromium",
        groups: groups.length,
        corpusBytes: input.length,
        iterations,
        wallMs,
        perCorpusMs: wallMs / iterations,
        checksum,
      };
    },
    { bytes: [...corpus], iterations },
  );
  console.log(JSON.stringify(result, null, 2));
} finally {
  await browser.close();
}
