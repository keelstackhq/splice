# splice Milestone 4 — channels ("rigging") — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply extra format-1 templates ("channels") next to the main template (or alone), each with its own lock in `.splice/channels/<name>.toml`, piece selection, per-repo `--only`/`--exclude` filter, optional `--track BRANCH`, and owned Markdown **sections** inside files nobody owns; `update`, `check`, `diff`, `explain` and `add` learn `--channel`/`--all`; version 0.2.0 (the npm release follows plan 03).

**Architecture:** A channel is planned exactly like `update` (spec §8.3–8.4): render base (lock commit, old filter) and theirs (target, new filter) into memory, `planner::plan` turns them into `Planned` changes, and a new pure section planner turns rendered fragments + host files into `SectionChange`s. A source registry loads every lock (main + channels) and a claim check rejects any path owned by two sources before anything is written. `apply` is split into `apply_files` (files → section hosts → removals → steps) and the existing `apply` (= `apply_files` → post → lock, unchanged order), with a `LockOut` (main or channel lock). `update --all` plans every source in memory, checks claims on the combined result, runs `apply_files` per source, then the deduplicated post once, then writes every lock last (`write_locks`).

**Tech Stack:** only crates already in `Cargo.toml`: clap 4.6 (derive; `conflicts_with`, nested subcommand), globset 0.4 (filters, `literal_separator(true)` as `classify.rs`), toml 1.1 / toml_edit 0.25 (channel lock read/write), sha2 0.11 (section sha), indexmap 2, semver 1, minijinja 2.24 (fragments, same `Renderer`), tempfile 3, walkdir 2.5; dev: assert_cmd 2.2, insta 1.49 (json), tempfile. **No new crates.** Git via the `git` CLI only. `cargo insta` is not installed: snapshots are reviewed as `.snap.new` and accepted with `INSTA_UPDATE=always`.

**Spec:** `docs/superpowers/specs/2026-10-17-splice-channels-design.md` (approved; all) · main spec `2026-10-14-splice-design.md` §3, §6, §8.3–8.9, §10, §13 · **Contract:** `docs/template-format.md` — **unchanged by this plan** (channels spec §4). Never edit it here.

**Depends on plan 03 (not started at 999179b):** Task 19's SKILL.md part, Task 20, the npm part of Task 21, the 0.2.0 release, and C3. Each is marked **after plan 03**; until then it stays unchecked with a PROGRESS Blocked line `plan 03 not executed`.

## Global Constraints

- Everything in plans 01 and 02's Global Constraints (exit codes, `--json` one object, lock last and atomic, dirty tree → exit 2, owned drift → nothing written, exit 1).
- `.splice/lock.toml` is byte-for-byte what M2 writes, with or without channels present (guard tests in Tasks 1, 12, 18). The main lock never gets `[channel]` or `[sections]`.
- Channel lock = contract §6 shape + optional `[channel]` (after `splice`) and `[sections]` (after `[files]`); written with `fsio::write_atomic` to `.splice/channels/<name>.toml`, only by that channel's own operation (or `update --all`), always after every file, step and post of the operation.
- Channel names `^[a-z][a-z0-9-]*$`. Unknown, duplicate or bad name → exit 2.
- One owner per path: a path in two sources' owned/shared sets, or a section host in any source's `[files]`, → exit 2 with `path X is owned by <main|channel NAME>; exclude it with --exclude X or remove it from one source`; nothing written. Main `update`/`add` run the check too when channel locks exist (B4).
- Reserved paths for every source: `.splice/lock.toml` (as today) and `.splice/channels/**`. Other `.splice/` paths stay renderable (contract §7 ships `.splice/migrations/*` as owned).
- A channel never sees the main template's vars or pieces; render context is its own lock.
- Section markers exactly `<!-- splice:<channel>:begin — managed by \`splice update --channel <channel>\`; edit the source -->` and `<!-- splice:<channel>:end -->` (the dash is U+2014).
- Source attribution in JSON goes in `messages` only (`source=main`, `source=channel:NAME`, `channel NAME: …`), never extra fields. `command` values: `channel add`, `channel remove`, `update`, `check`, `diff`, `explain`, `add`.
- `splice update` without `--channel`/`--all` behaves exactly as in M2 in a repo without channels (all existing tests stay green unchanged); `--to` keeps working for it.
- Error texts for the main lock stay M2's (`the lock still records 1.0.0`); a channel appends its path (`the lock still records 1.0.0 (.splice/channels/rig.toml)`).
- Tests are hermetic via `tests/common` (`splice_cmd`, `template_repo*`, the new `channel_repo*`, `chan_commit`); no network. Git-using tests live in `tests/`, never as in-crate unit tests (no global git identity there).
- Spec follows behaviour (B6): a task that changes behaviour described in the main spec updates that section in its own commit (named in its **Spec:** line). The channels spec already records every ruling below (commit `docs: revise channels plan after pre-flight scan`).
- Cross-repo tasks (C1–C3) are done in the named repo, in their own PRs; human-only items go to `docs/PROGRESS.md` → Blocked.

## Rulings on spec gaps (indexed in main spec §10 item 15 in Task 19)

