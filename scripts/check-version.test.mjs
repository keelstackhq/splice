import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { checkVersion } from "./check-version.mjs";

const cargo = (v) => `[package]\nname = "keel-splice"\nversion = "${v}"\n`;
const platforms = JSON.parse(readFileSync(new URL("../npm/splice/platforms.json", import.meta.url), "utf8"));
const launcher = (version = "0.2.0", dep = "0.2.0") => ({
  version,
  optionalDependencies: Object.fromEntries(platforms.map((p) => [p.npm, dep])),
});

test("tag differing from Cargo.toml throws", () => {
  assert.throws(
    () => checkVersion({ tag: "v0.3.0", cargoToml: cargo("0.2.0") }),
    /tag v0\.3\.0 does not match Cargo\.toml 0\.2\.0/,
  );
});

test("non-semver tag is not a release tag", () => {
  assert.throws(() => checkVersion({ tag: "v1", cargoToml: cargo("0.2.0") }), /not a release tag/);
});

test("matching inputs return the version", () => {
  assert.equal(checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0") }), "0.2.0");
  assert.equal(checkVersion({ tag: "v0.2.0-rc.1", cargoToml: cargo("0.2.0-rc.1") }), "0.2.0-rc.1");
});

test("null tag returns the Cargo version", () => {
  assert.equal(checkVersion({ tag: null, cargoToml: cargo("0.2.0") }), "0.2.0");
});

test("launcher version mismatch names npm/splice/package.json", () => {
  assert.throws(
    () => checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0"), launcherPkg: launcher("0.1.0") }),
    /npm\/splice\/package\.json version 0\.1\.0/,
  );
});

test("optional dependency pin mismatch names the package", () => {
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), launcherPkg: launcher("0.2.0", "0.1.0") }),
    /@keelstackhq\/splice-linux-x64.*0\.1\.0/,
  );
});

test("missing optional dependency for a platform throws", () => {
  const pkg = launcher();
  delete pkg.optionalDependencies["@keelstackhq/splice-win32-x64"];
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), launcherPkg: pkg }),
    /missing optionalDependencies.*@keelstackhq\/splice-win32-x64/,
  );
});

test("consistent launcher passes", () => {
  assert.equal(checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0"), launcherPkg: launcher() }), "0.2.0");
});

test("release.yml tag filter is exactly the semver glob and skips v1", () => {
  const yml = readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");
  const m = yml.match(/tags:\s*\n\s*-\s*['"]?([^'"\n]+)['"]?\s*\n/);
  assert.ok(m, "tags list not found");
  assert.equal(m[1], "v[0-9]+.[0-9]+.[0-9]+*");
  // GitHub filter: `+` repeats the previous char/class, `*` matches any chars but `/`, `.` is literal.
  const re = new RegExp(
    "^" + m[1].replace(/\./g, "\\.").replace(/\]\+/g, "]+").replace(/\*/g, "[^/]*") + "$",
  );
  assert.ok(re.test("v0.2.0"));
  assert.ok(re.test("v0.2.0-rc.1"));
  assert.ok(!re.test("v1"));
  assert.ok(!re.test("v1.0"));
});

test("release.yml build matrix covers exactly the platforms.json targets", () => {
  const yml = readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");
  const targets = [...yml.matchAll(/\{ target: ([a-z0-9_-]+),/g)].map((m) => m[1]).sort();
  assert.deepEqual(targets, platforms.map((p) => p.target).sort());
});
