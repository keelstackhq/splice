# splice Milestone 4 — channels ("rigging") — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply extra format-1 templates ("channels") next to the main template (or alone), each with its own lock in `.splice/channels/<name>.toml`, piece selection, per-repo `--only`/`--exclude` filter, optional `--track BRANCH`, and owned Markdown **sections** inside files nobody owns; `update`, `check`, `diff`, `explain` and `add` learn `--channel`/`--all`; released as splice 0.2.0.

**Architecture:** A channel is planned exactly like `update` (spec §8.3–8.4): render base (lock commit, old filter) and theirs (target, new filter) into memory, `planner::plan` turns them into `Planned` changes, and a new pure section planner turns rendered fragments + host files into `SectionChange`s. A source registry loads every lock (main + channels) and a claim check rejects any path owned by two sources before anything is written. `apply` gains a `LockOut` (main or channel lock) and section writes, keeping the fixed order files → section hosts → removals → steps → post → lock. `update --all` plans every source in memory, checks claims on the combined result, then applies source by source.

**Tech Stack:** only crates already in `Cargo.toml`: clap 4.6 (derive; `conflicts_with`, nested subcommand), globset 0.4 (filters, `literal_separator(true)` as `classify.rs`), toml 1.1 / toml_edit 0.25 (channel lock read/write), sha2 0.11 (section sha), indexmap 2, semver 1, minijinja 2.24 (fragments, same `Renderer`), tempfile 3, walkdir 2.5; dev: assert_cmd 2.2, insta 1.49 (json), tempfile. **No new crates.** Git via the `git` CLI only.

**Spec:** `docs/superpowers/specs/2026-10-17-splice-channels-design.md` (all) · main spec `2026-10-14-splice-design.md` §3, §6, §8.3–8.9, §10, §13 · **Contract:** `docs/template-format.md` — **unchanged by this plan** (channels spec §4). Never edit it here.

## Global Constraints

- Everything in plans 01 and 02's Global Constraints (exit codes, `--json` one object, lock last and atomic, dirty tree → exit 2, owned drift → nothing written, exit 1).
- `.splice/lock.toml` is byte-for-byte what M2 writes, with or without channels present (contract guard test, Task 8). The main lock never gets `[channel]` or `[sections]`.
- Channel lock = contract §6 shape + optional `[channel]` (after `splice`) and `[sections]` (after `[files]`); written with `fsio::write_atomic` to `.splice/channels/<name>.toml`, only by that channel's own operation, last for that source.
- Channel names `^[a-z][a-z0-9-]*$`. Unknown, duplicate or bad name → exit 2.
- One owner per path: a path in two sources' owned/shared sets, or a section host in any source's `[files]`, → exit 2 with `path X is owned by <main|channel NAME>; exclude it with --exclude X or remove it from one source`; nothing written.
- No rendered path under `.splice/` for any source.
- A channel never sees the main template's vars or pieces; render context is its own lock.
- Section markers exactly `<!-- splice:<channel>:begin — managed by \`splice update --channel <channel>\`; edit the source -->` and `<!-- splice:<channel>:end -->` (the dash is U+2014).
- Source attribution in JSON goes in `messages` only (`source=main`, `source=channel:NAME`, `channel NAME: …`), never extra fields. `command` values: `channel add`, `channel remove`, `update`, `check`, `diff`, `explain`, `add`.
- `splice update` without `--channel`/`--all` behaves exactly as in M2 (all existing tests stay green unchanged).
- Tests are hermetic via `tests/common` (`splice_cmd`, `template_repo*`, the new `channel_repo`); no network.
- Cross-repo tasks (C1–C3) are done in the named repo, in their own PRs; human-only items go to `docs/PROGRESS.md` → Blocked.

## Rulings on spec gaps (record in main spec §10 item 15 in Task 19)

- **G1** The channels spec cites "main spec §10 item 10", which already exists (subset gate). The channel decision is recorded as **item 15**, marked *splice-only, not proposed for contract §9*.
- **G2** "No rendered path under `.splice/`" applies to the main template too (stricter than contract §9, which reserves only `lock.toml`; hull renders nothing there).
- **G3** `--track`: the target is always the branch head (tags ignored); `--to` on a tracked channel → exit 2 (`channel N tracks BRANCH; re-add it with --ref to pin a version`). No ancestry check; no downgrade check between `0.0.0+g…` versions; up to date = same commit.
- **G4** Subtree compare (spec §8 `check`) also applies to `update --channel`: target subtree id equal to the lock commit's → "up to date", lock unchanged (no lock-only bot PRs). Main template: unchanged M2 behaviour.
- **G5** A `[sections]` key is a host path relative to the piece's `target`, rendered like a file path (`render_rel_path`); the value is a literal path under `files/`. The fragment file is excluded from the rendered tree. Host must end `.md`; fragment `.md` or `.md.jinja`. A fragment that renders to whitespace only declares no section.
- **G6** Section changes use path `HOST#CHANNEL`, class `owned`, in `changes` of every command (`create`/`update`/`delete`/`skip`/`conflict`), not only `check`.
- **G7** The block body is the fragment with exactly one trailing `\n`; written with the host's line ending (CRLF if the host contains `\r\n`); `sha256` is over the LF-normalised body.
- **G8** On update, fragment unchanged → skip even if the body was edited (`check` still reports it). Host already holding this channel's markers but no lock entry (adoption) → adopt if the body equals the fragment, else drift.
- **G9** `--force` never overrides duplicate/unbalanced markers (exit 1, user fixes the file).
- **G10** A seed path that another source locks in `[files]` is dropped from this source's plan (never written, no error).
- **G11** `channel add` with a shared-file conflict follows `update` (markers written, lock written, exit 1). `splice add --channel` follows `add` (C3: writes nothing on drift/conflict).
- **G12** `update --all`: each source is applied with `no_post`; the deduplicated `[[post]]` of all sources runs after the last lock, skipped if any source conflicted. A failed post there exits 3 and says every lock already records its target, plus the restore command.
- **G13** Main lock containing `[channel]` or `[sections]`, a channel lock without `[channel]` or whose `name` ≠ file stem, or a file in `.splice/channels/` that is not `<valid-name>.toml` → exit 3 ("corrupt lock", names the file).
- **G14** `--only`/`--exclude`/`--to` need `--channel`; `--all` conflicts with `--channel`, `--to`, `--data`, `--only`, `--exclude` (clap → exit 2).
- **G15** Test fixtures use `min_splice = "0.1.0"`; the crate version moves to `0.2.0` in Task 21 (real channel templates in C1/C2 use `min_splice = "0.2.0"`).