- **G1** The channels spec cited "main spec §10 item 10", which already exists (subset gate). The channel decision is recorded as **item 15**, worded *splice-only; offered to hull at the next `format` bump, not required* (B7).
- **G2** (amended, B1) Reserved rendered paths for every source, including the main template: `.splice/lock.toml` and anything under `.splice/channels/`. Other `.splice/` paths (e.g. contract §7's `.splice/migrations/*.mjs`) stay allowed.
- **G3** (extended, B5) `--track`: the target is always the branch head (tags ignored); `--to` on a tracked channel → exit 2 (`channel N tracks BRANCH; re-add it with --ref to pin a version`). No ancestry check; `prepare_lock` with `Target::Branch` skips **every** version comparison (no downgrade check: semver orders `+g…` build metadata lexicographically); up to date = same subtree. Tracked channels run **no** migrations (`[[remove]]`, steps); files no longer rendered are removed by the B-only rule.
- **G4** Subtree compare (spec §8 `check`) also applies to `update --channel`: target subtree id equal to the lock commit's → "up to date", lock unchanged (no lock-only bot PRs). Main template: unchanged M2 behaviour.
- **G5** A `[sections]` key is a host path relative to the piece's `target`, rendered like a file path (`render_rel_path`); the value is a literal path under `files/`. The fragment file is excluded from the rendered tree. Host must end `.md`; fragment `.md` or `.md.jinja`. A fragment that renders to whitespace only declares no section.
- **G6** Section changes use path `HOST#CHANNEL`, class `owned`, in `changes` of every command (`create`/`update`/`delete`/`skip`/`conflict`), not only `check`.
- **G7** The block body is the fragment with exactly one trailing `\n`; written with the host's line ending (CRLF if the host contains `\r\n`); `sha256` is over the LF-normalised body.
- **G8** On update, fragment unchanged → skip even if the body was edited (`check` still reports it). Host already holding this channel's markers but no lock entry (adoption) → adopt if the body equals the fragment, else drift.
- **G9** `--force` never overrides duplicate/unbalanced markers (exit 1, user fixes the file).
- **G10** A seed path that another source locks in `[files]` is dropped from this source's plan (never written, no error).
- **G11** `channel add` with a shared-file conflict follows `update` (markers written, lock written, exit 1). `splice add --channel` follows `add` (C3: writes nothing on drift/conflict).
- **G12** (amended) `update --all`: (1) every source's files, sections, removals and steps (`apply_files`, main first, then channels by name); (2) the `[[post]]` of all sources with `when ∋ update`, deduplicated by exact `run` (first occurrence order), once, skipped on any conflict or `--no-post`; (3) every lock last (`write_locks`). A failure in (1) or (2) writes no lock: exit 3, the message names the source (`main` / `channel NAME`) or the post command, says every lock still records its old version, plus `git checkout -- . && git clean -fd`. Conflicts: locks still written, exit 1.
- **G13** Main lock containing `[channel]` or `[sections]`, a channel lock without `[channel]` or whose `name` ≠ file stem, or a file in `.splice/channels/` that is not `<valid-name>.toml` → exit 3 ("corrupt lock", names the file). Names starting with `.` (leftover `write_atomic` temp files) are ignored (C1).
- **G14** (amended, B3) Only `--only`/`--exclude` need `--channel`. `--to` is valid alone (main, contract §9.14) and with `--channel`. `--all` conflicts with `--channel`, `--to`, `--data`, `--only`, `--exclude` (clap → exit 2).
- **G15** Test fixtures use `min_splice = "0.1.0"`; the crate version moves to `0.2.0` in Task 21 (real channel templates in C1/C2 use `min_splice = "0.2.0"`).
- **G16** (A12) Any `--only`/`--exclude` on `update --channel` replaces **both** stored lists; `--only '**'` alone keeps everything.
- **G17** (A11) In `update --all`, the section planner's disk view of a host is the planned bytes of an earlier source's seed write to that path, and `created = false` then.

## Review Focus

1. **A channel and the main template both want `CLAUDE.md`** (owned by hull, channel section host) → `channel add` exits 2, names `main` and the `--exclude` flag; `git status --porcelain` empty. Task 10.
2. **A user edited text inside a section, then the channel releases a new fragment** → `update --channel` exits 1, nothing written in any file; `--force` replaces only the block, text outside the markers byte-identical (CRLF host stays CRLF). Tasks 7, 12.
3. **`update --all` where channel `b` has owned drift** → no file and no lock of main, `a` or `b` changes; exit 1; message names `channel b`. Task 14.
4. **`update --all` where source k's migration step (or the post) fails** → files of earlier sources may be written, but **every** lock still records its old version; exit 3; message names k (or the post command) and the restore command; restore + rerun succeeds. Task 14.
5. **`decisions` tracked on `main`, a commit lands that only touches files outside `channel/`** → `check` exit 0, no "update available"; a commit inside `channel/` → exit 1 with `channel decisions: update available 3f2c1ab → 9e01d2c (main)`. Tasks 13, 15.
6. **A repo with channels runs plain `splice update`/`new`** → `.splice/lock.toml` byte-identical to a channel-free repo; channel locks untouched; a main v2 that renders a channel-owned path → exit 2. Tasks 12, 14.

## Pre-flight rulings applied

Scan `.superpowers/sdd/2026-10-17-splice-04-channels/preflight.md` (against 999179b); rulings in `progress.md`. Task renumbering: old T7 (fixtures) → **T4**; old T4 → T5, T5 → T6, T6 → T7; T8–T21 keep their numbers.

| ID | Change |
|---|---|
| A1 | Fixtures + helpers moved before the manifest task (now Task 4). |
| A2 | Task 8 splits `apply` into `apply_files` + `apply`, adds `write_locks`; Task 14 uses them. |
| A3 | Task 4 adds fixture `tests/fixtures/channel-b/{v1,v2}` (name `rigb`); Task 14 tests use `rig` + `rigb`; Task 10 `name_defaults_and_as` uses a fresh repo for `--as rig2`. |
| A4 | Task 4 lists every fixture variant later tasks use (`template/v1-claude`, `template/v2-x`, `channel/v2-collide`, `channel/v2-x`, `lock/channel-rig.toml`). |
| A5 | Main error text unchanged; channels append the lock path; `src/post.rs` (`run_steps`) in Task 8 Files. |
| A6 | Task 1 guard reuses the existing redacted snapshot `roundtrip_matches_contract_shape`; no `main-m2.toml`. |
| A7 | Task 12 `update_channel_v1_to_v2` expects two lines in `.rig-post.log` (add + update). |
| A8 | Task 7 table gains a `created` column. |
| A9 | `Rendered.base_sections` dropped (Task 12). |
| A10 | `Claim::from_plan` filters out `Class::Seed` (Task 3). |
| A11 | G17; Task 14 overlays earlier seed writes; test `seed_then_section_same_host`. |
| A12 | G16; Task 12 `filter_change_on_update` rewritten. |
| B1 | G2 amended; Task 1 tests `.splice/migrations/x.mjs` renders, `.splice/channels/a.toml` rejected; channels spec §4. |
| B2 | G12 amended; channels spec §8 rewritten in this revision (change note); Task 14 steps, tests and Review Focus 4 rewritten. |
| B3 | G14 amended; Task 9 test `update_to_alone_is_valid`; channels spec §3. |
| B4 | Main `update` (Task 12) and `add` (Task 11) run `check_claims` when channel locks exist; tests added; channels spec §5. |
| B5 | G3 extended; Task 13 skips version comparisons and migrations for `Target::Branch`; test `tracked_head_sorting_lower_still_updates`; channels spec §3/§5. |
| B6 | Each behaviour task has a **Spec:** line for the main-spec section it changes; Task 19 only adds the §10 item 15 index, §15 and docs. |
| B7 | G1 wording; channels spec §4. |
| B8 | Channels spec status flipped to approved in this revision. |
| C1 | `read_all` ignores dot files (G13); test with `.tmpabc` (Task 1). |
| C2 | Task 13 makes `Fetcher::repo` `pub(crate)` (no new `git_dir`). |
| C3 | Task 8 drops `src/ops/new.rs`; `ApplyCtx.command` replaces the `When`-based report command. |
| C4 | Task 1: `bad_lock(path, why)` with a per-kind fix hint; `fetch::validate_commit(commit, lock_path)`. |
| C5 | Task 2: `Filter::from_meta` maps bad globs to `Template` (exit 3) naming the lock file. |
| C6 | Task 21 bumps only `Cargo.toml`/`Cargo.lock` (no snapshot holds `0.1.0`); node tests only after plan 03. |
| C7 | Task 18 uses `.snap.new` review + `INSTA_UPDATE=always`. |
| C8 | Tasks 12 and 16 start from `prepare`/`explain::run` at HEAD (`latest: Option<…>`, `--json`-only messages). |
| D1 | Task 13 `tree_id_of_subdir` lives in `tests/git.rs`. |
| D2 | Guard tests are marked **guard** (TDD exception: they pass before the change and must stay green). |
| D3 | Task 4 adds `chan_commit` (asserts exit 0, then commits); multi-command tests use it. |
| D4 | Task 20 marked after plan 03. |
| E1 | C2's `rigging/claude` has no `CLAUDE.md` section; channels spec §7. |
| E2 | berth added to C3 (`--piece import-claude`); channels spec §10. |
| E3 | C2 starts with a hull spec change + owner sign-off; `[sections]`/D-003 question in Blocked. |
| E4 | C1 adds `decisions/004-decisions-channel.md` + D-004 row; owner merges. |
| E5 | C3 after plan 03 + release; Blocked lists plan 03, `NPM_TOKEN`, npm scope owner. |
| E6 | hoist left out of C3 unless the owner asks. |

---

## File Structure

```
src/lock.rs           + ChannelMeta, SectionLock, ChannelLock, channel_lock_path; main-lock corruption check
src/paths.rs          `.splice/lock.toml` and `.splice/channels/**` rejected as rendered paths
src/filter.rs         Filter (only/exclude globs), keeps(), apply_tree()
src/sources.rs        SourceId, Sources::load, Claim, check_claims
src/sections.rs       markers, find_block, insert/replace/remove, body_sha, plan_sections
src/manifest.rs       PieceManifest.sections; validation
src/render.rs         render_sections; fragments excluded from render_tree
src/apply.rs          LockOut, apply_files, apply, write_locks, ApplyCtx.{command, sections}
src/post.rs           run_steps takes the "still records" text
src/fetch.rs src/git.rs   branch_head, tree_id, subtree_id (subtree compare); Fetcher::repo pub(crate)
src/cli.rs src/main.rs src/lib.rs   `channel add|remove`, --channel/--all/--only/--exclude
src/ops/channel.rs    channel add, channel remove
src/ops/{update,add,check,diff,explain}.rs   channel-aware
tests/fixtures/channel/{v1,v2,v2-collide,v2-x}/**   tests/fixtures/channel-b/{v1,v2}/**
tests/fixtures/template/{v1-claude,v2-x}/**   tests/fixtures/lock/channel-rig.toml
tests/fixtures/subset/section-fragment.*
tests/common/mod.rs   channel_repo(), channel_repo_tagged(), track_commit(), new_repo_at(), bare_repo(), chan(), chan_commit(), commit_all()
tests/cli_channel.rs  tests/cli_channel_update.rs  tests/golden_channels.rs
```

Channel fixture (`tests/fixtures/channel/v1`, `name = "rig"`, `base = "core"`, `min_splice = "0.1.0"`):

| Piece | v1 | v2 change | Test target |
|---|---|---|---|
| `core` | `[sections] "AGENTS.md" = "sections/rules.md.jinja"` (`{% if "claude" in pieces %}` line); owned `docs/agents/rules.md` | fragment line 2 edited | section replace / drift |
| `claude` | owned `.claude/agents/explorer.md`, `.claude/agents/reviewer.md`; `[sections] "CLAUDE.md" = "sections/pointer.md"` | `reviewer.md` edited; `explorer.md` dropped via `migrations/2.0.0.toml` `[[remove]]` | per-channel `[[remove]]` |
| `skills` | `multiple = true`, `target = ".{{ dir }}/skills"`, var `dir = { pattern = "^(claude\|agents)$" }`, owned `caveman/SKILL.md`, `verify/SKILL.md` | `verify/SKILL.md` edited | two instances, `--exclude` |
| `collide` | owned `.editorconfig` (the main fixture owns it) | — | claim conflict |
| `hooks` | owned `.claude/hooks/session-start.d/rig-sync.sh`; `[[post]] run = "echo rig-post >> .rig-post.log"`, `when = ["add", "update"]` | — | post dedup / order |

v2 also changes `core`'s seed `docs/agents/notes.md` (must stay untouched). `channel_repo(&["v1","v2"])` tags `v1.0.0`, `v2.0.0`; `track_commit(repo, inside: bool)` adds a commit on `main` that touches `channel/…` or only `README.md` outside it (the channel then lives under `//channel`, see Task 4). The `CLAUDE.md` section in the fixture's `claude` piece is test-only (it drives Review Focus 1); the real `rigging/claude` piece has none (E1).

Second channel fixture (`tests/fixtures/channel-b/v1`, `name = "rigb"`, `base = "notes"`, `min_splice = "0.1.0"`): piece `notes` owns `docs/agents/rigb.md` and `.claude/hooks/session-start.d/rigb-sync.sh` (disjoint from `rig`), `[[post]] run = "echo rig-post >> .rig-post.log"` (byte-identical to `rig`'s), `when = ["add", "update"]`, plus `[[post]] run = "grep -q 'version = \"1.0.0\"' .splice/channels/rigb.toml && echo old-lock >> .rig-post.log"`, `when = ["update"]`. v2: `docs/agents/rigb.md` edited, a new `[sections] "CHANGELOG.md" = "sections/log.md"` (the main fixture's v2 adds seed `CHANGELOG.md`: G17 test), and `migrations/2.0.0.toml` `[[step]] run = "test -z \"$SPLICE_FIXTURE_FAIL\""`.

Fixture variants (all created in Task 4):

| Path | Differs from its base by | Used in |
|---|---|---|
| `tests/fixtures/template/v1-claude` | main v1 plus an owned `CLAUDE.md` | Task 10 `host_owned_by_main_refused` |
| `tests/fixtures/template/v2-x` | main v2 plus an owned `x.txt` | Tasks 12, 14 |
| `tests/fixtures/channel/v2-collide` | channel v2 where `claude` also renders `.editorconfig` | Task 12 `claim_conflict_on_update` |
| `tests/fixtures/channel/v2-x` | channel v2 where `core` also renders owned `x.txt` | Task 14 |
| `tests/fixtures/lock/channel-rig.toml` | a hand-written channel lock (`[channel] name = "rig"`) | Task 12 guard |

---

### Task 1: Channel lock types, corruption checks, reserved `.splice/` paths

**Files:**
- Modify: `src/lock.rs`, `src/paths.rs`, `src/fetch.rs` (`validate_commit` takes the lock path)
- Test: unit tests in `src/lock.rs`, `src/paths.rs`
- **Spec:** main spec §6 path rule (reserved `.splice/lock.toml` + `.splice/channels/**`)

**Interfaces:**
- Consumes: `Lock`, `fsio::{read_opt, write_atomic}`, `toml_edit`
- Produces:
  - `pub const CHANNELS_DIR: &str = ".splice/channels";` `pub fn channel_lock_path(name: &str) -> String` (`.splice/channels/<name>.toml`)
  - `pub fn valid_channel_name(name: &str) -> bool` (`^[a-z][a-z0-9-]*$`)
  - `#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct ChannelMeta { pub name: String, pub track: Option<String>, pub only: Vec<String>, pub exclude: Vec<String> }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SectionLock { pub sha256: String, pub created: bool }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct ChannelLock { pub meta: ChannelMeta, pub lock: Lock, pub sections: BTreeMap<String, SectionLock> }`
  - `impl ChannelLock { pub fn read(repo: &Path, name: &str) -> Result<Option<ChannelLock>>; pub fn read_all(repo: &Path) -> Result<Vec<ChannelLock>> /* sorted by name; ignores names starting with "." */; pub fn write_atomic(&self, repo: &Path) -> Result<()>; pub fn path(&self) -> String; pub fn to_toml(&self) -> String }`
  - `Lock::read` refactored onto a private `fn parse_lock(text: &str, path: &str, hint: &str) -> Result<(Lock, toml::Table)>`; `bad_lock(path: &str, hint: &str, why)` names the file and the fix (`re-run \`splice new\`` for the main lock, `re-run \`splice channel add\`` for a channel lock). `Lock::read` rejects `[channel]`/`[sections]` (G13).
  - `fetch::validate_commit(commit: &str, lock_path: &str) -> Result<()>` (callers in `Lock::read`/`ChannelLock::read` pass their path).
  - `paths::render_rel_path` rejects `.splice/lock.toml` (as today) and any path whose first two segments are `.splice/channels` (G2).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn channel_lock_round_trip() {
    let cl = ChannelLock { meta: ChannelMeta { name: "decisions".into(), track: Some("main".into()),
        only: vec![], exclude: vec![".claude/skills/caveman/**".into()] },
        lock: lock_at("0.0.0+g3f2c1ab"), sections: [("docs/agents/project.md".into(),
        SectionLock { sha256: "ab".repeat(32), created: false })].into() };
    let dir = tempfile::tempdir().unwrap();
    cl.write_atomic(dir.path()).unwrap();
    assert!(dir.path().join(".splice/channels/decisions.toml").is_file());
    assert_eq!(ChannelLock::read(dir.path(), "decisions").unwrap().unwrap(), cl);
}
#[test] fn channel_lock_table_order() {
    let text = sample_channel_lock().to_toml();
    let order: Vec<_> = ["format =", "splice =", "[channel]", "[template]", "[vars]", "[[pieces]]", "[files]", "[sections]"]
        .iter().map(|k| text.find(k).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
}
// guard (passes before the change; must stay green): the existing redacted insta snapshot
// `roundtrip_matches_contract_shape` is not modified by this task — no new main-lock fixture file.
#[test] fn main_lock_with_channel_table_is_corrupt() { /* write [channel] into .splice/lock.toml → Lock::read exit 3, message names .splice/lock.toml and `splice new` */ }
#[test] fn main_lock_with_sections_is_corrupt() { /* same for [sections] */ }
#[test] fn channel_lock_without_channel_table_is_corrupt() { /* exit 3, names .splice/channels/x.toml and `splice channel add` */ }
#[test] fn channel_name_must_match_file() { /* [channel] name = "y" in x.toml → exit 3 */ }
#[test] fn bad_commit_names_channel_lock() { /* commit = "zz" in x.toml → exit 3, message names .splice/channels/x.toml */ }
#[test] fn stray_file_in_channels_dir_is_corrupt() { /* .splice/channels/Bad.toml or notes.txt → read_all exit 3 */ }
#[test] fn dot_files_in_channels_dir_ignored() { /* .splice/channels/.tmpabc next to a.toml → read_all == [a] */ }
#[test] fn channel_names() {
    for ok in ["a", "decisions", "rig-2"] { assert!(valid_channel_name(ok)); }
    for bad in ["", "A", "1a", "-a", "a_b", "a/b", "a.b"] { assert!(!valid_channel_name(bad)); }
}
// paths.rs
#[test] fn reserved_splice_paths() {
    for (src, target) in [(".splice/channels/a.toml", None), ("channels/a.toml", Some(".splice")), (".splice/lock.toml", None)] {
        assert_eq!(render_rel_path(src, target, &Renderer::new(), &context!{}).unwrap_err().exit_code(), 3);
    }
    for ok in [".splice/migrations/x.mjs", ".splicex/a", ".splice/channelsx/a"] {   // contract §7 example stays valid
        assert!(render_rel_path(ok, None, &Renderer::new(), &context!{}).is_ok(), "{ok}");
    }
}
```

- [x] **Step 2: Run `cargo nextest run -E 'test(lock) | test(paths)'` — FAIL**
- [x] **Step 3: Implement.** `ChannelLock::to_toml` builds the main document via a shared private `Lock::to_document()` and inserts `channel` after `splice` and `sections` after `files` (toml_edit `position`). `[sections]` entries are inline tables `{ sha256, created }`; `only`/`exclude` always written (empty arrays allowed). Update main spec §6's path rule.
- [x] **Step 4: Run — PASS; `cargo nextest run` all green (existing lock tests and snapshots unchanged)**
- [x] **Step 5: Commit** — `feat: channel lock type and reserved .splice/ paths`

---

### Task 2: Filters

**Files:**
- Create: `src/filter.rs`; Modify: `src/lib.rs`
- Test: unit tests in `src/filter.rs`

**Interfaces:**
- Consumes: `globset`, `RenderedTree`
- Produces:
  - `pub struct Filter { only: Vec<(String, GlobMatcher)>, exclude: Vec<(String, GlobMatcher)> }`
  - `impl Filter { pub fn new(only: &[String], exclude: &[String]) -> Result<Filter> /* bad glob → Usage, names the flag */; pub fn from_meta(m: &ChannelMeta, lock_path: &str) -> Result<Filter> /* bad stored glob → Template (exit 3) naming the lock file */; pub fn keeps(&self, path: &str) -> bool; pub fn apply_tree(&self, tree: RenderedTree) -> RenderedTree; pub fn unused(&self, paths: &[&str]) -> Vec<String> /* "filter `G` matches nothing" */ }`

- [x] **Step 1: Write failing tests**

```rust
#[test] fn empty_filter_keeps_everything() { assert!(Filter::new(&[], &[]).unwrap().keeps("a/b.md")); }
#[test] fn only_then_exclude() {
    let f = Filter::new(&s(&[".claude/**"]), &s(&[".claude/skills/caveman/**"])).unwrap();
    assert!(f.keeps(".claude/agents/x.md"));
    assert!(!f.keeps(".claude/skills/caveman/SKILL.md"));
    assert!(!f.keeps("AGENTS.md"));
}
#[test] fn star_does_not_cross_slash() {
    let f = Filter::new(&[], &s(&[".claude/*"])).unwrap();
    assert!(!f.keeps(".claude/a.md")); assert!(f.keeps(".claude/agents/a.md"));
}
#[test] fn bad_glob_is_usage() { assert_eq!(Filter::new(&s(&["a/[b"]), &[]).unwrap_err().exit_code(), 2); }
#[test] fn bad_stored_glob_is_corrupt_lock() { /* ChannelMeta{exclude:["a/[b"]} → from_meta exit 3, message names .splice/channels/rig.toml */ }
#[test] fn unused_globs_reported() {
    let f = Filter::new(&[], &s(&["nope/**", "a.md"])).unwrap();
    assert_eq!(f.unused(&["a.md"]), vec!["filter `nope/**` matches nothing".to_string()]);
}
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement** (globs built like `classify::compile_one`, `literal_separator(true)`). **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: channel path filters`

---

### Task 3: Source registry and claim check

**Files:**
- Create: `src/sources.rs`; Modify: `src/lib.rs`
- Test: unit tests in `src/sources.rs`

**Interfaces:**
- Consumes: `Lock::read`, `ChannelLock::read_all`
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)] pub enum SourceId { Main, Channel(String) }` with `Display`: `main` / `channel NAME`; `fn tag(&self) -> String`: `source=main` / `source=channel:NAME`
  - `pub struct Sources { pub main: Option<Lock>, pub channels: Vec<ChannelLock> }`
  - `impl Sources { pub fn load(repo: &Path) -> Result<Sources>; pub fn channel(&self, name: &str) -> Result<&ChannelLock> /* unknown → Usage listing names */; pub fn claims(&self) -> Vec<Claim> /* current state */ }`
  - `#[derive(Debug, Clone)] pub struct Claim { pub source: SourceId, pub files: BTreeSet<String> /* owned+shared */, pub hosts: BTreeSet<String> }`
  - `impl Claim { pub fn from_lock(source: SourceId, lock: &Lock, sections: &BTreeMap<String, SectionLock>) -> Claim; pub fn from_plan(source: SourceId, theirs: &RenderedTree, hosts: impl IntoIterator<Item = String>) -> Claim /* filters out Class::Seed (G10) */ }`
  - `pub fn check_claims(claims: &[Claim]) -> Result<()>` — Usage (exit 2); one message line per conflicting path, sorted.
  - `pub fn replace_claim(current: Vec<Claim>, new: Claim) -> Vec<Claim>` (swap the source's current claim for its planned one)

- [x] **Step 1: Write failing tests**

```rust
#[test] fn disjoint_claims_pass() { check_claims(&[claim(Main, &["a"], &[]), claim(ch("x"), &["b"], &["AGENTS.md"])]).unwrap(); }
#[test] fn same_path_two_sources() {
    let e = check_claims(&[claim(Main, &[".editorconfig"], &[]), claim(ch("rig"), &[".editorconfig"], &[])]).unwrap_err();
    assert_eq!(e.exit_code(), 2);
    assert_eq!(e.to_string(), "path .editorconfig is owned by main; exclude it with --exclude .editorconfig or remove it from one source");
}
#[test] fn host_owned_by_other_source() { /* main files {CLAUDE.md}, rig hosts {CLAUDE.md} → exit 2, "owned by main" */ }
#[test] fn host_in_own_files_is_conflict() { /* rig files {AGENTS.md} + rig hosts {AGENTS.md} → exit 2 "owned by channel rig" */ }
#[test] fn two_channels_one_host_is_fine() { /* rig hosts {AGENTS.md}, decisions hosts {AGENTS.md} → ok */ }
#[test] fn seeds_not_claimed() { /* from_plan with a seed path → files excludes it */ }
#[test] fn load_reads_main_and_channels_sorted() { /* temp repo with lock + channels b, a → names [a, b] */ }
#[test] fn unknown_channel_lists_known() { /* Sources.channel("zz") exit 2, message contains "a, b" */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** The message names the *other* source than the one being checked: claims are checked in order, and the earlier claim holder is named. **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: source registry and one-owner claim check`

---

### Task 4: Channel fixtures and test helpers

(Was Task 7; moved before the manifest task so every later test has its fixtures, A1.)

**Files:**
- Create: `tests/fixtures/channel/{v1,v2,v2-collide,v2-x}/**`, `tests/fixtures/channel-b/{v1,v2}/**`, `tests/fixtures/template/{v1-claude,v2-x}/**`, `tests/fixtures/lock/channel-rig.toml` (tables in File Structure)
- Modify: `tests/common/mod.rs` (helpers below; `golden_update::{new_at, commit_all}` move here, callers in `tests/golden_update.rs` and the per-file `commit_all` copies switch to the common one)
- Test: `tests/manifest.rs`, `tests/git.rs`

**Interfaces:**
- Produces (tests/common):
  - `pub fn channel_repo(versions: &[&str]) -> TempDir` = `channel_repo_tagged("channel", &[(v, "v<N>.0.0")…])`.
  - `pub fn channel_repo_tagged(fixture: &str /* "channel" | "channel-b" */, versions: &[(&str, &str)]) -> TempDir` — a git repo whose `channel/` subdir holds `tests/fixtures/<fixture>/<dir>`, plus a top-level `README.md`; each version committed on `main` and tagged. Source spec in tests: `format!("{}//channel", repo.path().display())`.
  - `pub fn track_commit(repo: &Path, inside: bool) -> String` — new commit on `main` (touching `channel/pieces/core/files/docs/agents/rules.md` when `inside`, else `README.md`); returns the full sha.
  - `pub fn new_repo_at(tpl: &TempDir, tag: &str) -> TempDir` — `splice new` of the main fixture at `tag`, `git init` + commit (moved from `golden_update::new_at`).
  - `pub fn bare_repo() -> TempDir` — an empty git repo with one commit (`README.md`), no lock.
  - `pub fn commit_all(repo: &Path)` (moved from the test files).
  - `pub fn chan(repo: &Path, args: &[&str]) -> assert_cmd::assert::Assert` (`splice_cmd -C repo --yes --json` + args; no commit).
  - `pub fn chan_commit(repo: &Path, args: &[&str]) -> assert_cmd::assert::Assert` — `chan`, asserts exit 0, then `commit_all` so the next command sees a clean tree (D3).

- [x] **Step 1: Write failing tests** — `channel_fixture_v1_loads` (pieces `claude, collide, core, hooks, skills`; v2 migration removes `.claude/agents/explorer.md`; the only warnings are `unknown key \`sections\` ignored` until Task 5); `channel_b_fixture_loads` (name `rigb`, two `[[post]]`, v2 step); `channel_repo_has_subdir_and_tags` in `tests/git.rs` (`git tag` lists both; `channel/splice.toml` exists; `track_commit(_, false)` leaves `git rev-parse HEAD:channel` unchanged, `track_commit(_, true)` changes it); `chan_commit_leaves_clean_tree`.
- [x] **Step 2: Run `cargo nextest run --test manifest --test git` — FAIL.**  **Step 3: Write fixtures and helpers.** **Step 4: Run `cargo nextest run` — PASS (existing tests unchanged after the helper move).**
- [x] **Step 5: Commit** — `test: channel fixtures and helpers`

---

### Task 5: `[sections]` in piece manifests, fragment rendering, main-template refusal

**Files:**
- Modify: `src/manifest.rs`, `src/render.rs`, `src/ops/new.rs`, `src/ops/add.rs`, `src/ops/update.rs`, `tests/conformance.rs`
- Create: `tests/fixtures/subset/section-fragment.{jinja,ctx.json,out}`
- Test: `tests/manifest.rs`, `tests/render_tree.rs`, `tests/cli_new.rs`, `tests/conformance.rs`

**Interfaces:**
- Produces:
  - `PieceManifest { …, pub sections: IndexMap<String /* host key */, String /* fragment rel under files/ */> }`; `"sections"` added to the known piece keys
  - `Template::has_sections(&self, ids: &[&str]) -> Option<&str>` (first piece id with sections)
  - `pub struct RenderedSection { pub host: String, pub body: String, pub piece_id: String, pub piece_name: String }`; `pub type RenderedSections = BTreeMap<String /* host */, RenderedSection>`
  - `pub fn render_sections(t: &Template, input: &RenderInput) -> Result<RenderedSections>`; `render_tree` skips every file that a piece lists as a fragment.
  - `new`, `add`, `update` (main) → Template error (exit 3) `piece \`X\` declares [sections]; sections are only applied by \`splice channel add\``.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn sections_parsed() { /* channel fixture v1 (Task 4) core: sections["AGENTS.md"] == "sections/rules.md.jinja"; no warnings (the Task 4 `unknown key` warning is gone) */ }
#[test] fn non_md_host_or_fragment_rejected() { /* "AGENTS.txt" = …, or fragment "x.txt" → exit 3 naming piece.toml */ }
#[test] fn missing_fragment_rejected() { /* fragment path not under files/ → exit 3 */ }
#[test] fn fragment_not_in_tree() { /* render_tree(channel v1, core) has no key starting with "sections/" */ }
#[test] fn sections_render_with_target_and_pieces() {
    // core + claude: AGENTS.md body contains the claude-only line; without claude it does not
}
#[test] fn two_pieces_same_host_rejected() { /* inline tempdir template (as src/manifest.rs tests build them): two pieces declare "AGENTS.md" → exit 3 naming both pieces */ }
#[test] fn whitespace_fragment_declares_nothing() { /* inline tempdir template: fragment renders "\n" → host absent from RenderedSections */ }
#[test] fn new_refuses_sections() {
    // splice new <channel fixture> → exit 3, message "sections are only applied by `splice channel add`", dest empty
}
#[test] fn conformance_section_fragment() { /* new corpus case: `{% if "claude" in pieces %}- Use the explorer agent.\n{% endif %}` */ }
```

- [x] **Step 2: Run — FAIL**
- [x] **Step 3: Implement.** Host keys render through `paths::render_rel_path(key, piece.target, …)` (G5); fragments render with `Renderer::render_str` in the instance context; empty-after-trim → no section. Add the corpus case to `CASES`. Note in PROGRESS that hull's Nunjucks corpus run must pick up the new case (C2).
- [x] **Step 4: Run — PASS (`cargo nextest run`)**
- [x] **Step 5: Commit** — `feat: piece [sections] and fragment rendering` (also adds `sections` to main spec §5's piece keys)

---

### Task 6: Section text operations

**Files:**
- Create: `src/sections.rs`; Modify: `src/lib.rs`
- Test: unit tests in `src/sections.rs`

**Interfaces:**
- Produces:
  - `pub fn begin_marker(channel: &str) -> String`; `pub fn end_marker(channel: &str) -> String`
  - `#[derive(Debug, PartialEq, Eq)] pub struct Block { pub begin: usize, pub end: usize /* 1-based marker lines */, pub body: String /* LF, as on disk minus CR */ }`
  - `#[derive(Debug, PartialEq, Eq)] pub enum MarkerError { Duplicate(Vec<usize>), Unbalanced(usize) }` with `fn message(&self, host: &str, channel: &str) -> String`
  - `pub fn find_block(host: &str, channel: &str) -> Result<Option<Block>, MarkerError>` (matches begin by prefix `<!-- splice:<channel>:begin` so a reworded tail still parses; other channels' markers ignored)
  - `pub fn body_sha(body: &str) -> String` (LF-normalised, sha256 hex)
  - `pub fn insert(host: Option<&str>, channel: &str, body: &str) -> String` (absent → block only; present → append after exactly one blank line)
  - `pub fn replace(host: &str, block: &Block, body: &str) -> String`; `pub fn remove(host: &str, block: &Block) -> String` (also drops the one blank line `insert` added)

- [x] **Step 1: Write failing tests**

```rust
const B: &str = "<!-- splice:rig:begin — managed by `splice update --channel rig`; edit the source -->";
#[test] fn insert_into_absent_and_present() {
    assert_eq!(insert(None, "rig", "x\n"), format!("{B}\nx\n<!-- splice:rig:end -->\n"));
    assert_eq!(insert(Some("# T\n"), "rig", "x\n"), format!("# T\n\n{B}\nx\n<!-- splice:rig:end -->\n"));
    assert_eq!(insert(Some("# T"), "rig", "x\n"), format!("# T\n\n{B}\nx\n<!-- splice:rig:end -->\n"));
}
#[test] fn find_replace_keeps_outside_bytes() {
    let host = format!("a\r\n{B}\r\nold\r\n<!-- splice:rig:end -->\r\nz\r\n");
    let b = find_block(&host, "rig").unwrap().unwrap();
    assert_eq!((b.begin, b.end, b.body.as_str()), (2, 4, "old\n"));
    assert_eq!(replace(&host, &b, "new\n"), format!("a\r\n{B}\r\nnew\r\n<!-- splice:rig:end -->\r\nz\r\n"));
}
#[test] fn other_channels_ignored() { /* host with decisions block only → find_block(.., "rig") == None */ }
#[test] fn duplicate_and_unbalanced() {
    assert_eq!(find_block(&format!("{B}\n<!-- splice:rig:end -->\n{B}\n<!-- splice:rig:end -->\n"), "rig"), Err(MarkerError::Duplicate(vec![1, 3])));
    assert_eq!(find_block(&format!("x\n{B}\nbody\n"), "rig"), Err(MarkerError::Unbalanced(2)));
    assert_eq!(find_block("<!-- splice:rig:end -->\n", "rig"), Err(MarkerError::Unbalanced(1)));
}
#[test] fn remove_round_trips_insert() {
    let host = "# T\n"; let with = insert(Some(host), "rig", "x\n");
    let b = find_block(&with, "rig").unwrap().unwrap();
    assert_eq!(remove(&with, &b), host);
}
#[test] fn sha_ignores_crlf() { assert_eq!(body_sha("a\r\nb\r\n"), body_sha("a\nb\n")); }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: section markers and block edits`

---

### Task 7: Section planner

**Files:**
- Modify: `src/sections.rs`
- Test: unit tests (table-driven, one per channels spec §6 rule)

**Interfaces:**
- Consumes: `Disk`, `RenderedSections`, `SectionLock`, Task 6 helpers
- Produces:
  - `pub struct SectionInput<'a> { pub channel: &'a str, pub theirs: &'a RenderedSections, pub disk: &'a Disk, pub locked: &'a BTreeMap<String, SectionLock>, pub filter: &'a Filter, pub removing: bool /* channel remove */, pub force: bool }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SectionChange { pub host: String, pub action: Action, pub bytes: Option<Vec<u8>> /* new host; None with Delete = delete host */, pub note: Option<String> }` and `fn report_path(&self, channel: &str) -> String` (`HOST#CHANNEL`)
  - `#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct SectionPlan { pub changes: Vec<SectionChange>, pub drift: Vec<String> /* HOST#CHANNEL */, pub broken: Vec<String> /* messages with line numbers */, pub locked: BTreeMap<String, SectionLock> /* the new [sections] */ }`
  - `pub fn plan_sections(input: &SectionInput) -> SectionPlan` (pure)
  - `pub fn host_paths(theirs: &RenderedSections, locked: &BTreeMap<String, SectionLock>) -> BTreeSet<String>` (for the disk snapshot)

- [x] **Step 1: Write failing tests**

```rust
#[test] fn table() {
    // (name, theirs_body, disk_host, locked_sha_of, created, force, expected_action, expected_host_after, created_after)
    let cases = [
        ("insert, host absent",        Some("x\n"), None,                       None,        false, false, Some(Create), Some(blk("x\n")),                Some(true)),
        ("insert, host present",       Some("x\n"), Some("# T\n"),              None,        false, false, Some(Create), Some(format!("# T\n\n{}", blk("x\n"))), Some(false)),
        ("adopt equal markers",        Some("x\n"), Some(&blk("x\n")),          None,        false, false, None,         None,                            Some(false)),
        ("adopt differing markers",    Some("x\n"), Some(&blk("u\n")),          None,        false, false, None /*drift*/, None,                          None),
        ("replace unedited",           Some("y\n"), Some(&blk("x\n")),          Some("x\n"), false, false, Some(Update), Some(blk("y\n")),                Some(false)),
        ("fragment unchanged",         Some("x\n"), Some(&blk("x\n")),          Some("x\n"), false, false, None,         None,                            Some(false)),
        ("unchanged, body edited",     Some("x\n"), Some(&blk("u\n")),          Some("x\n"), false, false, None,         None,                            Some(false)), // G8
        ("drift",                      Some("y\n"), Some(&blk("u\n")),          Some("x\n"), false, false, None /*drift*/, None,                          None),
        ("drift --force",              Some("y\n"), Some(&blk("u\n")),          Some("x\n"), false, true,  Some(Update), Some(blk("y\n")),                Some(false)),
        ("markers deleted",            Some("y\n"), Some("# T\n"),              Some("x\n"), false, false, None /*drift*/, None,                          None),
        ("markers deleted --force",    Some("y\n"), Some("# T\n"),              Some("x\n"), false, true,  Some(Create), Some(format!("# T\n\n{}", blk("y\n"))), Some(false)),
        ("gone, unedited",             None,        Some(&format!("# T\n\n{}", blk("x\n"))), Some("x\n"), false, false, Some(Delete), Some("# T\n".into()), None),
        ("gone, edited",               None,        Some(&blk("u\n")),          Some("x\n"), false, false, Some(Skip),   None,                            None),
        ("gone, created host emptied", None,        Some(&blk("x\n")),          Some("x\n"), true,  false, Some(Delete), None /*host deleted*/, None),
    ];
    for c in cases { check_case(c); }
}
#[test] fn broken_markers_refused_even_with_force() { /* duplicate markers → plan.broken non-empty with "lines 1, 3", force true still broken */ }
#[test] fn filtered_host_is_removed() { /* filter excludes AGENTS.md → like "gone" */ }
#[test] fn removing_plans_every_locked_section_as_gone() {}
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement** per channels spec §6 and G7–G9. **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: pure section planner`

---

### Task 8: Apply with `LockOut`, section writes, deferred locks

**Files:**
- Modify: `src/apply.rs`, `src/post.rs` (`run_steps` takes the "still records" text), `src/ops/{add,update}.rs` (callers pass `LockOut::Main` and `command`; `new` does not call `apply` and is untouched)
- Test: `src/apply.rs` (unit)

**Interfaces:**
- Consumes: `ChannelLock::write_atomic`, `SectionChange`, `post::{run_steps, run_post}`
- Produces:
  - `pub enum LockOut { Main(Lock), Channel(ChannelLock) }` with `fn write_atomic(&self, repo: &Path) -> Result<()>`, `fn path(&self) -> String`, `fn version(&self) -> &Version`, `fn still_records(&self, from: &Version) -> String` (`the lock still records 1.0.0` for main — M2 text unchanged; `the lock still records 1.0.0 (.splice/channels/rig.toml)` for a channel)
  - `ApplyCtx { …, pub command: &'static str, pub lock: LockOut, pub sections: &'a [SectionChange] }` — `command` replaces the `When`-based report command picked since `7288d7f` (`add` → `"add"`, `update` → `"update"`, same values for the main callers).
  - `pub fn apply_files(ctx: &ApplyCtx) -> Result<Report, SpliceError>`: drift refusal, `check_target` for every path **and** `ctx.lock.path()`, file writes → section host writes/deletes → file removals → steps. No post, no lock. Errors carry `still_records` + the restore hint exactly as `apply` does today.
  - `pub fn apply(ctx: ApplyCtx) -> Result<Report, SpliceError>` = `apply_files` → post (unless conflicts or `no_post`) → `ctx.lock.write_atomic` → conflict result. Order and texts for the main lock identical to M2.
  - `pub fn write_locks(repo: &Path, locks: &[&LockOut]) -> Result<(), SpliceError>` — writes each atomically in the given order; a failure at lock j → `Env` naming the locks already written and the ones not written, plus `git checkout -- . && git clean -fd` (used by `update --all`, Task 14).
  - `post::run_steps(repo, steps, from, to, still: &str, json)`: the restore text is `still` + the restore hint (the main caller passes `LockOut::still_records`, so M2 messages are unchanged).
  - Section changes are pushed to the report as `HOST#CHANNEL`/owned.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn section_written_before_removals_and_lock() {
    // plan deletes old.txt; one SectionChange creates AGENTS.md; step checks
    // `test -f AGENTS.md && test ! -f old.txt && test ! -f .splice/channels/rig.toml`
    let r = apply(ctx_channel("rig", plan_deleting("old.txt"), &[sec_create("AGENTS.md", "x\n")], step)).unwrap();
    assert!(r.changes.iter().any(|c| c.path == "AGENTS.md#rig" && c.class == Class::Owned));
    assert!(repo.join(".splice/channels/rig.toml").is_file() && !repo.join(".splice/lock.toml").exists());
}
#[test] fn section_delete_host() { /* SectionChange{Delete, bytes None} removes AGENTS.md */ }
#[test] fn failing_step_leaves_old_channel_lock() { /* message contains "the lock still records 1.0.0 (.splice/channels/rig.toml)" */ }
#[test] fn main_messages_unchanged() { /* guard: the three existing `contains("the lock still records 1.0.0")` asserts pass untouched */ }
#[test] fn apply_files_writes_no_lock_and_runs_no_post() { /* post `touch p`; apply_files → files written, no p, no lock file */ }
#[test] fn write_locks_in_order() { /* [Main, Channel rig] → both files exist, bytes equal each to_toml() */ }
#[test] fn write_locks_failure_names_unwritten() { /* `.splice/channels` is a regular file → Err exit 3, message names .splice/lock.toml as written and .splice/channels/rig.toml as not written */ }
#[test] fn command_name_in_report() { /* command "channel add" → report.command == "channel add" */ }
```

- [x] **Step 2: Run `cargo nextest run -E 'test(apply)'` — FAIL.**  **Step 3: Implement**; existing apply and post tests keep passing with `LockOut::Main`. **Step 4: Run `cargo nextest run` — PASS (all of `cli_update`, `cli_add`, `golden_update` unchanged).**
- [x] **Step 5: Commit** — `feat: apply channel locks, section writes and deferred locks`

(The CLI guard `main_lock_identical_with_channels_present` moved to Task 12, where main `update` first loads channel locks; D2.)

---

### Task 9: CLI grammar for channels

**Files:**
- Modify: `src/cli.rs`, `src/lib.rs`, `src/main.rs`, `src/ops/mod.rs`; Create: `src/ops/channel.rs` (functions return `Usage("splice channel add: not implemented yet")` until Tasks 10/17)
- Test: `tests/cli_basic.rs`
- **Spec:** main spec §3 command table (channel commands, `--channel`/`--all`)

**Interfaces:**
- Produces:
  - `Command::Channel(ChannelArgs)`; `#[derive(Subcommand)] pub enum ChannelCommand { Add(ChannelAddArgs), Remove(ChannelRemoveArgs) }`; `Command::NAMES` gains `"channel"`; `Command::name()` returns `"channel add"` / `"channel remove"`; `main.rs` builds `channel add|remove` for clap-error JSON.
  - `ChannelAddArgs { source: String, as_name: Option<String> /* --as */, git_ref: Option<String> /* --ref, conflicts_with track */, track: Option<String>, pre: bool, pieces: Vec<String>, data: Vec<String>, only: Vec<String>, exclude: Vec<String>, allow_dirty: bool, force: bool, no_post: bool }`
  - `ChannelRemoveArgs { name: String, allow_dirty: bool, force: bool }`
  - `AddArgs.channel: Option<String>`; `UpdateArgs { …, channel: Option<String>, all: bool, only: Vec<String>, exclude: Vec<String> }`; `CheckArgs.channel: Option<String>`; `DiffArgs.channel: Option<String>`; conflicts per G14 (amended): `only`/`exclude` `requires = "channel"`; `to` has no requirement; `all` `conflicts_with_all = ["channel", "to", "data", "only", "exclude"]`.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn ref_and_track_conflict() { splice_cmd().args(["--json", "channel", "add", "x", "--ref", "v1", "--track", "main"]).assert().code(2); }
#[test] fn channel_and_all_conflict() { splice_cmd().args(["update", "--channel", "a", "--all"]).assert().code(2); }
#[test] fn filter_flags_need_channel() { splice_cmd().args(["update", "--only", "a/**"]).assert().code(2); }
#[test] fn all_rejects_to() { splice_cmd().args(["update", "--all", "--to", "2.0.0"]).assert().code(2); }
#[test] fn all_rejects_data_only_exclude() { for f in [["--data", "a=b"], ["--only", "a"], ["--exclude", "a"]] { splice_cmd().args(["update", "--all"]).args(f).assert().code(2); } }
#[test] fn update_to_alone_is_valid() {
    // B3: `splice -C <empty dir> --json update --to 1.0.0` is not a clap error: exit 2 with the
    // "no .splice/lock.toml" message (not "requires --channel"); cli_update's --to tests stay green
}
#[test] fn clap_error_json_names_channel_command() {
    // `splice --json channel add` (missing source) → stdout one object, command == "channel add", exit 2
}
#[test] fn help_lists_channel() { /* `splice --help` contains "channel" */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: channel command grammar`

