# splice Milestone 2 — `update`, `check`, `diff` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep a generated repo current: 3-way `update` with per-class rules and json/yaml/toml/text merges, migrations, post commands and an atomic lock; `check` for drift, markers and new versions; `diff` as a dry run; `add` moved onto the full planner.

**Architecture:** `update` renders base (lock commit) and theirs (target) into memory, a pure `planner` turns base/theirs/disk/lock into a list of `Planned` changes, and `apply` executes it in a fixed order (files → removals → migration steps → lock → post). Merges live in `src/merge/`, one module per strategy behind `merge3`. `diff` and `check` reuse the planner without writing.

**Tech Stack:** as plan 01, plus diffy 0.5 (text 3-way, `ConflictStyle::Diff3`, `create_patch`), yaml_serde 0.10 (maintained fork of the archived serde_yaml, by the YAML organisation; `Mapping` keeps insertion order), toml_edit 0.25 (format-preserving edits).

**Spec:** `docs/superpowers/specs/2026-10-14-splice-design.md` (§8.3–8.8, §10) · **Contract:** `docs/template-format.md` (§5, §6, §7)

## Global Constraints

- Everything in plan 01's Global Constraints.
- Diff3 markers exactly `<<<<<<< ours`, `||||||| base`, `=======`, `>>>>>>> template` (diffy's `||||||| original`/`>>>>>>> theirs` relabelled inside conflict hunks only).
- Planner is pure (no I/O); disk reads go through a `Disk` snapshot built before planning.
- Owned drift without `--force` → nothing written, exit 1.
- Apply order: write files → removals → migration `[[step]]`s (A < v ≤ B, semver order) → lock (atomic) → `[[post]]`. A failed step exits 3 before the lock; a failed post exits 3 after it.
- Dirty git tree or no work tree → exit 2 unless `--allow-dirty`. Messages after a partial write say `git checkout -- . && git clean -fd` restores the previous state.
- Structured merge rules exactly contract §5; any structured conflict ends as diff3 markers in the file (spec §8.5).
- Downgrade (`--to` < lock version) → exit 2. Target `min_splice` > running → exit 3 before any write.

## Review Focus

1. **A user on Windows whose shared text file has CRLF endings** → merge works line by line and the result keeps CRLF; no whole-file conflict. Test in Task 2.
2. **A shared file that contains a literal line `||||||| original` outside any conflict** → left untouched by marker relabelling. Test in Task 2.
3. **The update crashes or a migration step fails after files were written** → the lock still describes the old version, the message names the restore command, and a rerun after `git checkout -- . && git clean -fd` succeeds. Test in Task 7.
4. **A JSON `package.json` where the user added a dependency and the template bumped another** → both survive, user key order kept, the template's new key appended, file indentation (2 spaces or tabs) kept. Test in Task 3.
5. **`splice check` right after a conflicted update, then after the user resolves markers but leaves a stray `=======` line in a Markdown file** → exit 1 with markers; after resolution exit 0 (a lone `=======` outside a `<<<<<<< ours` block is not a marker). Test in Task 8.

---

## File Structure

```
src/merge/mod.rs      merge3(), Merged, fast paths, structured→text fallback, whole-file wrap
src/merge/text.rs     diffy merge + marker relabel + CRLF handling
src/merge/keypath.rs  generic 3-way over a tree of maps (shared by json/yaml)
src/merge/json.rs     serde_json Value + indent detection
src/merge/yaml.rs     yaml_serde Value
src/merge/toml.rs     toml_edit DocumentMut in-place edits
src/migrate.rs        Migration { steps, removes }, load_migrations, select(A, B)
src/planner.rs        Disk, Planned, plan(), PlanInput
src/apply.rs          apply(plan, …) in the fixed order
src/markers.rs        find_markers(text) -> Vec<line>
src/ops/{update,check,diff}.rs ; Modify: src/ops/add.rs
tests/fixtures/template/v2/**  tests/fixtures/template/v3/**
tests/cli_update.rs  tests/cli_check.rs  tests/cli_diff.rs  tests/golden_update.rs
```

Fixture v2 changes against v1 (each line is a test target):

| File | v2 change | User edit in golden test | Expected |
|---|---|---|---|
| `.editorconfig` (owned) | `indent_size = 4` | none | replaced |
| `tool.config.mts` → `tool.config.ts` (owned) | renamed; `migrations/2.0.0.toml` `[[remove]] path = "tool.config.mts"` | none | old deleted, new created |
| `package.json` (shared, json) | bumps `devDependencies.typescript`, adds `scripts.lint` | adds `dependencies.left-pad`, reorders nothing | merge: all three |
| `config/settings.toml` (shared, toml) | changes `[build] target` | adds a comment and `[local] x = 1` | merge, comment kept |
| `.github/workflows/ci.yml` (shared, yaml) | changes `jobs.test.runs-on` | changes the same key | conflict (markers) |
| `notes.txt` (shared, text) | edits line 2 | edits line 5 | merge |
| `README.md` (seed) | rewritten | edited | untouched |
| `apps/web/tsconfig.json` (owned) | `strict: true` | edited (golden "force" variant) | refused exit 1; replaced with `--force` |
| `CHANGELOG.md` (new seed) | added | absent | created |
| `old.txt` (owned) | dropped without `[[remove]]` | none | deleted |
| `migrations/2.0.0.toml` | `[[step]] run = "test -z \"$SPLICE_FIXTURE_FAIL\" && echo migrated > .splice/migrated.txt"` | — | step ran between files and lock |
| template vars | adds `license` (default `MIT`) | — | lock gains `license` with `--yes` |

v3 = v2 with `min_splice = "99.0.0"`.

---

### Task 1: Fixture v2/v3 and migrations loader

**Files:**
- Create: `tests/fixtures/template/v2/**`, `tests/fixtures/template/v3/**` (v3 may already exist from plan 01 Task 8 — make it v2-based), `src/migrate.rs`; Modify: `src/manifest.rs` (`Template.migrations`)
- Test: `src/migrate.rs` (unit), `tests/manifest.rs`

**Interfaces:**
- Produces:
  - `struct Step { run: String, description: Option<String> }`; `struct Remove { path: String, piece: Option<String> }`; `struct Migration { version: semver::Version, steps: Vec<Step>, removes: Vec<Remove> }`
  - `fn load_migrations(root: &Path) -> Result<Vec<Migration>>` (sorted; bad file name → exit 3)
  - `fn select(all: &[Migration], from: &Version, to: &Version) -> Vec<&Migration>` (from < v ≤ to)
  - `Template { …, migrations: Vec<Migration> }`

- [x] **Step 1: Write failing tests**

```rust
#[test] fn selects_range_in_semver_order() {
    let all = migs(&["1.2.0", "1.10.0", "2.0.0", "1.9.0"]);
    let v = |s| Version::parse(s).unwrap();
    assert_eq!(names(select(&all, &v("1.2.0"), &v("2.0.0"))), ["1.9.0", "1.10.0", "2.0.0"]);
    assert!(select(&all, &v("2.0.0"), &v("2.0.0")).is_empty());
}
#[test] fn bad_migration_name() { /* migrations/latest.toml → exit 3 naming the file */ }
#[test] fn fixture_v2_loads_with_migration() { /* Template::load(v2).migrations[0].removes[0].path == "tool.config.mts" */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.**  **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: fixture v2/v3 and migration loading`

---

### Task 2: Text 3-way merge and marker detection

**Files:**
- Create: `src/merge/mod.rs` (types only), `src/merge/text.rs`, `src/markers.rs`
- Test: unit tests in both

**Interfaces:**
- Produces:
  - `struct Merged { text: String, conflicts: Vec<String> /* key paths or "line N" */ }`
  - `fn merge_text(base: &str, ours: &str, theirs: &str) -> Merged`
  - `fn find_markers(text: &str) -> Vec<usize>` (1-based lines of `<<<<<<< ours` blocks; `=======`/`||||||| base` count only inside an open block; `>>>>>>> template` closes it)
  - `fn wrap_whole(base, ours, theirs) -> String` (one diff3 block)

- [x] **Step 1: Write failing tests**

```rust
#[test] fn clean_merge() {
    assert_eq!(merge_text("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n").text, "A\nb\nC\n");
}
#[test] fn conflict_uses_contract_markers() {
    let m = merge_text("x\n", "ours\n", "theirs\n");
    assert_eq!(m.text, "<<<<<<< ours\nours\n||||||| base\nx\n=======\ntheirs\n>>>>>>> template\n");
    assert_eq!(m.conflicts.len(), 1);
}
#[test] fn crlf_ours_is_kept() {
    // adjacent-line edits conflict in diff3 (as in git), so the edits are two lines apart
    assert_eq!(merge_text("a\nb\nc\n", "a\r\nb\r\nC\r\n", "A\nb\nc\n").text, "A\r\nb\r\nC\r\n");
}
#[test] fn literal_marker_text_outside_hunks_untouched() {
    let base = "||||||| original\nk\n"; let m = merge_text(base, base, "||||||| original\nK\n");
    assert_eq!(m.text, "||||||| original\nK\n");
}
#[test] fn markers_found_only_in_blocks() {
    assert_eq!(find_markers("=======\nplain\n"), Vec::<usize>::new());
    assert_eq!(find_markers("a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> template\n"), vec![2]);
}
```

- [x] **Step 2: Run — FAIL**
- [x] **Step 3: Implement** with `diffy::MergeOptions::new().set_conflict_style(ConflictStyle::Diff3).merge(base, ours, theirs)`; relabel only diffy's own delimiter lines (diffy 0.5 has no custom labels: pick a marker length no input line uses, then rewrite those delimiters to the contract labels). Missing final newline on any side: `IncompleteHunkStyle::Git` (diffy's default glues markers onto the last line, which `find_markers` would miss).
- [x] **Step 4: Run — PASS**
- [x] **Step 5: Commit** — `feat: text 3-way merge with contract diff3 markers`

