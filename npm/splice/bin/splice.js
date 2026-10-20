#!/usr/bin/env node
"use strict";
// Thin launcher: find the platform binary, run it with inherited stdio, mirror its exit.
// Launcher failures (no binary, unsupported platform, spawn error) exit 3: env error, not drift (1).
const { spawnSync } = require("node:child_process");
const { binaryPath } = require("../lib/platform.js");

let bin;
try {
  bin = binaryPath();
} catch (e) {
  process.stderr.write(`splice: ${e.message}\n`);
  process.exit(3);
}

// Ctrl-C reaches the whole foreground process group. A registered handler keeps this
// process alive (spawnSync blocks, so it never runs); the child decides how to react.
process.on("SIGINT", () => {});

const r = spawnSync(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (r.error) {
  process.stderr.write(`splice: failed to run ${bin}: ${r.error.message}\n`);
  process.exit(3);
}
if (r.signal) {
  process.removeAllListeners("SIGINT");
  process.kill(process.pid, r.signal);
  // Re-raise did not terminate us (signal ignored or handled): fall back to 128 + n.
  const os = require("node:os");
  process.exit(128 + (os.constants.signals[r.signal] ?? 0));
}
process.exit(r.status ?? 1);