---

### Task 10: `splice channel add`

**Files:**
- Modify: `src/ops/channel.rs`, `src/ops/update.rs` (extract shared pieces), `src/fetch.rs` (`branch_head`)
- Test: `tests/cli_channel.rs`

**Interfaces:**
- Consumes: `Fetcher::resolve`, `Template::load`, `render_tree`, `render_sections`, `Filter`, `Sources`, `check_claims`, `planner::plan` (base = empty tree, lock = `Lock` with empty `files`), `plan_sections`, `apply`
- Produces:
  - `pub fn ops::channel::add(g: &GlobalArgs, a: &ChannelAddArgs) -> Result<Report>`
  - `pub(crate) fn ops::update::instances_for(template: &Template, base: &str, pieces: &[String], data: &IndexMap<String,String>, g: &GlobalArgs) -> Result<(IndexMap<String,String>, Vec<PieceInstance>)>` (moved out of `ops::new` so `new` and `channel add` share instance/var resolution; `new` keeps its behaviour)
  - `Fetcher::branch_head(&self, src: &TemplateSource, branch: &str) -> Result<ResolvedTemplate>` (rev-parse `refs/heads/<branch>` in the mirror/local repo; version `0.0.0+g<sha7>`; unknown branch → exit 3 naming it)
  - Name: `--as`, else the template's `name`; invalid → exit 2 (`use --as NAME`); already locked → exit 2 (`use splice update --channel NAME`).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn add_into_new_rendered_repo() {
    // main fixture repo at v1 + channel v1: `channel add <src>//channel --ref v1.0.0 --piece claude --piece skills:claude --data dir=claude`
    // → exit 0; .claude/agents/{explorer,reviewer}.md, .claude/skills/{caveman,verify}/SKILL.md, AGENTS.md and CLAUDE.md with rig blocks;
    // .splice/channels/rig.toml has [channel] name="rig", track absent, [sections] AGENTS.md + CLAUDE.md, created = true;
    // .splice/lock.toml bytes unchanged; JSON command "channel add", changes include "AGENTS.md#rig"
}
#[test] fn add_into_bare_repo() { /* bare_repo(): no main lock → ok; Sources has only rig */ }
#[test] fn name_defaults_and_as() { /* default name "rig"; --as Other → exit 2; in a fresh repo (no rig), --as rig2 → .splice/channels/rig2.toml; markers use rig2 */ }
#[test] fn name_taken() { /* second add → exit 2 "use splice update --channel rig" */ }
#[test] fn dirty_tree_refused() { /* untracked file → exit 2; --allow-dirty → 0 */ }
#[test] fn claim_conflict_writes_nothing() {
    // --piece collide → exit 2, message "path .editorconfig is owned by main; exclude it with --exclude .editorconfig …";
    // `git status --porcelain` empty. Same with `--exclude .editorconfig` → exit 0, .editorconfig untouched, not in rig.toml
}
#[test] fn host_owned_by_main_refused() { /* main repo from template/v1-claude (owns CLAUDE.md), --piece claude → exit 2 naming main; git status empty */ }
#[test] fn filters_stored_and_applied() {
    // --exclude '.claude/skills/caveman/**' → caveman absent, not in [files]; rig.toml exclude = [that glob];
    // --only 'nope/**' → message "filter `nope/**` matches nothing", exit 0
}
#[test] fn identical_owned_file_adopts() { /* pre-commit docs/agents/rules.md equal to the render → exit 0, no --force, file listed in [files] */ }
#[test] fn differing_owned_file_is_drift() { /* pre-commit a different docs/agents/rules.md → exit 1, nothing written; --force → replaced */ }
#[test] fn host_without_markers_gets_block_appended() { /* AGENTS.md "# Mine\n" committed → "# Mine\n\n<block>"; created = false */ }
#[test] fn track_pins_head() {
    // --track main → rig.toml track = "main", template.version = "0.0.0+g<sha7 of main>", commit = main head
}
#[test] fn post_runs_for_add() { /* --piece hooks → .rig-post.log has "rig-post"; --no-post → absent */ }
#[test] fn second_channel_disjoint_paths() { /* chan_commit rig, then channel add channel-b → exit 0; both locks present; main lock unchanged */ }
```

Multi-command tests use `chan_commit` between mutating calls (D3).

- [x] **Step 2: Run `cargo nextest run --test cli_channel` — FAIL**
- [x] **Step 3: Implement:** dirty check → `Sources::load` → resolve (`--ref`/`--track`/latest, `--pre`) → load, `check_min_splice` → name → instances (base + `--piece`) → `render_tree` + `render_sections` → filter → claims (`replace_claim` with `Claim::from_plan`) → drop seeds claimed elsewhere (G10) → disk snapshot (tree + hosts) → `plan` + `plan_sections` → drift/broken → refuse (exit 1, nothing written) → `apply` with `LockOut::Channel`, `When::Add`, `command = "channel add"`. Conflicts per G11.
- [x] **Step 4: Run — PASS**
- [x] **Step 5: Commit** — `feat: splice channel add`

---

### Task 11: `splice add --channel`

**Files:**
- Modify: `src/ops/add.rs`
- Test: `tests/cli_channel.rs`, `tests/cli_add.rs`

**Interfaces:**
- Consumes: `Sources::channel`, `Filter::from_meta`, `plan_sections`, claim check
- Produces: `ops::add::run` handles `a.channel`: base = channel lock state, theirs = with the new instance, both at the channel's commit, through the stored filter; writes only that channel's lock; C3 refusal unchanged (G11). Main `add` (no `--channel`) loads `Sources` and runs `check_claims` when channel locks exist (B4); without channels it is byte-identical to M2.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn second_harness_instance() {
    // after channel add (--piece skills:claude --data dir=claude):
    // `add skills --channel rig --name agents --data dir=agents` → .agents/skills/{caveman,verify}/SKILL.md, same bytes as .claude/skills/…;
    // rig.toml [[pieces]] has skills/claude and skills/agents; main lock bytes unchanged
}
#[test] fn add_channel_unknown() { /* --channel zz → exit 2 listing "rig" */ }
#[test] fn add_channel_respects_filter() { /* stored exclude '.agents/skills/caveman/**' → not written */ }
#[test] fn add_channel_claim_conflict() { /* add collide --channel rig → exit 2, nothing written */ }
#[test] fn main_add_claim_conflict_with_channel() { /* tests/cli_add.rs: rig owns .claude/agents/reviewer.md; a main piece rendering that path → `add` exit 2 naming channel rig, nothing written */ }
#[test] fn add_channel_updates_section_with_new_piece() { /* add claude later → AGENTS.md block gains the claude-only line (update action on AGENTS.md#rig) */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: add pieces to a channel`