---

### Task 3: Structured merges (json, yaml, toml) and `merge3`

**Files:**
- Create: `src/merge/keypath.rs`, `src/merge/json.rs`, `src/merge/yaml.rs`, `src/merge/toml.rs`; Modify: `src/merge/mod.rs`
- Test: unit tests in each

**Interfaces:**
- Produces:
  - `enum Tree { Map(IndexMap<String, Tree>), Leaf(Leaf) }` in `keypath.rs` and `fn merge_tree(base: &Tree, ours: &Tree, theirs: &Tree, path: &mut Vec<String>, conflicts: &mut Vec<String>) -> Tree` — contract §5 rules; a conflict keeps ours' value and records `a.b.c`.
  - `fn merge_json(b, o, t: &str) -> Result<Merged, ParseError>`, `fn merge_yaml(...)`, `fn merge_toml(...)`
  - `fn merge3(strategy: MergeStrategy, base: &str, ours: &str, theirs: &str) -> Merged` — fast paths (O = B → T; T = B → O), parse error → text, structured conflicts → `merge_text`, and if that is clean → `wrap_whole`.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn json_both_sides_kept_order_and_indent() {
    let b = "{\n  \"name\": \"x\",\n  \"dependencies\": {}\n}\n";
    let o = "{\n  \"name\": \"x\",\n  \"dependencies\": { \"left-pad\": \"1\" }\n}\n";
    let t = "{\n  \"name\": \"x\",\n  \"dependencies\": {},\n  \"scripts\": { \"lint\": \"oxlint\" }\n}\n";
    let m = merge3(MergeStrategy::Json, b, o, t);
    assert!(m.conflicts.is_empty());
    assert_eq!(m.text, "{\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"left-pad\": \"1\"\n  },\n  \"scripts\": {\n    \"lint\": \"oxlint\"\n  }\n}\n");
}
#[test] fn json_tabs_kept() { /* ours indented with \t → output indented with \t */ }
#[test] fn arrays_are_values() {
    let m = merge3(MergeStrategy::Json, r#"{"a":[1]}"#, r#"{"a":[1,2]}"#, r#"{"a":[1,3]}"#);
    assert_eq!(m.conflicts, ["a"]); assert!(!find_markers(&m.text).is_empty());
}
#[test] fn delete_vs_change_conflicts() { /* base {a:1,b:1}, ours deletes b, theirs sets b:2 → conflicts ["b"] */ }
#[test] fn delete_by_template_only() { /* base {a,b}, ours {a,b} edited a, theirs drops b → {a(ours)} */ }
#[test] fn toml_keeps_comments() {
    let b = "[build]\ntarget = \"es2022\"\n";
    let o = "# mine\n[build]\ntarget = \"es2022\"\n\n[local]\nx = 1\n";
    let t = "[build]\ntarget = \"es2024\"\n";
    assert_eq!(merge3(MergeStrategy::Toml, b, o, t).text, "# mine\n[build]\ntarget = \"es2024\"\n\n[local]\nx = 1\n");
}
#[test] fn yaml_same_key_conflict_has_markers() { /* runs-on changed both sides → markers via text merge */ }
#[test] fn yaml_multi_document_falls_back_to_text() { /* "---\na: 1\n---\nb: 2\n" */ }
#[test] fn invalid_json_falls_back_to_text() { /* ours has a trailing comma → text merge result */ }
```

- [x] **Step 2: Run — FAIL**
- [x] **Step 3: Implement.** JSON/YAML convert to `Tree` (non-map values are `Leaf`s compared by value), merge, convert back; JSON printed with `serde_json::ser::PrettyFormatter::with_indent(detected)` plus a final `\n` when ours had one. TOML: compute the set of key paths changed by theirs vs base with `keypath`, apply each to ours' `DocumentMut` (set / remove / append new tables at the end); conflicts as for others.
- [x] **Step 4: Run — PASS**
- [x] **Step 5: Commit** — `feat: structured json/yaml/toml 3-way merges`

---

### Task 4: The planner

**Files:**
- Create: `src/planner.rs`
- Test: `src/planner.rs` (unit, one test per spec §8.4 table cell)

**Interfaces:**
- Consumes: `RenderedTree` (plan 01 T6), `Lock`, `Migration`, `merge3`
- Produces:
  - `struct Disk { files: BTreeMap<String, Option<Vec<u8>>> }` with `fn snapshot(repo: &Path, paths: impl Iterator<Item = &str>) -> io::Result<Disk>`
  - `struct PlanInput<'a> { base: &'a RenderedTree, theirs: &'a RenderedTree, disk: &'a Disk, lock: &'a Lock, removes: Vec<String> /* rendered [[remove]] paths */, force: bool }`
  - `struct Planned { path: String, action: Action, class: Class, bytes: Option<Vec<u8>>, note: Option<String> }` (moved here from `ops/add.rs`, plan 01 Task 9, gaining `note`)
  - `struct Plan { changes: Vec<Planned>, drift: Vec<String>, conflicts: Vec<String> }`
  - `fn plan(input: &PlanInput) -> Plan`
  - `fn render_removes(t: &Template, removes: &[&Remove], instances: &[PieceInstance], vars) -> Result<Vec<String>>`