## Review Focus

1. **A channel and the main template both want `CLAUDE.md`** (owned by hull, channel section host) → `channel add` exits 2, names `main` and the `--exclude` flag; `git status --porcelain` empty. Task 10.
2. **A user edited text inside a section, then the channel releases a new fragment** → `update --channel` exits 1, nothing written in any file; `--force` replaces only the block, text outside the markers byte-identical (CRLF host stays CRLF). Tasks 6, 12.
3. **`update --all` where channel `b` has owned drift** → no file and no lock of main, `a` or `b` changes; exit 1; message names `channel b`. Task 14.
4. **A crash between source 1's lock and source 2's files** → source 1's lock matches its files, source 2's lock is old; message names source 2 and the restore command; rerun after restore succeeds. Task 14.
5. **`decisions` tracked on `main`, a commit lands that only touches files outside `channel/`** → `check` exit 0, no "update available"; a commit inside `channel/` → exit 1 with `channel decisions: update available 3f2c1ab → 9e01d2c (main)`. Tasks 13, 15.
6. **A repo with channels runs plain `splice update`/`new`** → `.splice/lock.toml` byte-identical to a channel-free repo; channel locks untouched. Task 8, 14.

---

## File Structure

```
src/lock.rs           + ChannelMeta, SectionLock, ChannelLock, channel_lock_path; main-lock corruption check
src/paths.rs          rendered paths under `.splice/` rejected
src/filter.rs         Filter (only/exclude globs), keeps(), apply_tree()
src/sources.rs        SourceId, Sources::load, Claim, check_claims
src/sections.rs       markers, find_block, insert/replace/remove, body_sha, plan_sections
src/manifest.rs       PieceManifest.sections; validation
src/render.rs         render_sections; fragments excluded from render_tree
src/apply.rs          LockOut, ApplyCtx.{command, sections}, section writes
src/fetch.rs src/git.rs   branch_head, tree_id (subtree compare)
src/cli.rs src/main.rs src/lib.rs   `channel add|remove`, --channel/--all/--only/--exclude
src/ops/channel.rs    channel add, channel remove
src/ops/{update,add,check,diff,explain}.rs   channel-aware
tests/fixtures/channel/{v1,v2}/**   tests/fixtures/subset/section-fragment.*
tests/common/mod.rs   channel_repo(), track_commit()
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

v2 also changes `core`'s seed `docs/agents/notes.md` (must stay untouched). `channel_repo(&["v1","v2"])` tags `v1.0.0`, `v2.0.0`; `track_commit(repo, inside: bool)` adds a commit on `main` that touches `channel/…` or only `README.md` outside it (the channel then lives under `//channel`, see Task 7).

---

### Task 1: Channel lock types, corruption checks, `.splice/` path rule

**Files:**
- Modify: `src/lock.rs`, `src/paths.rs`
- Test: unit tests in both

**Interfaces:**
- Consumes: `Lock`, `fsio::{read_opt, write_atomic}`, `toml_edit`
- Produces:
  - `pub const CHANNELS_DIR: &str = ".splice/channels";` `pub fn channel_lock_path(name: &str) -> String` (`.splice/channels/<name>.toml`)
  - `pub fn valid_channel_name(name: &str) -> bool` (`^[a-z][a-z0-9-]*$`)
  - `#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct ChannelMeta { pub name: String, pub track: Option<String>, pub only: Vec<String>, pub exclude: Vec<String> }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SectionLock { pub sha256: String, pub created: bool }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct ChannelLock { pub meta: ChannelMeta, pub lock: Lock, pub sections: BTreeMap<String, SectionLock> }`
  - `impl ChannelLock { pub fn read(repo: &Path, name: &str) -> Result<Option<ChannelLock>>; pub fn read_all(repo: &Path) -> Result<Vec<ChannelLock>> /* sorted by name */; pub fn write_atomic(&self, repo: &Path) -> Result<()>; pub fn path(&self) -> String; pub fn to_toml(&self) -> String }`
  - `Lock::read` refactored onto a private `fn parse_lock(text: &str, path: &str) -> Result<(Lock, toml::Table)>`; `bad_lock` takes the path. `Lock::read` rejects `[channel]`/`[sections]` (G13).
  - `paths::render_rel_path` rejects any path whose first segment is `.splice` (G2).

