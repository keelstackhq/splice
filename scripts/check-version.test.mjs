import { test } from "node:test";
import assert from "node:assert/strict";
import { checkVersion } from "./check-version.mjs";

const cargo = (v) => `[package]\nname = "x"\nversion = "${v}"\n`;

test("matching tag returns the version", () => {
  assert.equal(checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0") }), "0.2.0");
});

test("mismatch throws", () => {
  assert.throws(
    () => checkVersion({ tag: "v0.2.0", cargoToml: 'version = "0.1.0"' }),
    /tag v0\.2\.0 does not match Cargo\.toml 0\.1\.0/,
  );
});

test("prerelease tag matches", () => {
  assert.equal(
    checkVersion({ tag: "v1.0.0-rc.1", cargoToml: cargo("1.0.0-rc.1") }),
    "1.0.0-rc.1",
  );
  assert.throws(() => checkVersion({ tag: "v1.0.0", cargoToml: cargo("1.0.0-rc.1") }));
});

test("tag without v is rejected", () => {
  assert.throws(() => checkVersion({ tag: "0.2.0", cargoToml: cargo("0.2.0") }), /must start with "v"/);
});

test("uses the [package] version, not a dependency line", () => {
  const toml = `[dependencies]\nfoo = { version = "9.9.9" }\nversion = "9.9.9"\n\n[package]\nname = "x"\nversion = "0.3.0"\n\n[dependencies.bar]\nversion = "7.7.7"\n`;
  assert.equal(checkVersion({ tag: "v0.3.0", cargoToml: toml }), "0.3.0");
});

test("missing version throws", () => {
  assert.throws(() => checkVersion({ tag: "v0.1.0", cargoToml: "[dependencies]\nversion = \"0.1.0\"\n" }), /no \[package\] version/);
});
