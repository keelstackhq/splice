#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { binaryPath, LauncherError } from "../lib/platform.js";

let bin;
try {
  bin = binaryPath();
} catch (e) {
  if (e instanceof LauncherError) {
    console.error(e.message);
    process.exit(e.exitCode);
  }
  throw e;
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (result.error) {
  console.error(`splice: cannot run the binary: ${result.error.message}`);
  process.exit(3);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
  process.exit(1);
}
process.exit(result.status ?? 3);