- [ ] **Step 1: Write failing tests**

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
#[test] fn main_lock_bytes_unchanged() {   // contract guard at the type level
    assert_eq!(sample_lock().to_toml(), include_str!("../tests/fixtures/lock/main-m2.toml"));
}
#[test] fn main_lock_with_channel_table_is_corrupt() { /* write [channel] into .splice/lock.toml → Lock::read exit 3, message names .splice/lock.toml */ }
#[test] fn main_lock_with_sections_is_corrupt() { /* same for [sections] */ }
#[test] fn channel_lock_without_channel_table_is_corrupt() { /* exit 3, names .splice/channels/x.toml */ }
#[test] fn channel_name_must_match_file() { /* [channel] name = "y" in x.toml → exit 3 */ }
#[test] fn stray_file_in_channels_dir_is_corrupt() { /* .splice/channels/Bad.toml or notes.txt → read_all exit 3 */ }
#[test] fn channel_names() {
    for ok in ["a", "decisions", "rig-2"] { assert!(valid_channel_name(ok)); }
    for bad in ["", "A", "1a", "-a", "a_b", "a/b", "a.b"] { assert!(!valid_channel_name(bad)); }
}
// paths.rs
#[test] fn nothing_renders_under_dot_splice() {
    for (src, target) in [(".splice/x", None), ("channels/a.toml", Some(".splice")), (".splice", None)] {
        assert_eq!(render_rel_path(src, target, &Renderer::new(), &context!{}).unwrap_err().exit_code(), 3);
    }
    assert!(render_rel_path(".splicex/a", None, &Renderer::new(), &context!{}).is_ok());
}
```

`tests/fixtures/lock/main-m2.toml` is generated once from M2's `to_toml` before refactoring (commit it in this task).

- [ ] **Step 2: Run `cargo nextest run -E 'test(lock) | test(paths)'` — FAIL**
- [ ] **Step 3: Implement.** `ChannelLock::to_toml` builds the main document via a shared private `Lock::to_document()` and inserts `channel` after `splice` and `sections` after `files` (toml_edit `position`). `[sections]` entries are inline tables `{ sha256, created }`; `only`/`exclude` always written (empty arrays allowed).
- [ ] **Step 4: Run — PASS; `cargo nextest run` all green (existing lock tests unchanged)**
- [ ] **Step 5: Commit** — `feat: channel lock type and .splice/ path rule`

---

### Task 2: Filters

**Files:**
- Create: `src/filter.rs`; Modify: `src/lib.rs`
- Test: unit tests in `src/filter.rs`

**Interfaces:**
- Consumes: `globset`, `RenderedTree`
- Produces:
  - `pub struct Filter { only: Vec<(String, GlobMatcher)>, exclude: Vec<(String, GlobMatcher)> }`
  - `impl Filter { pub fn new(only: &[String], exclude: &[String]) -> Result<Filter> /* bad glob → Usage, names the flag */; pub fn from_meta(m: &ChannelMeta) -> Result<Filter>; pub fn keeps(&self, path: &str) -> bool; pub fn apply_tree(&self, tree: RenderedTree) -> RenderedTree; pub fn unused(&self, paths: &[&str]) -> Vec<String> /* "filter `G` matches nothing" */ }`

- [ ] **Step 1: Write failing tests**

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
#[test] fn unused_globs_reported() {
    let f = Filter::new(&[], &s(&["nope/**", "a.md"])).unwrap();
    assert_eq!(f.unused(&["a.md"]), vec!["filter `nope/**` matches nothing".to_string()]);
}
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement** (globs built like `classify::compile_one`, `literal_separator(true)`). **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: channel path filters`

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
  - `impl Claim { pub fn from_lock(source: SourceId, lock: &Lock, sections: &BTreeMap<String, SectionLock>) -> Claim; pub fn from_plan(source: SourceId, theirs: &RenderedTree, hosts: impl IntoIterator<Item = String>) -> Claim }`
  - `pub fn check_claims(claims: &[Claim]) -> Result<()>` — Usage (exit 2); one message line per conflicting path, sorted.
  - `pub fn replace_claim(current: Vec<Claim>, new: Claim) -> Vec<Claim>` (swap the source's current claim for its planned one)

- [ ] **Step 1: Write failing tests**

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
#[test] fn load_reads_main_and_channels_sorted() { /* temp repo with lock + channels b, a → names [a, b] */ }
#[test] fn unknown_channel_lists_known() { /* Sources.channel("zz") exit 2, message contains "a, b" */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** The message names the *other* source than the one being checked: claims are checked in order, and the earlier claim holder is named. **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: source registry and one-owner claim check`

---

### Task 4: `[sections]` in piece manifests, fragment rendering, main-template refusal

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

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn sections_parsed() { /* channel fixture v1 core: sections["AGENTS.md"] == "sections/rules.md.jinja"; no warnings */ }
#[test] fn non_md_host_or_fragment_rejected() { /* "AGENTS.txt" = …, or fragment "x.txt" → exit 3 naming piece.toml */ }
#[test] fn missing_fragment_rejected() { /* fragment path not under files/ → exit 3 */ }
#[test] fn fragment_not_in_tree() { /* render_tree(channel v1, core) has no key starting with "sections/" */ }
#[test] fn sections_render_with_target_and_pieces() {
    // core + claude: AGENTS.md body contains the claude-only line; without claude it does not
}
#[test] fn two_pieces_same_host_rejected() { /* fixture variant: claude also declares "AGENTS.md" → exit 3 naming both pieces */ }
#[test] fn whitespace_fragment_declares_nothing() { /* fragment renders "\n" → host absent from RenderedSections */ }
#[test] fn new_refuses_sections() {
    // splice new <channel fixture> → exit 3, message "sections are only applied by `splice channel add`", dest empty
}
#[test] fn conformance_section_fragment() { /* new corpus case: `{% if "claude" in pieces %}- Use the explorer agent.\n{% endif %}` */ }
```

- [ ] **Step 2: Run — FAIL**
- [ ] **Step 3: Implement.** Host keys render through `paths::render_rel_path(key, piece.target, …)` (G5); fragments render with `Renderer::render_str` in the instance context; empty-after-trim → no section. Add the corpus case to `CASES`. Note in PROGRESS that hull's Nunjucks corpus run must pick up the new case (C2).
- [ ] **Step 4: Run — PASS (`cargo nextest run`)**
- [ ] **Step 5: Commit** — `feat: piece [sections] and fragment rendering`

---

### Task 5: Section text operations

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

- [ ] **Step 1: Write failing tests**

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

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: section markers and block edits`

---

### Task 6: Section planner

**Files:**
- Modify: `src/sections.rs`
- Test: unit tests (table-driven, one per channels spec §6 rule)