---

### Task 12: `splice update --channel`

**Files:**
- Modify: `src/ops/update.rs`
- Test: `tests/cli_channel_update.rs`, `tests/cli_update.rs` (guard)
- **Spec:** main spec §8.3 note (claims across sources; plain `update` in a channel-only repo)

**Interfaces:**
- Produces:
  - `pub fn prepare_channel(g: &GlobalArgs, cl: &ChannelLock, others: &[Claim], to: Option<&Version>, filter_new: Option<(&[String], &[String])>, force: bool) -> Result<Prepared>`
  - `Prepared { …, pub channel: Option<ChannelPrepared> }` with `pub struct ChannelPrepared { pub lock_new: ChannelLock, pub sections: SectionPlan, pub claim: Claim }` (`None` for main; existing fields unchanged)
  - `prepare` (main) keeps its signature; the shared body becomes `fn prepare_lock(g, lock: &Lock, target: Target, data, force) -> Result<(Prepared, Rendered)>` with `pub enum Target<'a> { Latest, Version(&'a Version), Branch(&'a str) }` and `struct Rendered { base: RenderedTree, theirs: RenderedTree, theirs_sections: RenderedSections }` (the section planner compares with the lock sha, so no base sections, A9). Start from `prepare` at HEAD (`latest: Option<ResolvedTemplate>`, prerelease-only = up to date; C8).
  - Base filtered with the stored filter, theirs with `filter_new` (or the stored one); a new filter is stored in `lock_new.meta`.
  - Channel-only repo + plain `update` → exit 2 `no .splice/lock.toml; use --channel NAME or --all`.
  - Main `update` loads `Sources` and runs `check_claims` with its planned claim when channel locks exist (B4): a main target rendering a channel-owned path → exit 2, nothing written.
  - Filter flags (G16): any of `--only`/`--exclude` replaces both stored lists.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn update_channel_v1_to_v2() {
    // channel at v1 (claude, skills:claude, hooks); update --channel rig → exit 0: reviewer.md replaced, explorer.md deleted
    // ([[remove]]), verify/SKILL.md replaced, AGENTS.md#rig update, docs/agents/notes.md (seed) untouched,
    // rig.toml version 2.0.0; main lock bytes unchanged; .rig-post.log has two lines (add + update, A7)
}
#[test] fn section_drift_refused_then_forced() {
    // edit inside the AGENTS.md block + outside it, commit; update → exit 1, `git status` empty, message names AGENTS.md#rig and --force;
    // --force → block replaced, outside edit kept byte-identical
}
#[test] fn crlf_host_stays_crlf() { /* AGENTS.md converted to CRLF and committed → after update every line ends \r\n */ }
#[test] fn deleted_markers_are_drift() { /* remove the block lines → exit 1; --force re-appends */ }
#[test] fn broken_markers_exit_1_even_forced() { /* duplicate begin line → exit 1 with "lines" in message, also with --force */ }
#[test] fn filter_change_on_update() {
    // update --channel rig --exclude '.claude/skills/caveman/**' → caveman deleted (unedited), rig.toml exclude = [that glob], only = [];
    // variant: caveman edited first → kept, message, not in [files];
    // then `update --channel rig --only '**'` → rig.toml only = ["**"], exclude = [] (both lists replaced, G16), caveman re-created
}
#[test] fn owned_drift_in_channel_refused() { /* edit reviewer.md → exit 1, nothing written */ }
#[test] fn plain_update_untouched_by_channels() { /* main + channel; `update` → only main moves; rig.toml unchanged */ }
#[test] fn channel_only_repo_plain_update() { /* bare repo + channel; `update` → exit 2 "use --channel NAME or --all" */ }
#[test] fn claim_conflict_on_update() { /* channel_repo_tagged("channel", [("v1","v1.0.0"),("v2-collide","v2.0.0")]) → update --channel rig exit 2, nothing written */ }
#[test] fn main_update_claim_conflict_with_channel() { /* main at v1, rig at channel/v2-x (owns x.txt); main `update --to 2.0.0` against template/v2-x → exit 2 naming channel rig, nothing written */ }
#[test] fn main_lock_identical_with_channels_present() {
    // guard (tests/cli_update.rs): two repos from the same main fixture at v1; in one, copy tests/fixtures/lock/channel-rig.toml
    // to .splice/channels/rig.toml and commit; `update --yes` both to v2 → `.splice/lock.toml` bytes equal; channel lock unchanged
}
#[test] fn unknown_channel() { /* --channel zz → exit 2 */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement** spec §8.3 steps 1–8 on the channel lock plus claims and sections (Rulings G3–G10). **Step 4: Run — PASS; all of `cli_update` unchanged and green.**
- [x] **Step 5: Commit** — `feat: splice update --channel`

