#!/usr/bin/env node

import process from "node:process";
import { launchNativeCli } from "./launcher.mjs";

try {
  const result = await launchNativeCli(process.argv.slice(2));
  if (result.signal) {
    process.kill(process.pid, result.signal);
  } else {
    process.exitCode = result.code ?? 1;
  }
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`pangenome-range: ${message}`);
  process.exitCode = 1;
}