**Interfaces:**
- Consumes: `Disk`, `RenderedSections`, `SectionLock`, Task 5 helpers
- Produces:
  - `pub struct SectionInput<'a> { pub channel: &'a str, pub theirs: &'a RenderedSections, pub disk: &'a Disk, pub locked: &'a BTreeMap<String, SectionLock>, pub filter: &'a Filter, pub removing: bool /* channel remove */, pub force: bool }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SectionChange { pub host: String, pub action: Action, pub bytes: Option<Vec<u8>> /* new host; None with Delete = delete host */, pub note: Option<String> }` and `fn report_path(&self, channel: &str) -> String` (`HOST#CHANNEL`)
  - `#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct SectionPlan { pub changes: Vec<SectionChange>, pub drift: Vec<String> /* HOST#CHANNEL */, pub broken: Vec<String> /* messages with line numbers */, pub locked: BTreeMap<String, SectionLock> /* the new [sections] */ }`
  - `pub fn plan_sections(input: &SectionInput) -> SectionPlan` (pure)
  - `pub fn host_paths(theirs: &RenderedSections, locked: &BTreeMap<String, SectionLock>) -> BTreeSet<String>` (for the disk snapshot)

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn table() {
    // (name, theirs_body, disk_host, locked_sha_of, force, expected_action, expected_host_after, created_after)
    let cases = [
        ("insert, host absent",        Some("x\n"), None,                       None,        false, Some(Create), Some(blk("x\n")),                Some(true)),
        ("insert, host present",       Some("x\n"), Some("# T\n"),              None,        false, Some(Create), Some(format!("# T\n\n{}", blk("x\n"))), Some(false)),
        ("adopt equal markers",        Some("x\n"), Some(&blk("x\n")),          None,        false, None,         None,                            Some(false)),
        ("adopt differing markers",    Some("x\n"), Some(&blk("u\n")),          None,        false, None /*drift*/, None,                          None),
        ("replace unedited",           Some("y\n"), Some(&blk("x\n")),          Some("x\n"), false, Some(Update), Some(blk("y\n")),                Some(false)),
        ("fragment unchanged",         Some("x\n"), Some(&blk("x\n")),          Some("x\n"), false, None,         None,                            Some(false)),
        ("unchanged, body edited",     Some("x\n"), Some(&blk("u\n")),          Some("x\n"), false, None,         None,                            Some(false)), // G8
        ("drift",                      Some("y\n"), Some(&blk("u\n")),          Some("x\n"), false, None /*drift*/, None,                          None),
        ("drift --force",              Some("y\n"), Some(&blk("u\n")),          Some("x\n"), true,  Some(Update), Some(blk("y\n")),                Some(false)),
        ("markers deleted",            Some("y\n"), Some("# T\n"),              Some("x\n"), false, None /*drift*/, None,                          None),
        ("markers deleted --force",    Some("y\n"), Some("# T\n"),              Some("x\n"), true,  Some(Create), Some(format!("# T\n\n{}", blk("y\n"))), Some(false)),
        ("gone, unedited",             None,        Some(&format!("# T\n\n{}", blk("x\n"))), Some("x\n"), false, Some(Delete), Some("# T\n".into()), None),
        ("gone, edited",               None,        Some(&blk("u\n")),          Some("x\n"), false, Some(Skip),   None,                            None),
        ("gone, created host emptied", None,        Some(&blk("x\n")),          Some("x\n") /*created*/, false, Some(Delete), None /*host deleted*/, None),
    ];
    for c in cases { check_case(c); }
}
#[test] fn broken_markers_refused_even_with_force() { /* duplicate markers → plan.broken non-empty with "lines 1, 3", force true still broken */ }
#[test] fn filtered_host_is_removed() { /* filter excludes AGENTS.md → like "gone" */ }
#[test] fn removing_plans_every_locked_section_as_gone() {}
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement** per channels spec §6 and G7–G9. **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: pure section planner`

---

### Task 7: Channel fixtures and test helpers

**Files:**
- Create: `tests/fixtures/channel/v1/**`, `tests/fixtures/channel/v2/**` (table above); Modify: `tests/common/mod.rs`
- Test: `tests/manifest.rs`

**Interfaces:**
- Produces (tests/common):
  - `pub fn channel_repo(versions: &[&str]) -> TempDir` — a git repo whose `channel/` subdir holds `tests/fixtures/channel/<v>`, plus a top-level `README.md`; each version committed on `main` and tagged `v<N>.0.0`. Source spec in tests: `format!("{}//channel", repo.path().display())`.
  - `pub fn track_commit(repo: &Path, inside: bool) -> String` — new commit on `main` (touching `channel/pieces/core/files/docs/agents/rules.md` when `inside`, else `README.md`); returns the full sha.
  - `pub fn new_repo_at(tpl: &TempDir, tag: &str) -> TempDir` — `splice new` of the main fixture at `tag`, `git init` + commit (refactor of `golden_update::new_at` into common).
  - `pub fn bare_repo() -> TempDir` — an empty git repo with one commit (`README.md`), no lock.
  - `pub fn chan(repo: &Path, args: &[&str]) -> assert_cmd::assert::Assert` (`splice_cmd -C repo --yes --json` + args)

- [ ] **Step 1: Write failing tests** — `channel_fixture_v1_loads` (pieces `claude, collide, core, hooks, skills`; `core.sections` set; v2 migration removes `.claude/agents/explorer.md`); `channel_repo_has_subdir_and_tags` (`git tag` lists both; `channel/splice.toml` exists; `track_commit(_, false)` leaves `channel/` tree id unchanged — assert with `git rev-parse HEAD:channel`).
- [ ] **Step 2: Run — FAIL.**  **Step 3: Write fixtures and helpers.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `test: channel fixture v1/v2 and helpers`

---

### Task 8: Apply with `LockOut` and section writes; contract guard

**Files:**
- Modify: `src/apply.rs`, `src/ops/{new,add,update}.rs` (callers pass `LockOut::Main`, `command`)
- Test: `src/apply.rs` (unit), `tests/cli_update.rs` (guard)

**Interfaces:**
- Consumes: `ChannelLock::write_atomic`, `SectionChange`
- Produces:
  - `pub enum LockOut { Main(Lock), Channel(ChannelLock) }` with `fn write_atomic(&self, repo: &Path) -> Result<()>`, `fn path(&self) -> String`, `fn version(&self) -> &Version`
  - `ApplyCtx { …, pub command: &'static str, pub lock: LockOut, pub sections: &'a [SectionChange], pub channel: Option<&'a str> }`
  - Order: file writes → section host writes/deletes → file removals → steps → post → lock. Section changes are pushed to the report as `HOST#CHANNEL`/owned. Error texts say `the lock <path> still records <from>`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn section_written_before_removals_and_lock() {
    // plan deletes old.txt; one SectionChange creates AGENTS.md; step checks
    // `test -f AGENTS.md && test ! -f old.txt && test ! -f .splice/channels/rig.toml`
    let r = apply(ctx_channel("rig", plan_deleting("old.txt"), &[sec_create("AGENTS.md", "x\n")], step)).unwrap();
    assert!(r.changes.iter().any(|c| c.path == "AGENTS.md#rig" && c.class == Class::Owned));
    assert!(repo.join(".splice/channels/rig.toml").is_file() && !repo.join(".splice/lock.toml").exists());
}
#[test] fn section_delete_host() { /* SectionChange{Delete, bytes None} removes AGENTS.md */ }
#[test] fn failing_step_leaves_old_channel_lock() { /* message contains ".splice/channels/rig.toml still records 1.0.0" */ }
#[test] fn command_name_in_report() { /* command "channel add" → report.command == "channel add" */ }
```

CLI guard (`tests/cli_update.rs`):

```rust
#[test] fn main_lock_identical_with_channels_present() {
    // two repos from the same main fixture at v1; in one, write a channel lock by hand (copied fixture
    // file) and commit; `update --yes` both to v2 → `.splice/lock.toml` bytes equal; channel lock unchanged
}
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement**; existing apply tests keep passing with `LockOut::Main`. **Step 4: Run `cargo nextest run` — PASS.**
- [ ] **Step 5: Commit** — `feat: apply channel locks and section writes`

---

### Task 9: CLI grammar for channels

**Files:**
- Modify: `src/cli.rs`, `src/lib.rs`, `src/main.rs`, `src/ops/mod.rs`; Create: `src/ops/channel.rs` (functions return `Usage("splice channel add: not implemented yet")` until Tasks 10/17)
- Test: `tests/cli_basic.rs`