- [x] **Step 1: Write failing tests** — table-driven, one case per cell:

```rust
#[test] fn table() {
    use Action::*; use Class::*;
    // (name, class, base, theirs, disk, lock_sha_of, force, expected_action, expected_bytes)
    let cases = [
        ("owned unedited changed",  Owned,  Some("a"), Some("b"), Some("a"), Some("a"), false, Some(Update), Some("b")),
        ("owned unchanged",         Owned,  Some("a"), Some("a"), Some("a"), Some("a"), false, None,         None),
        ("owned drift",             Owned,  Some("a"), Some("b"), Some("x"), Some("a"), false, None /* in drift */, None),
        ("owned drift --force",     Owned,  Some("a"), Some("b"), Some("x"), Some("a"), true,  Some(Update), Some("b")),
        ("owned deleted by user",   Owned,  Some("a"), Some("b"), None,      Some("a"), false, None /* in drift */, None),
        ("owned new, absent",       Owned,  None,      Some("b"), None,      None,      false, Some(Create), Some("b")),
        ("owned new, user file",    Owned,  None,      Some("b"), Some("u"), None,      false, None /* in drift */, None),
        ("shared ours==base",       Shared, Some("a\n"), Some("b\n"), Some("a\n"), Some("a\n"), false, Some(Update), Some("b\n")),
        ("shared theirs==base",     Shared, Some("a\n"), Some("a\n"), Some("u\n"), Some("a\n"), false, None,   None),
        ("shared merge",            Shared, Some("1\n2\n3\n"), Some("1\n2\nT\n"), Some("U\n2\n3\n"), Some("1\n2\n3\n"), false, Some(Merge), Some("U\n2\nT\n")),
        ("shared conflict",         Shared, Some("x\n"), Some("t\n"), Some("u\n"), Some("x\n"), false, Some(Conflict), None /* has markers */),
        ("shared deleted by user",  Shared, Some("a\n"), Some("b\n"), None,      Some("a\n"), false, Some(Skip), None),
        ("shared new, user file",   Shared, None,      Some("t\n"), Some("u\n"), None,      false, Some(Conflict), None),
        ("seed exists",             Seed,   Some("a"), Some("b"), Some("u"), None,      false, None,         None),
        ("seed new, absent",        Seed,   None,      Some("b"), None,      None,      false, Some(Create), Some("b")),
        ("owned gone, unedited",    Owned,  Some("a"), None,      Some("a"), Some("a"), false, Some(Delete), None),
        ("owned gone, edited",      Owned,  Some("a"), None,      Some("x"), Some("a"), false, Some(Skip),   None),
        ("shared gone, edited",     Shared, Some("a"), None,      Some("x"), Some("a"), false, Some(Skip),   None),
        ("seed gone",               Seed,   Some("a"), None,      Some("a"), None,      false, None,         None),
    ];
    for c in cases { check_case(c); }
}
#[test] fn remove_list_deletes_unedited_and_ignores_still_rendered() { /* [[remove]] for a path theirs still renders → warning note, no delete */ }
```

