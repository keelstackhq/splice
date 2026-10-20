import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { stage } from "./npm-stage.mjs";

const platforms = JSON.parse(readFileSync(new URL("../npm/splice/platforms.json", import.meta.url), "utf8"));
const REPO = "keelstackhq/splice";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "npm-stage-"));
  const artifactsDir = join(root, "artifacts");
  for (const p of platforms) {
    mkdirSync(join(artifactsDir, p.target), { recursive: true });
    writeFileSync(join(artifactsDir, p.target, p.bin), `binary ${p.target}`);
  }
  return { artifactsDir, outDir: join(root, "out") };
}

const readPkg = (dir) => JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));

test("stage writes repository, license, no scripts, exact pins", () => {
  const { artifactsDir, outDir } = fixture();
  const dirs = stage({ version: "0.2.0", artifactsDir, outDir, repository: REPO });
  assert.equal(dirs.length, platforms.length + 1);
  for (const dir of dirs) {
    const pkg = readPkg(dir);
    assert.equal(pkg.repository.url, "git+https://github.com/keelstackhq/splice.git");
    assert.equal(pkg.repository.type, "git");
    assert.equal(pkg.license, "MIT");
    assert.equal(pkg.version, "0.2.0");
    assert.ok(!("scripts" in pkg));
    assert.ok(!("devDependencies" in pkg));
    assert.ok(existsSync(join(dir, "LICENSE")));
  }
  const launcher = readPkg(dirs.at(-1));
  assert.equal(launcher.name, "@keelstackhq/splice");
  assert.deepEqual(Object.keys(launcher.optionalDependencies).sort(), platforms.map((p) => p.npm).sort());
  for (const v of Object.values(launcher.optionalDependencies)) assert.equal(v, "0.2.0");
  for (const f of ["bin/splice.js", "lib/platform.js", "platforms.json", "README.md"]) {
    assert.ok(existsSync(join(dirs.at(-1), f)), f);
  }
});

test("stage writes platform packages with os, cpu and the binary", () => {
  const { artifactsDir, outDir } = fixture();
  const dirs = stage({ version: "0.2.0-rc.1", artifactsDir, outDir, repository: REPO });
  const linux = dirs.find((d) => d.endsWith("splice-linux-x64"));
  const pkg = readPkg(linux);
  assert.equal(pkg.name, "@keelstackhq/splice-linux-x64");
  assert.deepEqual(pkg.os, ["linux"]);
  assert.deepEqual(pkg.cpu, ["x64"]);
  assert.deepEqual(pkg.files, ["bin"]);
  assert.equal(readFileSync(join(linux, "bin", "splice"), "utf8"), "binary x86_64-unknown-linux-musl");
  if (process.platform !== "win32") assert.equal(statSync(join(linux, "bin", "splice")).mode & 0o777, 0o755);
  assert.ok(existsSync(join(dirs.find((d) => d.endsWith("splice-win32-x64")), "bin", "splice.exe")));
});

test("stage rejects a bad repository", () => {
  const { artifactsDir, outDir } = fixture();
  assert.throws(() => stage({ version: "0.2.0", artifactsDir, outDir, repository: "a b" }), /owner\/repo/);
});

test("stage fails when a binary is missing", () => {
  const { artifactsDir, outDir } = fixture();
  assert.throws(
    () => stage({ version: "0.2.0", artifactsDir: join(artifactsDir, "nope"), outDir, repository: REPO }),
    /missing binary/,
  );
});

test("launcher package.json is committed with exact optionalDependencies and no scripts", () => {
  const pkg = JSON.parse(readFileSync(new URL("../npm/splice/package.json", import.meta.url), "utf8"));
  assert.equal(pkg.name, "@keelstackhq/splice");
  assert.ok(!("scripts" in pkg));
  assert.equal(pkg.type, "module");
  assert.deepEqual(pkg.bin, { splice: "bin/splice.js" });
});