**Interfaces:**
- Produces:
  - `Command::Channel(ChannelArgs)`; `#[derive(Subcommand)] pub enum ChannelCommand { Add(ChannelAddArgs), Remove(ChannelRemoveArgs) }`; `Command::NAMES` gains `"channel"`; `Command::name()` returns `"channel add"` / `"channel remove"`; `main.rs` builds `channel add|remove` for clap-error JSON.
  - `ChannelAddArgs { source: String, as_name: Option<String> /* --as */, git_ref: Option<String> /* --ref, conflicts_with track */, track: Option<String>, pre: bool, pieces: Vec<String>, data: Vec<String>, only: Vec<String>, exclude: Vec<String>, allow_dirty: bool, force: bool, no_post: bool }`
  - `ChannelRemoveArgs { name: String, allow_dirty: bool, force: bool }`
  - `AddArgs.channel: Option<String>`; `UpdateArgs { …, channel: Option<String>, all: bool, only: Vec<String>, exclude: Vec<String> }`; `CheckArgs.channel: Option<String>`; `DiffArgs.channel: Option<String>`; conflicts per G14.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn ref_and_track_conflict() { splice_cmd().args(["--json", "channel", "add", "x", "--ref", "v1", "--track", "main"]).assert().code(2); }
#[test] fn channel_and_all_conflict() { splice_cmd().args(["update", "--channel", "a", "--all"]).assert().code(2); }
#[test] fn filter_flags_need_channel() { splice_cmd().args(["update", "--only", "a/**"]).assert().code(2); }
#[test] fn all_rejects_to() { splice_cmd().args(["update", "--all", "--to", "2.0.0"]).assert().code(2); }
#[test] fn clap_error_json_names_channel_command() {
    // `splice --json channel add` (missing source) → stdout one object, command == "channel add", exit 2
}
#[test] fn help_lists_channel() { /* `splice --help` contains "channel" */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: channel command grammar`

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

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn add_into_new_rendered_repo() {
    // main fixture repo at v1 + channel v1: `channel add <src>//channel --ref v1.0.0 --piece claude --piece skills:claude --data dir=claude`
    // → exit 0; .claude/agents/{explorer,reviewer}.md, .claude/skills/{caveman,verify}/SKILL.md, AGENTS.md and CLAUDE.md with rig blocks;
    // .splice/channels/rig.toml has [channel] name="rig", track absent, [sections] AGENTS.md + CLAUDE.md, created = true;
    // .splice/lock.toml bytes unchanged; JSON command "channel add", changes include "AGENTS.md#rig"
}
#[test] fn add_into_bare_repo() { /* bare_repo(): no main lock → ok; Sources has only rig */ }
#[test] fn name_defaults_and_as() { /* default name "rig"; --as Other → exit 2; --as rig2 → .splice/channels/rig2.toml; markers use rig2 */ }
#[test] fn name_taken() { /* second add → exit 2 "use splice update --channel rig" */ }
#[test] fn dirty_tree_refused() { /* untracked file → exit 2; --allow-dirty → 0 */ }
#[test] fn claim_conflict_writes_nothing() {
    // --piece collide → exit 2, message "path .editorconfig is owned by main; exclude it with --exclude .editorconfig …";
    // `git status --porcelain` empty. Same with `--exclude .editorconfig` → exit 0, .editorconfig untouched, not in rig.toml
}
#[test] fn host_owned_by_main_refused() { /* main fixture variant owning CLAUDE.md, --piece claude → exit 2 naming main */ }
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
```

- [ ] **Step 2: Run `cargo nextest run --test cli_channel` — FAIL**
- [ ] **Step 3: Implement:** dirty check → `Sources::load` → resolve (`--ref`/`--track`/latest, `--pre`) → load, `check_min_splice` → name → instances (base + `--piece`) → `render_tree` + `render_sections` → filter → claims (`replace_claim` with `Claim::from_plan`) → drop seeds claimed elsewhere (G10) → disk snapshot (tree + hosts) → `plan` + `plan_sections` → drift/broken → refuse (exit 1, nothing written) → `apply` with `LockOut::Channel`, `When::Add`, `command = "channel add"`. Conflicts per G11.
- [ ] **Step 4: Run — PASS**
- [ ] **Step 5: Commit** — `feat: splice channel add`

---

### Task 11: `splice add --channel`

**Files:**
- Modify: `src/ops/add.rs`
- Test: `tests/cli_channel.rs`

**Interfaces:**
- Consumes: `Sources::channel`, `Filter::from_meta`, `plan_sections`, claim check
- Produces: `ops::add::run` handles `a.channel`: base = channel lock state, theirs = with the new instance, both at the channel's commit, through the stored filter; writes only that channel's lock; C3 refusal unchanged (G11).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn second_harness_instance() {
    // after channel add (--piece skills:claude --data dir=claude):
    // `add skills --channel rig --name agents --data dir=agents` → .agents/skills/{caveman,verify}/SKILL.md, same bytes as .claude/skills/…;
    // rig.toml [[pieces]] has skills/claude and skills/agents; main lock bytes unchanged
}
#[test] fn add_channel_unknown() { /* --channel zz → exit 2 listing "rig" */ }
#[test] fn add_channel_respects_filter() { /* stored exclude '.agents/skills/caveman/**' → not written */ }
#[test] fn add_channel_claim_conflict() { /* add collide --channel rig → exit 2, nothing written */ }
#[test] fn add_channel_updates_section_with_new_piece() { /* add claude later → AGENTS.md block gains the claude-only line (update action on AGENTS.md#rig) */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: add pieces to a channel`

---

### Task 12: `splice update --channel`

**Files:**
- Modify: `src/ops/update.rs`
- Test: `tests/cli_channel_update.rs`