- [x] **Step 2: Run — FAIL**
- [x] **Step 3: Implement `plan`** exactly per spec §8.4; `Skip` changes carry a `note` that becomes a message. Paths in drift produce no change entry; `Plan.drift` lists them.
- [x] **Step 4: Run — PASS**
- [x] **Step 5: Commit** — `feat: pure update planner for all ownership classes`

---

### Task 5: Apply, migration steps and post

**Files:**
- Create: `src/apply.rs`; Modify: `src/post.rs` (add `run_steps`)
- Test: `src/apply.rs` (unit with temp dirs)

**Interfaces:**
- Consumes: `Plan`, `Lock::write_atomic`, `run_post`
- Produces:
  - `fn run_steps(repo: &Path, steps: &[(&Version, &Step)], from: &Version, to: &Version, json: bool) -> Result<()>` — env `SPLICE_FROM_VERSION`, `SPLICE_TO_VERSION`; failure → Env error whose message includes the step, its version and the restore command.
  - `struct ApplyCtx<'a> { repo: &'a Path, plan: &'a Plan, steps: Vec<(&'a Version, &'a Step)>, from: Version, to: Version, lock: Lock, post: Vec<PostCmd>, when: When, no_post: bool, json: bool }`
  - `fn apply(ctx: ApplyCtx) -> Result<Report>` — the fixed order from Global Constraints; returns a Report with every change; `Err(SpliceError::Conflict(report))` when `plan.conflicts` is non-empty (after the lock is written).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn order_is_files_removals_steps_lock_post() {
    // step: `test -f new.txt && test ! -f old.txt && grep -q 'version = "1.0.0"' .splice/lock.toml && echo step >> order.log`
    // post: `grep -q 'version = "2.0.0"' .splice/lock.toml && echo post >> order.log`
    apply(ctx_for(plan_creating("new.txt").deleting("old.txt"), step, post)).unwrap();
    assert_eq!(read(&repo, "order.log"), "step\npost\n");
}
#[test] fn failing_step_leaves_old_lock() {
    let r = apply(ctx_with_step("exit 7"));
    let e = r.unwrap_err(); assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("git checkout -- . && git clean -fd"));
    assert_eq!(read_lock(&repo).template.version, Version::parse("1.0.0").unwrap());
}
#[test] fn failing_post_keeps_new_lock() { /* post "exit 1" → exit 3, lock version is the target */ }
#[test] fn conflicts_still_write_lock_and_exit_1() { /* plan with one conflict → Err(Conflict), lock = target, file has markers */ }
```

- [x] **Step 2: Run — FAIL.**  **Step 3: Implement.**  **Step 4: Run — PASS.**
- [x] **Step 5: Commit** — `feat: apply plans with migrations, atomic lock and post commands`

---

### Task 6: `splice update`

**Files:**
- Create: `src/ops/update.rs`
- Test: `tests/cli_update.rs`

**Interfaces:**
- Consumes: Tasks 1–5, `Fetcher`, `render_tree`, `resolve_vars`, `Git::is_clean`
- Produces:
  - `fn ops::update::prepare(g: &GlobalArgs, to: Option<&Version>, data, force: bool) -> Result<Prepared>` — steps 1–6 of spec §8.3 except the dirty check; `struct Prepared { plan: Plan, lock_new: Lock, from: Version, to: Version, template: Template, instances: Vec<PieceInstance>, up_to_date: bool }` (reused by `diff` and `check`).
  - `fn ops::update::run(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn dirty_tree_refused() { /* new at v1, commit, touch x → update exit 2; --allow-dirty → proceeds */ }
#[test] fn up_to_date_is_noop() { /* repo at v2 → exit 0, message "up to date", lock bytes unchanged */ }
#[test] fn downgrade_refused() { /* repo at v2, --to 1.0.0 → exit 2 */ }
#[test] fn min_splice_refused_before_write() { /* --to 3.0.0 → exit 3, `git status --porcelain` empty */ }
#[test] fn owned_drift_refused_then_forced() {
    // edit apps/web/tsconfig.json, commit; update → exit 1, JSON changes empty, messages name the file, tree unchanged
    // update --force → exit 0, file replaced, change {action:"update", class:"owned"}
}
#[test] fn new_var_takes_default_with_yes() { /* lock vars gain license = "MIT" */ }
#[test] fn removed_piece_is_template_error() { /* v2 variant without `lib` while repo has lib → exit 3, nothing written */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement** spec §8.3. **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: splice update`

