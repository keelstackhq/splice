// Release tag decision for a push to main: which tag `v<version>` this run releases, and whether
// it must create it first (see `releaseTag`). `--previous <tag>`: the release notes' start tag.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { checkVersion } from "./check-version.mjs";

/**
 * `remoteTags` is `git ls-remote --tags` output (plain and peeled `^{}` lines). Returns the tag and
 * whether to create it: a missing tag is created; a tag on `sha` (the run's own commit) is
 * released again without creating it only on a re-run (`attempt` > 1, "Re-run all jobs" after a
 * failed publish). On a first attempt an existing tag always releases nothing (`tag: ""`): a tag
 * a person pushed has its own tag-push run.
 */
export function releaseTag({ version, remoteTags, sha, attempt = 1 }) {
  const tag = `v${version}`;
  const ref = `refs/tags/${tag}`;
  let plain = null;
  let peeled = null;
  for (const line of remoteTags.split("\n")) {
    const [commit, name] = line.split("\t");
    if (name === ref) plain = commit;
    if (name === `${ref}^{}`) peeled = commit;
  }
  const at = peeled ?? plain;
  if (at === null) return { tag, create: true };
  if (sha && at === sha && attempt > 1) return { tag, create: false };
  return { tag: "", create: false };
}

const SEMVER = /^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

/** Semver order of two parsed tags (build metadata is not used in release tags). */
function compare(a, b) {
  for (let i = 1; i <= 3; i++) {
    if (a[i] !== b[i]) return Number(a[i]) - Number(b[i]);
  }
  if (!a[4] || !b[4]) return (a[4] ? -1 : 0) - (b[4] ? -1 : 0);
  const x = a[4].split(".");
  const y = b[4].split(".");
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if (x[i] === undefined) return -1;
    if (y[i] === undefined) return 1;
    const [n, m] = [/^\d+$/.test(x[i]), /^\d+$/.test(y[i])];
    if (n && m && x[i] !== y[i]) return Number(x[i]) - Number(y[i]);
    if (n !== m) return n ? -1 : 1;
    if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  }
  return 0;
}

/**
 * The release before `tag` among the `v<semver>` tags in `remoteTags` (`git ls-remote --tags`),
 * for `gh release create --notes-start-tag`: the highest lower one, stable only when `tag` is
 * stable. Other tags (the Action's `v1`) never count. "" when there is none.
 */
export function previousTag(tag, remoteTags) {
  const me = tag.match(SEMVER);
  if (!me) return "";
  let best = null;
  for (const line of remoteTags.split("\n")) {
    const name = (line.split("\t")[1] ?? "").replace(/^refs\/tags\//, "");
    const t = name.match(SEMVER);
    if (!t || name.endsWith("^{}") || compare(t, me) >= 0 || (!me[4] && t[4])) continue;
    if (!best || compare(t, best) > 0) best = t;
  }
  return best ? best[0] : "";
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const i = process.argv.indexOf("--previous");
  if (i > 0) {
    try {
      const remoteTags = execFileSync("git", ["ls-remote", "--tags", "origin", "refs/tags/v*"], { encoding: "utf8" });
      const previous = previousTag(process.argv[i + 1] ?? "", remoteTags);
      console.log(previous ? `release notes start at ${previous}` : "no earlier release: default notes");
      if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `previous=${previous}\n`);
    } catch (e) {
      console.error(`release-tag: ${e.message}`);
      process.exit(1);
    }
    process.exit(0);
  }
  try {
    const pkgPath = new URL("../npm/splice/package.json", import.meta.url);
    const version = checkVersion({
      tag: null,
      cargoToml: readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8"),
      launcherPkg: existsSync(pkgPath) ? JSON.parse(readFileSync(pkgPath, "utf8")) : undefined,
    });
    const remoteTags = execFileSync("git", ["ls-remote", "--tags", "origin", `refs/tags/v${version}*`], {
      encoding: "utf8",
    });
    const sha = process.env.GITHUB_SHA ?? "";
    const attempt = Number(process.env.GITHUB_RUN_ATTEMPT ?? "1");
    const { tag, create } = releaseTag({ version, remoteTags, sha, attempt });
    console.log(
      create
        ? `${tag} is not tagged yet: tag and release it`
        : tag
          ? `${tag} already tags this commit: release it again (a re-run)`
          : `v${version} is already tagged (by a person or an earlier run): nothing to release`,
    );
    if (process.env.GITHUB_OUTPUT) {
      appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\ntag=${tag}\ncreate=${create}\n`);
    }
  } catch (e) {
    console.error(`release-tag: ${e.message}`);
    process.exit(1);
  }
}