**Interfaces:**
- Produces:
  - `pub fn prepare_channel(g: &GlobalArgs, cl: &ChannelLock, others: &[Claim], to: Option<&Version>, filter_new: Option<(&[String], &[String])>, force: bool) -> Result<Prepared>`
  - `Prepared { …, pub channel: Option<ChannelPrepared> }` with `pub struct ChannelPrepared { pub lock_new: ChannelLock, pub sections: SectionPlan, pub claim: Claim }` (`None` for main; existing fields unchanged)
  - `prepare` (main) keeps its signature; the shared body becomes `fn prepare_lock(g, lock: &Lock, target: Target, data, force) -> Result<(Prepared, Rendered)>` with `pub enum Target<'a> { Latest, Version(&'a Version), Branch(&'a str) }` and `struct Rendered { base: RenderedTree, theirs: RenderedTree, base_sections: RenderedSections, theirs_sections: RenderedSections }`
  - Base filtered with the stored filter, theirs with `filter_new` (or the stored one); a new filter is stored in `lock_new.meta`.
  - Channel-only repo + plain `update` → exit 2 `no .splice/lock.toml; use --channel NAME or --all`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn update_channel_v1_to_v2() {
    // channel at v1 (claude, skills:claude, hooks); update --channel rig → exit 0: reviewer.md replaced, explorer.md deleted
    // ([[remove]]), verify/SKILL.md replaced, AGENTS.md#rig update, docs/agents/notes.md (seed) untouched,
    // rig.toml version 2.0.0; main lock bytes unchanged; .rig-post.log has one line
}
#[test] fn section_drift_refused_then_forced() {
    // edit inside the AGENTS.md block + outside it, commit; update → exit 1, `git status` empty, message names AGENTS.md#rig and --force;
    // --force → block replaced, outside edit kept byte-identical
}
#[test] fn crlf_host_stays_crlf() { /* AGENTS.md converted to CRLF and committed → after update every line ends \r\n */ }
#[test] fn deleted_markers_are_drift() { /* remove the block lines → exit 1; --force re-appends */ }
#[test] fn broken_markers_exit_1_even_forced() { /* duplicate begin line → exit 1 with "lines" in message, also with --force */ }
#[test] fn filter_change_on_update() {
    // update --channel rig --exclude '.claude/skills/caveman/**' → caveman deleted (unedited), stored exclude replaced;
    // edited caveman → kept, message, not in [files]; later `--exclude ''`-free update with --only '**' re-creates it
}
#[test] fn owned_drift_in_channel_refused() { /* edit reviewer.md → exit 1, nothing written */ }
#[test] fn plain_update_untouched_by_channels() { /* main + channel; `update` → only main moves; rig.toml unchanged */ }
#[test] fn channel_only_repo_plain_update() { /* bare repo + channel; `update` → exit 2 "use --channel NAME or --all" */ }
#[test] fn claim_conflict_on_update() { /* v2 variant where claude also renders .editorconfig → exit 2, nothing written */ }
#[test] fn unknown_channel() { /* --channel zz → exit 2 */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement** spec §8.3 steps 1–8 on the channel lock plus claims and sections (Rulings G3–G10). **Step 4: Run — PASS; all of `cli_update` unchanged and green.**
- [ ] **Step 5: Commit** — `feat: splice update --channel`

---

### Task 13: `--track` updates and subtree compare

**Files:**
- Modify: `src/git.rs`, `src/fetch.rs`, `src/ops/update.rs`
- Test: `src/git.rs` (unit), `tests/cli_channel_update.rs`

**Interfaces:**
- Produces:
  - `Git::tree_id(git_dir: &Path, commit: &str, subdir: Option<&str>) -> Result<String>` (`git rev-parse --verify <commit>^{tree}` or `<commit>:<subdir>`)
  - `Fetcher::subtree_id(&self, src: &TemplateSource, commit: &str) -> Result<String>` (no fetch if the commit is in the mirror)
  - `Fetcher::git_dir(&self, src) -> Result<PathBuf>` made `pub(crate)` for the above
  - `prepare_channel`: tracked → `Target::Branch(track)`; `--to` with track → exit 2 (G3); subtree equal → up to date (G4).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn tree_id_of_subdir() { /* repo with a/x; tree_id(.., Some("a")) == `git rev-parse HEAD:a` */ }
#[test] fn tracked_channel_moves_to_head() {
    // channel add --track main; track_commit(inside=true); update --channel rig → exit 0, rig.toml commit = new head,
    // version 0.0.0+g<sha7>, rules.md updated
}
#[test] fn tracked_outside_change_is_up_to_date() { /* track_commit(inside=false) → exit 0, message "up to date", rig.toml bytes unchanged */ }
#[test] fn tracked_rejects_to() { /* update --channel rig --to 2.0.0 → exit 2 "re-add it with --ref" */ }
#[test] fn tagged_channel_same_subtree_is_up_to_date() { /* tag v3.0.0 on a commit changing only README.md → update exit 0, lock unchanged */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: tracked channels and subtree compare`

---

### Task 14: `splice update --all`

**Files:**
- Modify: `src/ops/update.rs`
- Test: `tests/cli_channel_update.rs`

**Interfaces:**
- Produces: `fn run_all(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report>` (called by `run` when `a.all`):
  1. dirty check; `Sources::load`; prepare main (latest, if present) and every channel (sorted) in memory;
  2. claims on the combined planned result (each source's planned claim);
  3. any drift/broken markers in any source → nothing written, exit 1, messages prefixed `channel NAME: ` / `main: `;
  4. apply per source (main first, then channels by name) with `no_post = true`: files, sections, removals, steps, lock;
  5. post commands of all sources, deduplicated by exact `run`, after the last lock (G12);
  6. one report: `changes` merged and sorted, `messages` with a `source=…` line per source that changed; conflicts in any source → exit 1.
  - A failure in source k: `Env` error naming k (`main` or `channel NAME`), saying sources before k are applied with matching locks, plus `git checkout -- . && git clean -fd`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn all_updates_main_then_channels() { /* main v1 + rig v1 → update --all: both at v2; .rig-post.log one line; messages contain source=main and source=channel:rig */ }
#[test] fn one_drifted_source_writes_nothing() { /* edit reviewer.md (rig owned) → exit 1, main lock and files unchanged, message "channel rig:" */ }
#[test] fn claim_conflict_across_new_versions() { /* main v2 variant and rig v2 variant both render x.txt → exit 2, nothing written */ }
#[test] fn post_dedup_and_after_locks() {
    // two channels from the same fixture (rig, rig2 via --as) both with hooks → `rig-post` runs once,
    // and the post command `grep -q 2.0.0 .splice/channels/rig2.toml && echo rig-post >> .rig-post.log` succeeds (locks written first)
}
#[test] fn failure_in_second_source_keeps_first() {
    // rig2's v2 migration step fails (env SPLICE_FIXTURE_FAIL=1) → exit 3; main lock at v2 matching its files;
    // rig2.toml still 1.0.0; message names "channel rig2" and the restore command; restore + rerun → exit 0
}
#[test] fn all_in_channel_only_repo() { /* bare + rig → update --all updates rig only, no lock.toml created */ }
#[test] fn all_up_to_date() { /* everything current → exit 0, message per source "up to date" */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: splice update --all`

---