---

### Task 7: Golden v1 → v2 test and crash recovery

**Files:**
- Create: `tests/golden_update.rs`, snapshots under `tests/snapshots/`
- Test: itself

**Interfaces:**
- Consumes: the binary; fixture table above.

- [ ] **Step 1: Write the tests**

```rust
#[test] fn golden_v1_to_v2_with_user_edits() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "1.0.0", &["--piece", "app:web", "--data", "project=demo"]);   // new --ref v1.0.0
    apply_user_edits(&repo);   // the "User edit" column of the fixture table, then git commit
    let out = splice_cmd().current_dir(&repo).args(["update", "--yes", "--json"]).assert().code(1).get_output().stdout.clone();
    insta::assert_json_snapshot!(serde_json::from_slice::<serde_json::Value>(&out).unwrap());
    insta::assert_snapshot!(tree_snapshot(repo.path()));
    assert!(repo.path().join(".splice/migrated.txt").exists());
    assert!(!repo.path().join("tool.config.mts").exists() && repo.path().join("tool.config.ts").exists());
    assert_eq!(read(&repo, "README.md"), USER_README);                         // seed untouched
}
#[test] fn rerun_after_restore_succeeds() {
    // make the v2 step fail via a var-controlled env (`SPLICE_FIXTURE_FAIL=1` read by the step command),
    // run update → exit 3; run `git checkout -- . && git clean -fd`; unset the env; update → exit 1 (the yaml conflict), lock = 2.0.0
}
#[test] fn resolve_conflict_then_check_passes() { /* edit ci.yml to remove markers, commit → `splice check --offline` exit 0 */ }
```

