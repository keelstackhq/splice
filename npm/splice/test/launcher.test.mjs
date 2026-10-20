import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { binaryPath, packageFor } from "../lib/platform.js";

const launcher = fileURLToPath(new URL("../bin/splice.js", import.meta.url));
const posix = process.platform !== "win32";
const scratch = mkdtempSync(join(tmpdir(), "splice-launcher-"));

function fakeScript(body, name = "fake") {
  const path = join(scratch, name);
  writeFileSync(path, `#!/bin/sh\n${body}\n`);
  chmodSync(path, 0o755);
  return path;
}

function run(env) {
  return spawnSync(process.execPath, [launcher], { env: { ...process.env, ...env }, encoding: "utf8" });
}

test("maps platforms", () => {
  assert.deepEqual(packageFor("win32", "x64"), { npm: "@keelstackhq/splice-win32-x64", bin: "splice.exe" });
  assert.equal(packageFor("linux", "ia32"), null);
});

test("unsupported platform message", () => {
  assert.throws(
    () =>
      binaryPath({
        resolve: () => {
          throw new Error("nf");
        },
        platform: "freebsd",
        arch: "x64",
        env: {},
      }),
    (e) => e.exitCode === 3 && /freebsd-x64.*linux-x64.*keel-splice/s.test(e.message),
  );
});

test("SPLICE_BINARY override", () => {
  assert.equal(binaryPath({ env: { SPLICE_BINARY: "/x/splice" } }), "/x/splice");
});

for (const code of [0, 1, 3]) {
  test(`exit ${code} passes through`, { skip: !posix }, () => {
    const r = run({ SPLICE_BINARY: fakeScript(`exit ${code}`, `exit${code}`) });
    assert.equal(r.status, code);
  });
}

test("arguments are forwarded without a shell", { skip: !posix }, () => {
  const bin = fakeScript('[ "$1" = "a b;c" ] && exit 0; exit 9', "args");
  const r = spawnSync(process.execPath, [launcher, "a b;c"], {
    env: { ...process.env, SPLICE_BINARY: bin },
  });
  assert.equal(r.status, 0);
});

test("signal is re-raised", { skip: !posix }, () => {
  const r = run({ SPLICE_BINARY: fakeScript("kill -TERM $$", "sig") });
  assert.equal(r.signal, "SIGTERM");
});

test("missing platform package exits 3", () => {
  const empty = join(scratch, "empty");
  mkdirSync(empty);
  const env = { ...process.env, NODE_PATH: empty };
  delete env.SPLICE_BINARY;
  const r = spawnSync(process.execPath, [launcher], { env, encoding: "utf8" });
  assert.equal(r.status, 3);
  assert.match(r.stderr, /^splice: no prebuilt binary/);
  assert.match(r.stderr, /keel-splice/);
});

test("platform package resolves via node_modules", { skip: !posix }, () => {
  const pkg = packageFor();
  assert.ok(pkg, "host platform must be supported for this test");
  const dir = join(scratch, "nm", "node_modules", ...pkg.npm.split("/"));
  mkdirSync(join(dir, "bin"), { recursive: true });
  writeFileSync(join(dir, "package.json"), JSON.stringify({ name: pkg.npm, version: "0.0.0" }));
  const bin = join(dir, "bin", pkg.bin);
  writeFileSync(bin, "#!/bin/sh\nexit 3\n");
  chmodSync(bin, 0o755);
  const env = { ...process.env, NODE_PATH: join(scratch, "nm", "node_modules") };
  delete env.SPLICE_BINARY;
  const r = spawnSync(process.execPath, [launcher], { env, encoding: "utf8" });
  assert.equal(r.status, 3);
  assert.equal(r.stderr, "");
});
