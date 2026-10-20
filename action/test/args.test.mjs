import assert from "node:assert/strict";
import test from "node:test";
import { command, validateInputs } from "../args.mjs";

const ok = { version: "latest", to: "", branch: "splice/update" };

test("command and inputs", () => {
  assert.deepEqual(command({ to: "" }), ["update", "--json", "--yes"]);
  assert.deepEqual(command({ to: "2.0.0" }), ["update", "--json", "--yes", "--to", "2.0.0"]);
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
