import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { stage } from "./npm-stage.mjs";

const platforms = JSON.parse(readFileSync(new URL("../npm/platforms.json", import.meta.url), "utf8"));

function setup(only = platforms, env = {}) {
  const root = mkdtempSync(join(tmpdir(), "splice-stage-"));
  const artifacts = join(root, "artifacts");
  mkdirSync(artifacts);
  for (const p of only) {
    const dir = join(artifacts, p.target);
    mkdirSync(dir);
    writeFileSync(join(dir, p.bin), "binary");
  }
  const out = join(root, "out");
  stage({ version: "1.2.3", artifacts, out, env });
  return out;
}

const readPkg = (out, name) => JSON.parse(readFileSync(join(out, name, "package.json"), "utf8"));

test("stage writes no install scripts", () => {
  const out = setup();
  const dirs = readdirSync(out);
  assert.equal(dirs.length, 6);
  for (const d of dirs) {
    const scripts = readPkg(out, d).scripts ?? {};
    for (const k of ["preinstall", "install", "postinstall", "prepare"]) assert.equal(scripts[k], undefined, `${d} ${k}`);
  }
});

test("platform packages declare os/cpu, files and the binary", () => {
  const out = setup();
  const pkg = readPkg(out, "splice-win32-x64");
  assert.equal(pkg.name, "@keelstack/splice-win32-x64");
  assert.equal(pkg.version, "1.2.3");
  assert.deepEqual(pkg.os, ["win32"]);
  assert.deepEqual(pkg.cpu, ["x64"]);
  assert.equal(pkg.preferUnplugged, true);
  assert.deepEqual(pkg.files, ["bin"]);
  assert.ok(pkg.repository.url.includes("keelstack/splice"));
  assert.ok(existsSync(join(out, "splice-win32-x64", "bin", "splice.exe")));
  if (process.platform !== "win32") {
    const mode = statSync(join(out, "splice-linux-x64", "bin", "splice")).mode & 0o777;
    assert.equal(mode, 0o755);
  }
});

test("launcher pins optionalDependencies and ships platforms.json", () => {
  const out = setup();
  const pkg = readPkg(out, "splice");
  assert.equal(pkg.name, "@keelstack/splice");
  assert.equal(pkg.version, "1.2.3");
  assert.deepEqual(pkg.bin, { splice: "bin/splice.js" });
  assert.deepEqual(
    pkg.optionalDependencies,
    Object.fromEntries(platforms.map((p) => [p.npm, "1.2.3"])),
  );
  assert.ok(existsSync(join(out, "splice", "bin", "splice.js")));
  assert.ok(existsSync(join(out, "splice", "lib", "platform.js")));
  assert.deepEqual(
    JSON.parse(readFileSync(join(out, "splice", "platforms.json"), "utf8")),
    platforms,
  );
});

test("README and LICENSE are copied into every package", () => {
  const out = setup();
  for (const d of readdirSync(out)) {
    assert.ok(existsSync(join(out, d, "LICENSE")), `${d} LICENSE`);
    assert.ok(existsSync(join(out, d, "README.md")), `${d} README`);
  }
});

test("a missing binary fails loudly", () => {
  assert.throws(() => setup(platforms.slice(1)), /splice-linux-x64|x86_64-unknown-linux-musl/);
});

test("repository comes from GITHUB_REPOSITORY, with a default", () => {
  const dflt = setup();
  const custom = setup(platforms, { GITHUB_REPOSITORY: "acme/widgets" });
  for (const [out, repo] of [[dflt, "keelstack/splice"], [custom, "acme/widgets"]]) {
    for (const d of readdirSync(out)) {
      assert.equal(readPkg(out, d).repository.url, `git+https://github.com/${repo}.git`, d);
    }
  }
});