- [ ] **Step 2: Run, review the snapshots by hand against the fixture table, accept with `cargo insta accept`**
- [ ] **Step 3: Fix any planner/merge bug the snapshot exposes (test first in the owning module)**
- [ ] **Step 4: Run `cargo nextest run` — PASS**
- [ ] **Step 5: Commit** — `test: golden v1 to v2 update with edits on every class`

---

### Task 8: `splice check`

**Files:**
- Create: `src/ops/check.rs`
- Test: `tests/cli_check.rs`

**Interfaces:**
- Consumes: `Lock`, `find_markers`, `sha256_hex`, `Fetcher::available_versions`
- Produces: `fn ops::check::run(g: &GlobalArgs, a: &CheckArgs) -> Result<Report>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn clean_repo_passes() { /* new at v2 → check --offline exit 0, changes [] */ }
#[test] fn owned_drift_fails() { /* edit .editorconfig → exit 1, change {path:".editorconfig",action:"update",class:"owned"} */ }
#[test] fn markers_fail_and_stray_separator_does_not() {
    // conflicted update → check exit 1, change action "conflict" for ci.yml
    // resolve, add "=======\n" to README.md (seed, not in lock) and to notes.txt → exit 0
}
#[test] fn update_available() { /* repo at v1, template has v2 → exit 1, message "update available: 1.0.0 → 2.0.0"; --offline → exit 0 */ }
#[test] fn fetch_failure_is_message_not_error() { /* template dir removed → exit 0 with message when nothing else is wrong */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement** spec §8.7. **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: splice check`