### Task 15: `splice check` across sources

**Files:**
- Modify: `src/ops/check.rs`
- Test: `tests/cli_check.rs`

**Interfaces:**
- Produces: `ops::check::run`: without `--channel` → main (if present) + every channel; `--channel N` → that channel only. Per channel: owned drift, markers, section drift/broken markers (`HOST#CHANNEL`, `update`, `owned`), update available (newer stable tag or tracked head, only when `subtree_id` differs). Messages `channel NAME: update available A → B (BRANCH|tag)` with short shas for tracked. Main messages unchanged from M2 (no prefix) so existing tests and the bot keep parsing them.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn check_aggregates_sources() { /* main clean, rig owned drift → exit 1, change .claude/agents/reviewer.md update/owned, message names channel rig */ }
#[test] fn check_section_drift() { /* edit inside block → change {path:"AGENTS.md#rig", action:"update", class:"owned"} */ }
#[test] fn check_tracked_head_moved() { /* track_commit(inside) → exit 1, "channel rig: update available <a7> → <b7> (main)"; --offline → exit 0 */ }
#[test] fn check_tracked_outside_change() { /* track_commit(outside) → exit 0 */ }
#[test] fn check_channel_only_repo() { /* bare + rig, clean → exit 0 (no "no lock.toml" error) */ }
#[test] fn check_single_channel() { /* --channel rig ignores main drift */ }
#[test] fn check_main_messages_unchanged() { /* existing update_available test passes untouched */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: splice check covers channels and sections`

---

### Task 16: `splice diff --channel` and `explain` across sources

**Files:**
- Modify: `src/ops/diff.rs`, `src/ops/explain.rs`
- Test: `tests/cli_diff.rs`, `tests/cli_explain.rs`

