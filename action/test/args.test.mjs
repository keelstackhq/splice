import assert from "node:assert/strict";
import test from "node:test";
import { command, scopeDefault, validateInputs } from "../args.mjs";

const ok = { version: "latest", to: "", branch: "splice/update", scope: "" };

test("command and inputs", () => {
  assert.deepEqual(command({ scope: "main", to: "" }), ["update", "--json", "--yes"]);
  assert.deepEqual(command({ scope: "main", to: "2.0.0" }), ["update", "--json", "--yes", "--to", "2.0.0"]);
  assert.throws(() => validateInputs({ version: "1; rm -rf /", to: "", branch: "x" }), /version/);
});

test("accepted versions", () => {
  for (const version of ["latest", "next", "none", "0.2.0", "1.2.3-rc.1"]) {
    validateInputs({ ...ok, version });
  }
});

test("rejected versions and to", () => {
  for (const version of ["", "v1", "1.2", "latest ", "1.2.3\nx"]) {
    assert.throws(() => validateInputs({ ...ok, version }), /version/, JSON.stringify(version));
  }
  validateInputs({ ...ok, to: "2.0.0" });
  for (const to of ["v2", "2.0", "$(id)"]) {
    assert.throws(() => validateInputs({ ...ok, to }), /`to`/, to);
  }
});

test("scope default follows the channel files", () => {
  assert.equal(scopeDefault({ channels: ["rig.toml"] }), "all");
  assert.equal(scopeDefault({ channels: [] }), "main");
});

test("scope all runs update --all and refuses to", () => {
  assert.deepEqual(command({ scope: "all", to: "" }), ["update", "--all", "--json", "--yes"]);
  assert.throws(() => command({ scope: "all", to: "2.0.0" }), /to.*scope: main/);
});

test("scope input", () => {
  for (const scope of ["", "main", "all"]) validateInputs({ ...ok, scope });
  for (const scope of ["ALL", "channel", "all\nx", "$(id)"]) {
    assert.throws(() => validateInputs({ ...ok, scope }), /scope/, JSON.stringify(scope));
  }
});