---

### Task 13: `--track` updates and subtree compare

**Files:**
- Modify: `src/git.rs`, `src/fetch.rs`, `src/ops/update.rs`
- Test: `tests/git.rs` (uses `tests/common`, D1), `tests/cli_channel_update.rs`
- **Spec:** main spec §4 note (tracked versions `0.0.0+g…` are never compared; no migrations)

**Interfaces:**
- Produces:
  - `Git::tree_id(git_dir: &Path, commit: &str, subdir: Option<&str>) -> Result<String>` (`git rev-parse --verify <commit>^{tree}` or `<commit>:<subdir>`)
  - `Fetcher::subtree_id(&self, src: &TemplateSource, commit: &str) -> Result<String>` (no fetch if the commit is in the mirror)
  - `Fetcher::repo(&self, src) -> Result<(PathBuf, String)>` made `pub(crate)` (no new method; C2)
  - `prepare_channel`: tracked → `Target::Branch(track)`; `--to` with track → exit 2 (G3); subtree equal → up to date (G4).
  - `prepare_lock` with `Target::Branch`: no version comparison at all (skip the `target < from` downgrade check) and no migrations (`migrate::select` not called; steps and `[[remove]]` empty) (G3, B5).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn tree_id_of_subdir() { /* tests/git.rs on common::git: repo with a/x; tree_id(.., Some("a")) == `git rev-parse HEAD:a` */ }
