// After a release: move the Action's major tag (`v1`) to the release commit, tag-only, then check
// it is there. `--tag <v<semver>>`. Releases of splice 0.x and 1.x share the Action major `v1`;
// from 2.0.0 on the major is `v<major>`. Only a stable release that is the newest of its major
// moves the tag, so a prerelease or a re-released older version never moves it backwards.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const SEMVER = /^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

/** The Action major tag for release tag `tag`, or "" when `tag` is not `v<semver>`. */
export function actionMajor(tag) {
  const m = tag.match(SEMVER);
  if (!m) return "";
  return Number(m[1]) <= 1 ? "v1" : `v${m[1]}`;
}

/** name → commit from `git ls-remote --tags` output; an annotated tag's peeled `^{}` line wins. */
function commits(remoteTags) {
  const plain = new Map();
  const peeled = new Map();
  for (const line of remoteTags.split("\n")) {
    const [sha, ref] = line.split("\t");
    if (!sha || !ref?.startsWith("refs/tags/")) continue;
    const name = ref.slice("refs/tags/".length);
    if (name.endsWith("^{}")) peeled.set(name.slice(0, -3), sha);
    else plain.set(name, sha);
  }
  for (const [name, sha] of peeled) plain.set(name, sha);
  return plain;
}

const short = (sha) => sha.slice(0, 7);
const triple = (m) => [Number(m[1]), Number(m[2]), Number(m[3])];
const newer = (a, b) => {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
};

/**
 * Whether to move the major tag for release `tag`. Returns `{ major, commit, move, reason }`:
 * `commit` is the release commit, `move` is true when the major tag is missing or elsewhere and
 * `tag` is a stable release with no newer stable release of the same major on the remote.
 */
export function majorTagPlan({ tag, remoteTags }) {
  const m = tag.match(SEMVER);
  if (!m) throw new Error(`${tag} is not a release tag v<semver>`);
  const major = actionMajor(tag);
  const all = commits(remoteTags);
  const commit = all.get(tag);
  if (!commit) throw new Error(`${tag} is not on the remote`);
  const plan = (move, reason) => ({ major, commit, move, reason });
  if (m[4]) return plan(false, `${tag} is a prerelease: ${major} stays`);
  for (const name of all.keys()) {
    const t = name.match(SEMVER);
    if (t && !t[4] && actionMajor(name) === major && newer(triple(t), triple(m))) {
      return plan(false, `${name} is newer than ${tag}: ${major} stays`);
    }
  }
  const at = all.get(major);
  if (at === commit) return plan(false, `${major} is already on ${tag} (${short(commit)})`);
  const from = at ? `${major} is at ${short(at)}` : `${major} is missing`;
  return plan(true, `${from}: move it to ${tag} (${short(commit)})`);
}

/**
 * The completeness check: "" when `major` is on the remote (on `commit` when given), else the
 * problem.
 */
export function majorTagVerify({ major, commit, remoteTags }) {
  const at = commits(remoteTags).get(major);
  if (!at) return `${major} is missing on the remote`;
  if (commit && at !== commit) return `${major} is at ${short(at)}, not ${short(commit)}`;
  return "";
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const git = (...args) => execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
  try {
    const i = process.argv.indexOf("--tag");
    const tag = i > 0 ? (process.argv[i + 1] ?? "") : "";
    const before = git("ls-remote", "--tags", "origin", "refs/tags/v*");
    const plan = majorTagPlan({ tag, remoteTags: before });
    console.log(plan.reason);
    if (plan.move) {
      const ref = `refs/tags/${plan.major}`;
      const old = commits(before).get(plan.major) ?? "";
      // Tag-only push of a lightweight tag; the lease refuses if someone moved it meanwhile.
      git("fetch", "--no-tags", "--depth=1", "origin", plan.commit);
      git("push", `--force-with-lease=${ref}:${old}`, "origin", `${plan.commit}:${ref}`);
    }
    const after = git("ls-remote", "--tags", "origin", `refs/tags/${plan.major}`);
    // Not moved (prerelease, older release): the major tag must still exist.
    const want = plan.move || commits(before).get(plan.major) === plan.commit ? plan.commit : "";
    const problem = majorTagVerify({ major: plan.major, commit: want, remoteTags: after });
    if (problem) throw new Error(`release incomplete: ${problem}`);
    console.log(`${plan.major} → ${short(commits(after).get(plan.major))}`);
  } catch (e) {
    console.error(`release-major: ${e.message}`);
    process.exit(1);
  }
}
