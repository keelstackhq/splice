import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
const launcher = join(here, "..", "bin", "splice.js");
const { packageFor, binaryPath, supportedPairs } = require("../lib/platform.js");

const isWin = process.platform === "win32";

// A fake platform package for the host, under <tmp>/node_modules, found via NODE_PATH.
function fakeInstall(script) {
  const tmp = mkdtempSync(join(tmpdir(), "splice-launcher-"));
  const host = packageFor();
  assert.ok(host, "host platform must be supported for these tests");
  const dir = join(tmp, "node_modules", ...host.npm.split("/"));
  mkdirSync(join(dir, "bin"), { recursive: true });
  writeFileSync(join(dir, "package.json"), JSON.stringify({ name: host.npm, version: "0.0.0" }));
  const bin = join(dir, "bin", host.bin);
  writeFileSync(bin, `#!/usr/bin/env node\n${script}\n`);
  chmodSync(bin, 0o755);
  return { tmp, env: { ...process.env, NODE_PATH: join(tmp, "node_modules") } };
}

test("maps platforms", () => {
  assert.deepEqual(packageFor("win32", "x64"), { npm: "@keelstack/splice-win32-x64", bin: "splice.exe" });
  assert.deepEqual(packageFor("linux", "arm64"), { npm: "@keelstack/splice-linux-arm64", bin: "splice" });
  assert.deepEqual(packageFor("darwin", "arm64"), { npm: "@keelstack/splice-darwin-arm64", bin: "splice" });
  assert.equal(packageFor("linux", "ia32"), null);
});

test("unsupported platform message lists supported pairs", () => {
  assert.throws(
    () => binaryPath(() => { throw new Error("nf"); }, "freebsd", "x64"),
    /freebsd-x64.*linux-x64/s,
  );
  assert.ok(supportedPairs().includes("win32-x64"));
});

test("windows resolves splice.exe", () => {
  const seen = [];
  const p = binaryPath((id) => { seen.push(id); return join("/pkg", "package.json"); }, "win32", "x64");
  assert.deepEqual(seen, ["@keelstack/splice-win32-x64/package.json"]);
  assert.equal(p, join("/pkg", "bin", "splice.exe"));
});

test("missing platform package gives a helpful message", () => {
  assert.throws(
    () => binaryPath(() => { throw new Error("nf"); }, "linux", "x64"),
    /@keelstack\/splice-linux-x64/,
  );
});

test("unsupported platform: message, exit 1, no stack trace", () => {
  const shim = join(mkdtempSync(join(tmpdir(), "splice-shim-")), "shim.cjs");
  writeFileSync(
    shim,
    'Object.defineProperty(process, "platform", { value: "freebsd" });\n',
  );
  const r = spawnSync(process.execPath, ["--require", shim, launcher, "--version"], { encoding: "utf8" });
  assert.equal(r.status, 1);
  assert.match(r.stderr, /freebsd-/);
  assert.match(r.stderr, /linux-x64/);
  assert.doesNotMatch(r.stderr, /\n\s+at /);
});

for (const code of [0, 1, 3]) {
  test(`exit code ${code} passes through`, { skip: isWin }, () => {
    const { tmp, env } = fakeInstall(`process.exit(${code});`);
    const r = spawnSync(process.execPath, [launcher], { env });
    rmSync(tmp, { recursive: true, force: true });
    assert.equal(r.status, code);
  });
}

test("arguments are forwarded", { skip: isWin }, () => {
  const { tmp, env } = fakeInstall("console.log(JSON.stringify(process.argv.slice(2)));");
  const r = spawnSync(process.execPath, [launcher, "new", "--yes"], { env, encoding: "utf8" });
  rmSync(tmp, { recursive: true, force: true });
  assert.equal(r.stdout.trim(), '["new","--yes"]');
});

test("a child killed by SIGTERM is re-raised", { skip: isWin }, () => {
  const { tmp, env } = fakeInstall('process.kill(process.pid, "SIGTERM"); setTimeout(() => {}, 5000);');
  const r = spawnSync(process.execPath, [launcher], { env });
  rmSync(tmp, { recursive: true, force: true });
  assert.equal(r.signal, "SIGTERM");
});

test("SIGINT to the process group: launcher survives and the child decides", { skip: isWin }, async () => {
  const { tmp, env } = fakeInstall(
    'process.on("SIGINT", () => { setTimeout(() => process.exit(7), 100); });\n' +
      'process.stdout.write("ready\\n"); setTimeout(() => process.exit(9), 10000);',
  );
  const child = spawn(process.execPath, [launcher], { env, detached: true, stdio: ["ignore", "pipe", "inherit"] });
  await new Promise((resolve) => child.stdout.once("data", resolve));
  process.kill(-child.pid, "SIGINT"); // like a terminal Ctrl-C
  const [code, signal] = await new Promise((resolve) => child.on("exit", (c, s) => resolve([c, s])));
  rmSync(tmp, { recursive: true, force: true });
  assert.equal(signal, null);
  assert.equal(code, 7);
});