#[test] fn tracked_channel_moves_to_head() {
    // channel add --track main; track_commit(inside=true); update --channel rig → exit 0, rig.toml commit = new head,
    // version 0.0.0+g<sha7>, rules.md updated
}
#[test] fn tracked_outside_change_is_up_to_date() { /* track_commit(inside=false) → exit 0, message "up to date", rig.toml bytes unchanged */ }
#[test] fn tracked_rejects_to() { /* update --channel rig --to 2.0.0 → exit 2 "re-add it with --ref" */ }
#[test] fn tracked_head_sorting_lower_still_updates() { /* commit until the head's sha7 sorts below the lock's (loop track_commit) → update exit 0, no downgrade error */ }
#[test] fn tracked_runs_no_migrations() { /* add channel/migrations/9.0.0.toml with `[[step]] run = "touch ran"` on main (inside commit) → update --channel rig exit 0, `ran` absent */ }
#[test] fn tagged_channel_same_subtree_is_up_to_date() { /* tag v3.0.0 on a commit changing only README.md → update exit 0, lock unchanged */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: tracked channels and subtree compare`

---

### Task 14: `splice update --all`

**Files:**
- Modify: `src/ops/update.rs`
- Test: `tests/cli_channel_update.rs`
- **Spec:** main spec §8.3 (`--all` order, G12); the channels spec §8 already has the rewritten order

