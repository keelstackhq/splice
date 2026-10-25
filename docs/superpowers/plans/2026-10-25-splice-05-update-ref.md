# splice 05: `update --ref` and ancestry for untagged versions

**Goal:** fix canary finding F1 (keelstackhq/rust-canary `docs/canary-findings.md`): a repo rendered
at an untagged template commit (`0.0.0+g<sha7>`) can be updated. Add `update --ref R` and `diff
--ref R`; never order untagged versions by semver, use git ancestry instead.

**Spec:** main spec §3 (CLI table), §4 (`--ref`, untagged versions), §8.3 steps 2 and 7, §8.7, §8.8,
§10 item 21 — already written in the plan commit; read §10 item 21 first. README "Untagged
commits". **Contract:** `docs/template-format.md` is **unchanged**. Never edit it.

**Tech Stack:** crates already in `Cargo.toml` only (semver 1, clap 4). Git via the `git` CLI
(`merge-base --is-ancestor`). No new crates.

## Global Constraints

- Exit codes and the `--json` shape do not change. A non-descendant target is exit 2
  (`SpliceError::Usage`), before any write. Lock shape unchanged; lock still written last.
- Untagged = `!version.build.is_empty()`. Never compare an untagged version with `<`/`>`/`==`
  to decide newer/older; compare commits.
- Two tagged sides behave exactly as today (all existing tests stay green unchanged).
- Tracked channels keep B5 (no ancestry check on the branch head, no migrations); `--ref` on a
  tracked channel is exit 2 with the existing `--to` message.
- `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test` (no
  nextest in the cloud container) before each commit.

## Interfaces

```rust
// src/git.rs
impl Git {
    /// `git merge-base --is-ancestor <ancestor> <descendant>`: exit 0 → true, 1 → false,
    /// anything else → SpliceError::Env. A commit is its own ancestor.
    pub fn is_ancestor(git_dir: &Path, ancestor: &str, descendant: &str) -> Result<bool, SpliceError>;
}
// src/fetch.rs
/// A version with build metadata (`0.0.0+g<sha7>`): an untagged commit (spec §4).
pub fn is_untagged(v: &Version) -> bool;
impl Fetcher {
    pub fn is_ancestor(&self, src: &TemplateSource, ancestor: &str, descendant: &str) -> Result<bool>;
    /// Every `v<semver>` tag as (version, commit), ascending; prereleases only with `pre`.
    pub fn tag_commits(&self, src: &TemplateSource, pre: bool) -> Result<Vec<(Version, String)>>;
    /// The highest stable tag whose commit is a strict descendant of `commit` (spec §10 item 21).
    pub fn newest_descendant_tag(&self, src: &TemplateSource, commit: &str) -> Result<Option<ResolvedTemplate>>;
}
// src/ops/update.rs
pub enum Target<'a> { Latest, Version(&'a Version), Ref(&'a str), Branch(&'a str) }
/// `--to`/`--ref` → Target (Latest when neither).
pub fn target_of<'a>(to: Option<&'a Version>, git_ref: Option<&'a str>) -> Target<'a>;
pub fn prepare(g, target: Target, data, force) -> Result<Prepared>;            // was `to: Option<&Version>`
pub fn prepare_channel(g, cl, others, target: Target, filter_new, force, earlier) -> Result<Prepared>;
// Prepared gains `pub migrations: Vec<Version>`: the selected migration versions (steps and
// removes both use it; steps_of no longer calls `select` itself).
// src/cli.rs: UpdateArgs and DiffArgs gain
//   #[arg(long = "ref", value_name = "R", conflicts_with = "to")] pub git_ref: Option<String>
// and `update --all` conflicts with "git_ref" too.
```

## Task 1: `--ref`, the ancestry check, `--to` with build metadata

**Files:** `src/git.rs`, `src/fetch.rs`, `src/cli.rs`, `src/ops/update.rs`, `src/ops/diff.rs`,
`tests/git.rs` or `tests/fetch.rs`, `tests/cli_update.rs`, `tests/cli_diff.rs`,
`tests/cli_channel_update.rs`.

Tests first (each must fail before the change for the stated reason):

- [x] `Git::is_ancestor`: true for parent→child and for a commit and itself, false child→parent,
  Env error for an unknown sha.