---

### Task 9: `splice diff` and `add` on the full planner

**Files:**
- Create: `src/ops/diff.rs`; Modify: `src/ops/add.rs` (delete `plan_reduced`, use `planner::plan` + `apply` with `When::Add` and no steps)
- Test: `tests/cli_diff.rs`, `tests/cli_add.rs`

**Interfaces:**
- Consumes: `ops::update::prepare`, `planner::plan`, `apply`
- Produces: `fn ops::diff::run(g: &GlobalArgs, a: &DiffArgs) -> Result<Report>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn diff_writes_nothing() {
    // repo at v1 with user edits; `splice diff` → stdout contains "--- a/.editorconfig" and "+indent_size = 4";
    // `git status --porcelain` empty; exit 0 even when the update would conflict (diff reports, it does not fail)
}
#[test] fn diff_json_changes_match_update() { /* diff --json changes == update --json changes (same repo, two clones) */ }
#[test] fn add_merges_edited_shared_file() {
    // v1 root `package.json.jinja` lists `{% for p in pieces %}` entries under "workspaces";
    // user adds "private": true; `add app --name admin` → action "merge", both changes present, exit 0
}
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement**; update the v1 fixture's root `package.json.jinja` to list `{% for p in pieces %}` entries if not already there (and refresh plan 01 snapshots). **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: splice diff; add uses the full planner`

---

## Self-review notes

- Spec §8.3 step 2 "moved tag" (same version, different commit) is covered by `prepare` (message + proceed); add a unit test in Task 6 if the implementer finds it cheap.
- `explain` (plan 01) is unaffected; `check` does not render (lock + disk only, plus `ls-remote`), so it is fast and offline-safe.
- Release, npm, Action and skill file are plan 03.