**Interfaces:**
- Consumes: `prepare_lock`/`prepare_channel` (Tasks 12–13), `check_claims`, `apply::{apply_files, write_locks}`, `post::run_post`
- Produces: `fn run_all(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report>` (called by `run` when `a.all`):
  1. dirty check; `Sources::load`; prepare main (latest, if present) and every channel (sorted) in memory. Section hosts: before planning a channel's sections, overlay the disk snapshot with every earlier source's planned seed **create** at that path (`disk.files.insert(path, Some(bytes))`), so the planner sees the seed and records `created = false` (G17);
  2. claims on the combined planned result (each source's planned claim) → exit 2, nothing written;
  3. any drift/broken markers in any source → nothing written, exit 1, messages prefixed `channel NAME: ` / `main: `;
  4. `apply_files` per source (main first, then channels by name): files, sections, removals, steps — **no post, no lock**. A failure in source k → exit 3: `<main|channel NAME>: <error>; no lock was written, every lock still records its old version; \`git checkout -- . && git clean -fd\` restores the previous state`;
  5. unless any source conflicted or `--no-post`: the `[[post]]` of all sources with `when ∋ update`, deduplicated by exact `run` (first occurrence, main then channels by name), run once with `run_post(.., When::Update, ..)`. A failure → exit 3, same "no lock was written" text naming the post command;
  6. `write_locks(repo, [main?, channels…])` — every lock last (G12);
  7. one report: `changes` merged and sorted, `messages` with a `source=…` line per source that changed; conflicts in any source → exit 1 (locks written, as `update`).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn all_updates_main_then_channels() { /* main v1 + rig v1 → update --all: both at v2; .rig-post.log gains one line; messages contain source=main and source=channel:rig */ }
#[test] fn one_drifted_source_writes_nothing() { /* edit reviewer.md (rig owned) → exit 1, main lock and files unchanged, message "channel rig:" */ }
#[test] fn claim_conflict_across_new_versions() { /* main template/v2-x and rig channel/v2-x both render x.txt → exit 2, nothing written */ }
#[test] fn post_dedup_and_before_locks() {
    // rig (hooks) + rigb (channel-b), both v1 → v2, chan_commit between adds; update --all →
    // `echo rig-post` (identical run in both) adds exactly one line; rigb's update-only post
    // `grep -q 'version = "1.0.0"' .splice/channels/rigb.toml && echo old-lock` succeeds (post sees the old locks);
    // afterwards rig.toml and rigb.toml record 2.0.0
}
#[test] fn failure_in_second_source_writes_no_lock() {
    // main + rig + rigb at v1; env SPLICE_FIXTURE_FAIL=1 → rigb's v2 step fails → exit 3;
    // .splice/lock.toml, rig.toml, rigb.toml bytes unchanged (all still 1.0.0);
    // message names "channel rigb", "no lock was written" and `git checkout -- . && git clean -fd`;
    // run the restore command, rerun without the env → exit 0, every lock at 2.0.0
}
#[test] fn failed_post_writes_no_lock() { /* replace .rig-post.log by a directory (`>>` then fails, also as root), commit → update --all exit 3, every lock unchanged, message names `echo rig-post >> .rig-post.log` and "no lock was written" */ }
#[test] fn seed_then_section_same_host() { /* main v1 + rigb v1; --all: main v2 creates seed CHANGELOG.md, rigb v2 adds its CHANGELOG.md section → host = main's seed bytes, a blank line, the rigb block; rigb.toml [sections] CHANGELOG.md created = false (G17) */ }
#[test] fn all_in_channel_only_repo() { /* bare + rig → update --all updates rig only, no lock.toml created */ }
#[test] fn all_up_to_date() { /* everything current → exit 0, message per source "up to date" */ }
```

- [x] **Step 2: Run `cargo nextest run --test cli_channel_update -E 'test(all)'` — FAIL.**  **Step 3: Implement.** **Step 4: Run `cargo nextest run` — PASS.**
- [x] **Step 5: Commit** — `feat: splice update --all`

---

### Task 15: `splice check` across sources

**Files:**
- Modify: `src/ops/check.rs`
- Test: `tests/cli_check.rs`
- **Spec:** main spec §8.7 (sources and sections in `check`)

**Interfaces:**
- Produces: `ops::check::run`: without `--channel` → main (if present) + every channel; `--channel N` → that channel only. Per channel: owned drift, markers, section drift/broken markers (`HOST#CHANNEL`, `update`, `owned`), update available (newer stable tag or tracked head, only when `subtree_id` differs). Messages `channel NAME: update available A → B (BRANCH|tag)` with short shas for tracked. Main messages unchanged from M2 (no prefix) so existing tests and the bot keep parsing them.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn check_aggregates_sources() { /* main clean, rig owned drift → exit 1, change .claude/agents/reviewer.md update/owned, message names channel rig */ }
#[test] fn check_section_drift() { /* edit inside block → change {path:"AGENTS.md#rig", action:"update", class:"owned"} */ }
#[test] fn check_tracked_head_moved() { /* track_commit(inside) → exit 1, "channel rig: update available <a7> → <b7> (main)"; --offline → exit 0 */ }
#[test] fn check_tracked_outside_change() { /* track_commit(outside) → exit 0 */ }
#[test] fn check_channel_only_repo() { /* bare + rig, clean → exit 0 (no "no lock.toml" error) */ }
#[test] fn check_single_channel() { /* --channel rig ignores main drift */ }
#[test] fn check_main_messages_unchanged() { /* guard (D2): existing update_available test passes untouched */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: splice check covers channels and sections`

---

### Task 16: `splice diff --channel` and `explain` across sources

**Files:**
- Modify: `src/ops/diff.rs`, `src/ops/explain.rs`
- Test: `tests/cli_diff.rs`, `tests/cli_explain.rs`
- **Spec:** main spec §8.9 (`explain` across sources)