- [x] `update --ref <untagged sha>` from a tagged lock (template tags v1.0.0, then an untagged
  descendant commit): exit 0, lock `version = "0.0.0+g<sha7>"`, `commit` = full sha, files of
  the new commit written.
- [x] F1 regression: two untagged commits X then Y; `new --ref X`; `update --ref Y` succeeds,
  whatever the text order of the sha7s (assert it also when sha7(Y) < sha7(X) textually: pick
  or construct commits so that holds, or assert via a lock edited to a smaller sha7 text).
- [x] `update --ref X` from a lock at Y (non-descendant): exit 2, message contains "not a
  descendant" and "--force", lock bytes unchanged; with `--force`: exit 0, lock at X.
- [x] `update --ref <lock commit>`: exit 0, "up to date", nothing written.
- [x] `update --ref v1.0.0` from a lock at a later commit: exit 2 (ancestry, also for tags).
- [x] `update --to 0.0.0+gabc1234`: exit 2, message names `--ref abc1234`.
- [x] untagged lock + `--to 2.0.0` where tag v2.0.0 is not a descendant: exit 2 "not a
  descendant"; where it is a descendant: exit 0.
- [x] `--ref` with `--to`: clap exit 2; `update --all --ref x`: exit 2.
- [x] `diff --ref <sha>` prints the plan (changes equal `update --ref --json`); non-descendant
  exit 2.
- [x] `update --channel N --ref <sha>` on a `--ref`-pinned channel works; on a tracked channel
  exit 2 with "tracks".

Implement in `prepare_lock`: a `force: bool` parameter; `Target::Ref(r)` resolves with
`fetcher.resolve(&src, Some(r), false)`; for `Ref`, and for `Version` when the lock is untagged,
check `is_ancestor(lock.commit, target.commit)` (same commit → up to date); non-descendant →
`Usage` unless `force` (then a message `--force: <t7> is not a descendant of the locked <l7>;
updating anyway`). The channel lock path goes into the message as today (`at`). The moved-tag
message only for a tagged lock. `parse_to` refuses build metadata.

Commit: `feat: update --ref with an ancestry check (F1)`

## Task 2: latest and `check` from an untagged lock; migrations by ancestry

**Files:** `src/fetch.rs`, `src/ops/update.rs`, `src/ops/check.rs`, `tests/cli_update.rs`,
`tests/cli_check.rs`, `tests/golden_update.rs` (+ snapshot), `docs/PROGRESS.md`.

Tests first:

- [ ] Golden F1 (`golden_update.rs`): tags v1.0.0 (fixture v1), then an untagged commit with
  fixture v2 content (X), `new --ref X`. `update` (no flags): exit 0, "no stable tag descends",
  lock unchanged (today it moves to v1.0.0). `check`: exit 0, no "update available" (today exit
  1). Then tag `v2.0.0` on a new descendant commit: `check` exit 1 with `update available:
  0.0.0+g<x7> → 2.0.0`; `update` moves to 2.0.0; JSON report snapshot.
- [ ] Migrations: tags v1.0.0 (v1), v2.0.0 (v2, has migration 2.0.0), untagged descendant Y (v2-x
  or v2 content edited). Lock at v1.0.0, `update --ref Y`: the 2.0.0 step runs
  (`.splice/migrated.txt`) and its `[[remove]]` applies. Lock at an untagged commit after
  v2.0.0, `update --ref` later: the 2.0.0 step does not run. A migration file without its tag
  does not run when a side is untagged.
- [ ] Channel: an untagged (`channel add --ref <sha>`) channel lock with only an older stable
  tag: `check` reports no update, `update --channel` up to date; `update --all` likewise.
- [ ] Tagged → tagged selection unchanged (existing tests).

Implement: `Target::Latest` from an untagged lock uses `newest_descendant_tag`; `no_newer`
message for an untagged lock per spec §4; `check::newer_stable` and `channel_update` (untracked)
use `newest_descendant_tag` when the lock is untagged. Migration selection per §10 item 21 into
`Prepared.migrations` (removes and steps).

Commit: `feat: untagged locks follow git ancestry for latest, check and migrations (F1)`

## Review Focus

- No `<`/`>` between versions where either is untagged (grep `version <`, `version >`, `< &from`).
- Ancestry refusal happens before any write; `diff` never writes; JSON stays one object.
- Tracked channels unchanged (B5); tagged → tagged unchanged.