**Interfaces:**
- Produces:
  - `diff --channel N [--to V]` runs `prepare_channel` and prints file patches plus section patches (host before/after, labels `a/HOST`, `b/HOST`); `--json` changes equal `update --channel N --json`'s.
  - `explain <path>` searches main then every channel: adds `source=main` / `source=channel:NAME`; a section host (in any channel's `[sections]`) adds `section=NAME` per block and, if no source owns the file, reports it `unmanaged` plus the section lines. A channel-only repo no longer exits 2 for `explain`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn diff_channel_writes_nothing() { /* rig at v1 → diff --channel rig shows "+" lines of reviewer.md and the AGENTS.md block; git status empty */ }
#[test] fn diff_channel_json_matches_update() {}
#[test] fn explain_channel_file() { /* .claude/agents/reviewer.md → messages piece=claude/claude, glob=…, merge=none, source=channel:rig */ }
#[test] fn explain_section_host() { /* AGENTS.md → messages include "section=rig"; unmanaged by files */ }
#[test] fn explain_main_file_has_source_main() { /* .editorconfig → "source=main" appended; other messages unchanged */ }
```

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: diff and explain for channels`

---

### Task 17: `splice channel remove`

**Files:**
- Modify: `src/ops/channel.rs`
- Test: `tests/cli_channel.rs`

**Interfaces:**
- Produces: `pub fn ops::channel::remove(g: &GlobalArgs, a: &ChannelRemoveArgs) -> Result<Report>` — plan = every locked file "B only" (base = render at the lock, theirs = empty) + `plan_sections` with `removing = true`; owned drift → exit 1 unless `--force` (then edited files are kept and reported, never deleted); apply without steps/post; delete `.splice/channels/NAME.toml` last (`fsio::remove_file`; remove `.splice/channels/` if empty).

- [ ] **Step 1: Write failing tests**

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

- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `feat: splice channel remove`

---

### Task 18: Golden main + channel update

**Files:**
- Create: `tests/golden_channels.rs`, snapshots
- Test: itself

**Interfaces:**
- Consumes: the binary, both fixtures.

- [ ] **Step 1: Write the tests**

```rust
#[test] fn golden_main_and_channel_v1_to_v2() {
    // main at v1 (apply_user_edits from golden_update, refactored into common) + rig v1 (claude, skills:claude, skills:agents, core, hooks);
    // user edits: AGENTS.md outside the block, CLAUDE.md inside the rig block (then reverted for the forced variant), seed notes.md;
    // update --all --yes --json → snapshot JSON + tree_snapshot
}
#[test] fn golden_forced_section_variant() { /* inside-block edit kept, update --all --force → snapshot */ }
#[test] fn contract_guard_golden() { /* the golden repo's .splice/lock.toml equals golden_update's snapshot lock text */ }
```

- [ ] **Step 2: Run, read every `.snap.new` against the fixture table and Rulings, accept with `cargo insta accept`**
- [ ] **Step 3: Fix any bug the snapshot exposes (test first in the owning module)**
- [ ] **Step 4: Run `cargo nextest run` — PASS; fmt + clippy clean**
- [ ] **Step 5: Commit** — `test: golden main plus channel update`

---

### Task 19: Spec, README and skill file

**Files:**
- Modify: `docs/superpowers/specs/2026-10-14-splice-design.md` (§3 table, §6 path rule, §8.7/§8.9 notes, §10 item 15 with G1–G15, §15 milestone 4), `docs/superpowers/specs/2026-10-17-splice-channels-design.md` (Status: approved; "§10 item 10" → "item 15"; link this plan), `README.md` (Channels section), `skills/splice/SKILL.md` (if plan 03 Task 5 has landed; else add the lines to plan 03 Task 5's checklist and note it in PROGRESS)
- Test: plan 03's `docs_mention_only_real_flags` (if present), else a new `tests/cli_basic.rs::readme_channel_flags_exist`

**Interfaces:** none (docs).

- [ ] **Step 1: Write failing test** — every `--[a-z-]+` in the README Channels section appears in `splice channel add --help`, `splice update --help`, `splice check --help` or `splice add --help`.
- [ ] **Step 2: Run — FAIL.**  **Step 3: Write.** SKILL.md gains ≤ 10 lines: channels exist when `.splice/channels/` has files; `splice explain <path> --json` names the source; never edit inside `<!-- splice:NAME:begin …` blocks, change the channel source instead; `splice check --json` covers every channel. **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** — `docs: channels in spec, README and skill`

---

### Task 20: Update-bot `scope` input

Depends on plan 03 Task 4 (`action/action.yml`, `action/summary.mjs`). If it has not landed, leave this task unchecked and note it in PROGRESS.

**Files:**
- Modify: `action/action.yml`, `action/summary.mjs`, `action/test/*.test.mjs`, README bot section
- Test: `node --test action/test`

**Interfaces:**
- Produces: input `scope` (`main` | `all`; default: `all` when `.splice/channels/` has a `*.toml`, else `main`). `all` runs `splice update --all --json --yes`; `main` runs today's command. One PR either way; the PR body groups rows by the `source=` messages. `scope: main` in a channel-only repo fails with splice's exit-2 message.

- [ ] **Step 1: Write failing tests** — `scopeDefault({channels: ["rig.toml"]}) === "all"`, `scopeDefault({channels: []}) === "main"`, `command("all")` equals `["update","--all","--json","--yes"]`, `summary()` groups `source=channel:rig` rows under a `rig` heading.
- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement.** **Step 4: Run — PASS; actionlint clean.**
- [ ] **Step 5: Commit** — `feat: update bot scope input for channels`

---

### Task 21: Version 0.2.0 and milestone wrap-up

**Files:**
- Modify: `Cargo.toml` (`version = "0.2.0"`), `Cargo.lock`, insta snapshots that contain `splice = "0.1.0"` (cli_new ×2, golden_update ×2, render_tree, golden_channels), `docs/PROGRESS.md` (Status, Blocked)
- Test: full suite

- [ ] **Step 1:** Bump the version; run `cargo nextest run`; read every `.snap.new` and accept only diffs that change `splice = "0.1.0"` → `"0.2.0"` (anything else is a bug).
- [ ] **Step 2:** `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo nextest run && node --test npm/splice/test action/test scripts/` — paste the summary into PROGRESS.
- [ ] **Step 3:** Add the Blocked items below to `docs/PROGRESS.md`.
- [ ] **Step 4: Commit** — `chore: splice 0.2.0`

---

## Cross-repo tasks (each in its own repo and PR; not in this repo)

### Task C1 (repo `keelstackhq/decisions`): `channel/` template

**Files (in keelstackhq/decisions):** `channel/splice.toml` (`name = "decisions"`, `base = "snapshot"`, `min_splice = "0.2.0"`), `channel/pieces/snapshot/{piece.toml,files/docs/agents/org-decisions.md}` (owned), `channel/pieces/import-project/` (`[sections] "docs/agents/project.md"`), `channel/pieces/import-claude/` (`"CLAUDE.md"`), `channel/pieces/import-agents/` (`"AGENTS.md"`: "Read docs/agents/org-decisions.md"), `scripts/check-index.sh` (fails unless `channel/pieces/snapshot/files/docs/agents/org-decisions.md` equals `DECISIONS.md`), `adopt/adopt.sh` (prints `deprecated: run splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project`), `adopt/README.md`.

- [ ] Step 1: Make `check-index.sh` fail on a deliberately different copy; then copy `DECISIONS.md`; `bash scripts/check-index.sh` passes.
- [ ] Step 2: With a local splice ≥ 0.2.0 build: `splice channel add <decisions checkout>//channel --track <branch> --piece import-project` into a scratch copy of hull — the existing snapshot adopts without `--force`; paste output.
- [ ] Step 3: PR `feat: decisions channel for splice` (owner merges, D-002). The SessionStart `sync-decisions.sh` stays until C3 replaces it per repo.

### Task C2 (repo `keelstackhq/hull`): `rigging/` channel and `AGENTS.md` → seed

Inside the contract (no `docs/template-format.md` change); hull's own plan/PR.

- [ ] `rigging/` (format 1, `name = "rigging"`, `base = "core"`, `min_splice = "0.2.0"`): `core` (`[sections] "AGENTS.md"` with the harness-neutral rules), `claude` (`.claude/agents/{explorer,implementer,reviewer,ci-fixer}.md` owned; `[sections] "CLAUDE.md"` pointer), `skills` (`multiple`, `target = ".{{ dir }}/skills"`, `dir` pattern `^(claude|agents)$`). Harness dirs checked against current Claude Code and Codex docs at write time.
- [ ] Reclassify `AGENTS.md` from owned to seed in hull's base piece; hull test for the class.
- [ ] Add splice's `section-fragment` corpus case to hull's Nunjucks conformance run.
- [ ] dogfood renders `rigging/` with splice once 0.2.0 is published (Blocked until then).
- [ ] A later hull release: `migrations/<v>.toml` `[[remove]]` for the four agent files; README + generated `CLAUDE.md` say: run `splice update`, then `splice channel add gh:keelstackhq/hull//rigging --piece claude` (not a post/step: the main lock is not yet written then).

### Task C3 (repos hull, splice, hoard, example-app, keelstack): adopt the decisions channel

One PR per repo, after splice 0.2.0 is on npm and C1 is merged:

- [ ] Delete `.claude/hooks/session-start.d/sync-decisions.sh` (or `.claude/hooks/sync-decisions.sh` and its `settings.json` entry); delete the hand-written "## Org decisions" block.
- [ ] `pnpm dlx @keelstackhq/splice@0.2.0 channel add gh:keelstackhq/decisions//channel --track main --piece import-project` (repos with `docs/agents/project.md`; else `--piece import-claude`). In hull-generated repos `CLAUDE.md` is owned by hull, so use `import-project`.
- [ ] Enable the update bot with `scope` default (`all`); `splice check --json` exit 0; CI green. Commit `chore: adopt decisions channel`.
- [ ] After all five: delete `adopt/adopt.sh` in keelstackhq/decisions (PR).

## Blocked (human-only; copy into `docs/PROGRESS.md`)

- **Release 0.2.0:** push tag `v0.2.0` on keelstackhq/splice after Task 21 is on `main` (needs plan 03's release workflow and `NPM_TOKEN`).
- **Decisions PR merge:** the owner merges C1 in keelstackhq/decisions (D-002).
- **Bot tokens:** each adopting repo needs the bot workflow enabled and a token that can read `keelstackhq/decisions` and `keelstackhq/hull` if they are private (repo secret, e.g. a GitHub App token passed as `token`).
- **hull release** removing the four agent files (C2 last step): tag a hull version.

## Self-review notes

- Spec §12 order, adjusted: lock + path rule (T1), filters (T2), registry + claims (T3), then sections (T4–T6) before `channel add` (T10) so `add` writes blocks from the start; `--channel`/`--all` (T11–T16) with `--track` (T13); docs (T19), bot (T20), release prep (T21), cross-repo (C1–C3).
- Spec §11 coverage: fixtures (T7), every CLI case (T10–T17), units (T1, T2, T5, T6), golden (T18), conformance (T4), contract guard (T1, T8, T18).
- No contract change anywhere; G1 is recorded in splice's spec only.