**Interfaces:**
- Produces:
  - `diff --channel N [--to V]` runs `prepare_channel` and prints file patches plus section patches (host before/after, labels `a/HOST`, `b/HOST`); `--json` changes equal `update --channel N --json`'s.
  - `explain <path>` searches main then every channel: adds `source=main` / `source=channel:NAME`; a section host (in any channel's `[sections]`) adds `section=NAME` per block and, if no source owns the file, reports it `unmanaged` plus the section lines. A channel-only repo no longer exits 2 for `explain`. Start from `explain::run` at HEAD: `piece=`/`glob=` messages are emitted only with `--json` (C8); `source=`/`section=` follow the same rule, and the human line gains a `source=` column.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn diff_channel_writes_nothing() { /* rig at v1 → diff --channel rig shows "+" lines of reviewer.md and the AGENTS.md block; git status empty */ }
#[test] fn diff_channel_json_matches_update() {}
#[test] fn explain_channel_file() { /* .claude/agents/reviewer.md → messages piece=claude/claude, glob=…, merge=none, source=channel:rig */ }
#[test] fn explain_section_host() { /* AGENTS.md → messages include "section=rig"; unmanaged by files */ }
#[test] fn explain_main_file_has_source_main() { /* .editorconfig --json → "source=main" appended; other messages unchanged */ }
#[test] fn explain_human_has_source_column() { /* without --json: one line, contains "source=main", no messages */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: diff and explain for channels`

---

### Task 17: `splice channel remove`

**Files:**
- Modify: `src/ops/channel.rs`
- Test: `tests/cli_channel.rs`
- **Spec:** main spec §3 table row for `channel remove` (added in Task 9) gains the "lock last" note

**Interfaces:**
- Produces: `pub fn ops::channel::remove(g: &GlobalArgs, a: &ChannelRemoveArgs) -> Result<Report>` — plan = every locked file "B only" (base = render at the lock, theirs = empty) + `plan_sections` with `removing = true`; owned drift → exit 1 unless `--force` (then edited files are kept and reported, never deleted); apply without steps/post; delete `.splice/channels/NAME.toml` last (`fsio::remove_file`; remove `.splice/channels/` if empty).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn remove_deletes_unedited_and_strips_sections() {
    // rig added; remove rig → files gone, AGENTS.md (created) deleted, CLAUDE.md (pre-existing) back to original bytes,
    // .splice/channels/rig.toml gone; main lock bytes unchanged; command "channel remove"
}
#[test] fn remove_refuses_owned_drift() { /* edit reviewer.md → exit 1 nothing changed; --force → reviewer.md kept + message, rest removed */ }
#[test] fn remove_keeps_edited_section() { /* edit block body → --force: block kept, message "kept" */ }
#[test] fn remove_unknown() { /* exit 2 */ }
#[test] fn remove_dirty() { /* exit 2 unless --allow-dirty */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: splice channel remove`

---

### Task 18: Golden main + channel update

**Files:**
- Create: `tests/golden_channels.rs`, snapshots
- Test: itself

**Interfaces:**
- Consumes: the binary, both fixtures.

- [x] **Step 1: Write the tests**

```rust
#[test] fn golden_main_and_channel_v1_to_v2() {
    // main at v1 (apply_user_edits from golden_update, refactored into common) + rig v1 (claude, skills:claude, skills:agents, core, hooks);
    // user edits: AGENTS.md outside the block, CLAUDE.md inside the rig block (then reverted for the forced variant), seed notes.md;
    // update --all --yes --json → snapshot JSON + tree_snapshot (`golden_update::apply_user_edits` moves to tests/common in this task)
}
#[test] fn golden_forced_section_variant() { /* inside-block edit kept, update --all --force → snapshot */ }
#[test] fn contract_guard_golden() { /* the golden repo's .splice/lock.toml equals golden_update's snapshot lock text */ }
```

- [x] **Step 2: Run `cargo nextest run --test golden_channels`, read every `.snap.new` against the fixture table and Rulings, then accept with `INSTA_UPDATE=always cargo nextest run --test golden_channels` (`cargo insta` is not installed; never blind-accept)**
- [x] **Step 3: Fix any bug the snapshot exposes (test first in the owning module)**
- [x] **Step 4: Run `cargo nextest run` — PASS; fmt + clippy clean**
- [x] **Step 5: Commit** — `test: golden main plus channel update`

---

### Task 19: Spec index, README and skill file

**Files:**
- Modify: `docs/superpowers/specs/2026-10-14-splice-design.md` (§10 item 15: an index of G1–G17 with one line each, worded per G1; §15 milestone 4), `README.md` (Channels section)
- **After plan 03:** `skills/splice/SKILL.md` (plan 03 Task 5 creates it). Until then add the lines below to plan 03 Task 5's checklist and a PROGRESS Blocked line `SKILL.md channel lines: plan 03 not executed`.
- Test: plan 03's `docs_mention_only_real_flags` (if present), else a new `tests/cli_basic.rs::readme_channel_flags_exist`

**Interfaces:** none (docs). The other main-spec sections were updated by the tasks that changed them (B6); the channels spec was updated in the plan revision.

- [x] **Step 1: Write failing test** — every `--[a-z-]+` in the README Channels section appears in `splice channel add --help`, `splice update --help`, `splice check --help` or `splice add --help`.
- [x] **Step 2: Run — FAIL.**  **Step 3: Write.** SKILL.md (done in plan 03 Task 5) gains ≤ 10 lines: channels exist when `.splice/channels/` has files; `splice explain <path> --json` names the source; never edit inside `<!-- splice:NAME:begin …` blocks, change the channel source instead; `splice check --json` covers every channel. **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `docs: channels in spec index and README`

---

### Task 20: Update-bot `scope` input

Built on plan 03 Task 4 (`action/action.yml`, `action/summary.mjs`); landed as plan 03 Task 4b.

**Files:**
- Modify: `action/action.yml`, `action/summary.mjs`, `action/test/*.test.mjs`, README bot section
- Test: `node --test action/test`

**Interfaces:**
- Produces: input `scope` (`main` | `all`; default: `all` when `.splice/channels/` has a `*.toml`, else `main`). `all` runs `splice update --all --json --yes`; `main` runs today's command. One PR either way; the PR body groups rows by the `source=` messages. `scope: main` in a channel-only repo fails with splice's exit-2 message.

- [x] **Step 1: Write failing tests** — `scopeDefault({channels: ["rig.toml"]}) === "all"`, `scopeDefault({channels: []}) === "main"`, `command("all")` equals `["update","--all","--json","--yes"]`, `summary()` groups `source=channel:rig` rows under a `rig` heading.
- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS; actionlint clean.**
- [x] **Step 5: Commit** — `feat: update bot scope input for channels`

---

### Task 21: Version 0.2.0 and milestone wrap-up

**Files:**
- Modify: `Cargo.toml` (`version = "0.2.0"`), `Cargo.lock`, `docs/PROGRESS.md` (Status, Blocked). No insta snapshot holds the version (all redact `splice`), so none should change (C6).
- **After plan 03:** the npm launcher and platform package versions (`npm/**/package.json`) move to `0.2.0` in the same kind of commit, and the node tests run.
- Test: full suite

- [x] **Step 1:** Bump the version; run `cargo nextest run`; any `.snap.new` is a bug (snapshots are redacted) — investigate, do not accept.
- [x] **Step 2:** `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo nextest run` — paste the summary into PROGRESS. Run `node --test` only on the paths that exist (`for p in npm/splice/test action/test scripts; do [ -e "$p" ] && …; done`); with none present record `node tests: n/a (plan 03 not landed)`. Never run `node --test` on missing paths (it exits 1).
- [x] **Step 3:** Add the Blocked items below to `docs/PROGRESS.md`.
- [x] **Step 4: Commit** — `chore: splice 0.2.0`

---

## Cross-repo tasks (each in its own repo and PR; not in this repo)

### Task C1 (repo `keelstackhq/decisions`): `channel/` template and D-004

**Files (in keelstackhq/decisions):** `channel/splice.toml` (`name = "decisions"`, `base = "snapshot"`, `min_splice = "0.2.0"`), `channel/pieces/snapshot/{piece.toml,files/docs/agents/org-decisions.md}` (owned), `channel/pieces/import-project/` (`[sections] "docs/agents/project.md"`), `channel/pieces/import-claude/` (`"CLAUDE.md"`), `channel/pieces/import-agents/` (`"AGENTS.md"`: "Read docs/agents/org-decisions.md"), `scripts/check-index.sh` (fails unless `channel/pieces/snapshot/files/docs/agents/org-decisions.md` equals `DECISIONS.md`), `adopt/adopt.sh` (prints `deprecated: run splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project`), `adopt/README.md`, `decisions/004-decisions-channel.md` (copy of `decisions/000-template.md`) and a D-004 row in `DECISIONS.md` (E4).

- [ ] Step 1: Make `check-index.sh` fail on a deliberately different copy; then copy `DECISIONS.md`; `bash scripts/check-index.sh` passes.
- [ ] Step 2: D-004 — row `| D-004 | <date> | all | Org decisions reach repos as the splice channel gh:keelstackhq/decisions//channel (--track main; --piece import-project, else import-claude), kept fresh by the update bot and splice check; adopt/adopt.sh and the SessionStart sync-decisions.sh hooks are retired. [why](decisions/004-decisions-channel.md) |`; the record names the repos (hull, splice, hoard, example-app, keelstack, berth), the files deleted per repo and the command. `DECISIONS.md` ≤ 60 lines; `npx prettier --write DECISIONS.md`; `bash scripts/check-index.sh` passes (the snapshot copy gets the row too).
- [ ] Step 3: With a local splice ≥ 0.2.0 build (`cargo build --release` in splice; npm is not needed for this dry run): `splice channel add <decisions checkout>//channel --track <branch> --piece import-project` into a scratch copy of hull — the existing snapshot adopts without `--force`; paste output.
- [ ] Step 4: PR `feat: decisions channel for splice` (owner merges, D-002). The SessionStart `sync-decisions.sh` stays until C3 replaces it per repo.

### Task C2 (repo `keelstackhq/hull`): `rigging/` channel and `AGENTS.md` → seed

Inside the contract (no `docs/template-format.md` change); hull's own plan/PR.

- [ ] First: a hull spec change (AI layer: `AGENTS.md` becomes seed; `rigging/` is a second template) signed off by the hull owner; record it in hull's `docs/PROGRESS.md` Blocked until then. Ask the owner whether hull publishing `[sections]` (a key outside contract §4) needs a contract note under D-003 before `rigging/` ships (E3).
- [ ] `rigging/` (format 1, `name = "rigging"`, `base = "core"`, `min_splice = "0.2.0"`): `core` (`[sections] "AGENTS.md"` with the harness-neutral rules), `claude` (`.claude/agents/{explorer,implementer,reviewer,ci-fixer}.md` owned; **no** `CLAUDE.md` section — hull owns `CLAUDE.md`, which imports `@AGENTS.md`, E1), `skills` (`multiple`, `target = ".{{ dir }}/skills"`, `dir` pattern `^(claude|agents)$`). Harness dirs checked against current Claude Code and Codex docs at write time.
- [ ] Reclassify `AGENTS.md` from owned to seed in hull's base piece; hull test for the class.
- [ ] Add splice's `section-fragment` corpus case to hull's Nunjucks conformance run.
- [ ] dogfood renders `rigging/` with splice once 0.2.0 is published (**after plan 03**; Blocked until then).
- [ ] A later hull release: `migrations/<v>.toml` `[[remove]]` for the four agent files; README + generated `CLAUDE.md` say: run `splice update`, then `splice channel add gh:keelstackhq/hull//rigging --piece claude` (not a post/step: the main lock is not yet written then).

### Task C3 (repos hull, splice, hoard, example-app, keelstack, berth): adopt the decisions channel — **after plan 03**

One PR per repo, after plan 03 is executed and released (splice 0.2.0 on npm via its release workflow, bot Action available) and C1 is merged (E5). hoist has not adopted the snapshot; leave it out unless the owner asks (E6).

- [ ] Delete `.claude/hooks/session-start.d/sync-decisions.sh` (or `.claude/hooks/sync-decisions.sh` and its `settings.json` entry — berth uses this form); delete the hand-written "## Org decisions" block.
- [ ] `pnpm dlx @keelstackhq/splice@0.2.0 channel add gh:keelstackhq/decisions//channel --track main --piece import-project` (repos with `docs/agents/project.md`: hull-generated example-app, keelstack); else `--piece import-claude` (hull, splice, hoard, berth). In hull-generated repos `CLAUDE.md` is owned by hull, so use `import-project`.
- [ ] Enable the update bot (plan 03 Action, Task 20 `scope` default `all`); `splice check --json` exit 0; CI green. Commit `chore: adopt decisions channel`.
- [ ] After all six: delete `adopt/adopt.sh` in keelstackhq/decisions (PR).

## Blocked (human-only; copy into `docs/PROGRESS.md`)

- **Plan 03:** not executed at 999179b. Task 19's SKILL.md part, Task 20, Task 21's npm part, C2's dogfood step and C3 wait for it.
- **Release 0.2.0 (npm):** after plan 03's release workflow exists: the `NPM_TOKEN` repo secret, an owner of the npm scope `keelstackhq`, then push tag `v0.2.0` on keelstackhq/splice after Task 21 is on `main`.
- **Decisions PR merge:** the owner merges C1 (incl. D-004) in keelstackhq/decisions (D-002).
- **hull sign-off:** the hull owner approves the hull spec change (AGENTS.md → seed, `rigging/`) and answers whether `[sections]` in a hull-published template needs a contract note (D-003).
- **Bot tokens:** each adopting repo needs the bot workflow enabled and a token that can read `keelstackhq/decisions` and `keelstackhq/hull` if they are private (repo secret, e.g. a GitHub App token passed as `token`).
- **hull release** removing the four agent files (C2 last step): tag a hull version.

## Self-review notes

- Order: lock + reserved paths (T1), filters (T2), registry + claims (T3), fixtures + helpers (T4, before any test that loads them), sections (T5–T7) before `channel add` (T10) so `add` writes blocks from the start; `apply` split (T8) before `--all` (T14); `--channel`/`--all` (T11–T16) with `--track` (T13); docs (T19), bot (T20, after plan 03), release prep (T21), cross-repo (C1–C3).
- Spec §11 coverage: fixtures (T4), every CLI case (T10–T17), units (T1, T2, T6, T7), golden (T18), conformance (T5), contract guards (T1 snapshot, T12 CLI, T18 golden).
- No contract change anywhere; G1 is recorded in splice's spec only. The channels spec §8 order (B2) matches G12 and the lock-last rule.
- Open proposals (spec silent, controller to confirm): `write_locks` is per-file atomic, not atomic across files — a crash between two lock writes leaves the earlier locks new and the later ones old, and the message names both groups; dedup keeps the first occurrence of a `run` string (main, then channels by name).
