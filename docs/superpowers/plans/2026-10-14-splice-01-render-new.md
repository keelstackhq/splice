# splice Milestone 1 — Templates, rendering, `new`, `add`, `explain` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `splice` binary that fetches a template (local git path or git URL, `//subdir`, tag or ref), validates it, renders the contract's subset, classifies files, and implements `new`, `add` (reduced planner), `explain` and the lock file, all with `--json`.

**Architecture:** One Rust crate (lib `splice` + bin `splice`). Pure modules (source parsing, manifests, subset gate, render, classify, lock) feed thin command modules in `src/ops/`. Git is the `git` CLI behind one wrapper module. Every command returns a `Report`; `main.rs` prints it and maps errors to exit codes.

**Tech Stack:** Rust stable, edition 2024 · clap 4.6 (derive) · minijinja 2.24 (not 3.0.0: released 2026-10-08, adopt after it settles) · toml 1.1 (manifest read) · toml_edit 0.25 (lock write) · serde 1 · serde_json 1 (`preserve_order`) · regex 1.13 · globset 0.4 · sha2 0.11 · semver 1 · walkdir 2.5 · tempfile 3 · dirs 7 · indexmap 2 · dialoguer 0.12 · anyhow 1 · thiserror 2 · dev: assert_cmd 2.2, predicates 3.1, insta 1.49 (`json`, `yaml` features). Test runner: cargo-nextest.

**Spec:** `docs/superpowers/specs/2026-10-14-splice-design.md` · **Contract:** `docs/template-format.md`

## Global Constraints

- Cargo package `keel-splice`, `[lib] name = "splice"`, `[[bin]] name = "splice"`.
- The contract (`docs/template-format.md`) is binding and is never edited in this repo alone.
- Exit codes: 0 ok · 1 drift/conflict/update available/owned-drift refusal · 2 usage · 3 template or environment error.
- `--json` prints exactly one object `{"ok","command","changes":[{"path","action","class"}],"messages"}` to stdout, nothing else on stdout. `action` ∈ `create|update|merge|conflict|delete|skip`; `class` ∈ `owned|shared|seed`.
- Rendering subset exactly as spec §6 grammar; MiniJinja `Environment::empty()`, filters `lower`/`upper` only, `UndefinedBehavior::Strict`, no auto-escape, `keep_trailing_newline(true)`.
- Output uses `\n`; files rendering to only whitespace are not written; non-`.jinja` files are copied byte for byte.
- Ownership: rendered path relative to `files/` minus `.jinja`, before `target`; owned → shared → seed globs, default owned; `literal_separator(true)`.
- Lock at `.splice/lock.toml`, written last, via temp file + rename; seed files never listed; sha256 of rendered bytes.
- All git calls: `git -c core.autocrlf=false -c core.eol=lf`, env `GIT_TERMINAL_PROMPT=0`.
- Cache root `$SPLICE_CACHE_DIR` else `dirs::cache_dir()/splice`; mirrors `git/<sha256(url)[..16]>.git`, trees `trees/<sha256(url)[..16]>/<commit>/`.
- No network or TLS crates. No `unsafe`.

## Review Focus

1. **A var value like `../../etc` or `a/b` that passes a loose `pattern`** → rendered path escapes the repo or gains a segment → exit 3, nothing written. Test in Task 6.
2. **A template that uses syntax outside the subset (`{% set %}`, `{{ a ~ b }}`, `{%- if %}`, `loop.index`)** → exit 3 naming file and line, even if MiniJinja could render it. Test in Task 5.
3. **A `.jinja` file whose last line ends with `\n`** → output keeps the final newline (MiniJinja drops it by default). Test in Task 5 (conformance corpus case `trailing-newline`).
4. **`splice new` with `--json` in a non-TTY and a var with no default** → exit 2, one JSON object naming the var and `--data`, no prompt hang. Test in Task 8.
5. **`splice add app` twice with the same name, or `add lib` when `lib` is not `multiple` and present** → exit 2, tree and lock untouched. Test in Task 9.

---

## File Structure

```
Cargo.toml  rust-toolchain.toml (channel = "stable")  .gitignore (target/)
src/main.rs          parse → run → print Report → exit code
src/lib.rs           pub mods
src/cli.rs           clap Cli, Command, GlobalArgs
src/error.rs         SpliceError + exit_code()
src/output.rs        Report, Change, Action, Class, print_human, print_json
src/source.rs        TemplateSource::parse
src/git.rs           Git wrapper: ls_remote_tags, ensure_mirror, extract_tree, is_clean, init
src/fetch.rs         Fetcher: resolve / resolve_version / checkout / available_versions
src/manifest.rs      TemplateManifest, PieceManifest, VarSpec, PostCmd, Template::load + validate
src/vars.rs          resolve_vars (flags → prompt → default), pattern check
src/subset.rs        validate(text, path) per spec §6 grammar
src/render.rs        Renderer (MiniJinja env), render_tree(...) -> RenderedTree
src/classify.rs      Classifier per piece (globset), Class, MergeStrategy
src/paths.rs         render_rel_path safety, RelPath
src/lock.rs          Lock (serde) read/write_atomic, from_tree
src/fsio.rs          sha256_hex, write_file (mkdir -p), read_opt
src/ops/{mod,new,add,explain}.rs
tests/common/mod.rs  template_repo(), splice_cmd(), tree_snapshot()
tests/fixtures/template/{v1,v2,v3}/   tests/fixtures/subset/
tests/cli_new.rs  tests/cli_add.rs  tests/cli_explain.rs  tests/conformance.rs
.github/workflows/ci.yml   (minimal now; finalized in plan 03)
```

---

### Task 1: Scaffold, errors, Report and CLI skeleton

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `src/main.rs`, `src/lib.rs`, `src/cli.rs`, `src/error.rs`, `src/output.rs`, `.github/workflows/ci.yml`
- Test: `src/output.rs`, `src/error.rs` (unit), `tests/cli_basic.rs`

**Interfaces:**
- Produces:
  - `enum Class { Owned, Shared, Seed }`, `enum Action { Create, Update, Merge, Conflict, Delete, Skip }` — serde lowercase.
  - `struct Change { path: String, action: Action, class: Class }`
  - `struct Report { ok: bool, command: String, changes: Vec<Change>, messages: Vec<String> }` with `fn new(command: &str) -> Self`, `fn push(&mut self, path, action, class)`, `fn msg(&mut self, s: impl Into<String>)`, `fn to_json(&self) -> String` (changes sorted by path), `fn print(&self, json: bool)` (JSON or terse human lines).
  - `enum SpliceError { Usage(String), Template(String), Env(String), Conflict(Report), Drift(Report), Io(#[from] std::io::Error) }` with `fn exit_code(&self) -> i32` (Usage 2, Template/Env/Io 3, Conflict/Drift 1).
  - `struct GlobalArgs { json: bool, yes: bool, dir: PathBuf /* -C */ }`; `enum Command { New(NewArgs), Add(AddArgs), Update(UpdateArgs), Check(CheckArgs), Diff(DiffArgs), Explain(ExplainArgs) }` with all flags from spec §3 (update/check/diff bodies return `SpliceError::Usage("not implemented")` until plan 02).
  - `fn run(cli: Cli) -> Result<Report, SpliceError>` in `lib.rs`.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn report_json_shape() {
    let mut r = Report::new("new");
    r.push("b.txt", Action::Create, Class::Seed); r.push("a.txt", Action::Merge, Class::Shared);
    let v: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
    assert_eq!(v, serde_json::json!({"ok":true,"command":"new",
      "changes":[{"path":"a.txt","action":"merge","class":"shared"},{"path":"b.txt","action":"create","class":"seed"}],
      "messages":[]}));
}
#[test] fn exit_codes() {
    assert_eq!(SpliceError::Usage("x".into()).exit_code(), 2);
    assert_eq!(SpliceError::Template("x".into()).exit_code(), 3);
    assert_eq!(SpliceError::Conflict(Report::new("update")).exit_code(), 1);
}
// tests/cli_basic.rs
#[test] fn bad_flag_with_json_is_one_json_object_exit_2() {
    let out = splice_cmd().args(["new", "--json", "--bogus"]).assert().code(2).get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false); assert_eq!(v["command"], "new");
}
```

- [x] **Step 2: Run `cargo nextest run` — expect FAIL (types missing)**
- [x] **Step 3: Implement.** `main.rs` uses `Cli::try_parse()`; on clap error, if argv contains `--json` print an error Report (command = first non-flag arg if it names a subcommand) and exit 2, else let clap print and exit 2. On `Err(e)`: Conflict/Drift print their Report with `ok=false`; others print a Report with the message. CI (temporary, plan 03 finalizes): one job, `dtolnay/rust-toolchain@stable` + rustfmt/clippy, `Swatinem/rust-cache@v2`, `taiki-e/install-action@nextest`, `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo nextest run --locked`.
- [x] **Step 4: Run `cargo nextest run` — expect PASS; clippy clean**
- [x] **Step 5: Commit** — `feat: scaffold splice CLI, report and exit codes`

---

### Task 2: Template sources and the git wrapper

**Files:**
- Create: `src/source.rs`, `src/git.rs`, `tests/common/mod.rs`
- Test: `src/source.rs` (unit), `tests/git.rs`

**Interfaces:**
- Produces:
  - `enum SourceKind { Git { url: String }, Local { path: PathBuf } }`; `struct TemplateSource { spec: String, kind: SourceKind, subdir: Option<String> }`; `fn TemplateSource::parse(spec: &str) -> Result<Self, SpliceError>`; `fn cache_key(&self) -> String` (sha256 of url or canonical path, first 16 hex).
  - `struct Git { git_dir: Option<PathBuf> }` with `fn ls_remote_tags(url: &str) -> Result<Vec<(String /*tag*/, String /*sha*/)>>` (peeled sha preferred), `fn ensure_mirror(url, mirror: &Path) -> Result<()>` (clone `--mirror` or `fetch --tags --prune`), `fn rev_parse(git_dir, rev) -> Result<String>`, `fn extract_tree(git_dir, commit, dest: &Path) -> Result<()>` (temp `GIT_INDEX_FILE`, `--work-tree <tmp> checkout <commit> -- .`, rename tmp → dest), `fn is_clean(repo: &Path) -> Result<Option<bool>>` (None = not a work tree), `fn init(dir: &Path) -> Result<()>`.
  - `tests/common`: `fn template_repo(versions: &[&str]) -> TempDir` (copies `tests/fixtures/template/<v>` in order, commits each, tags `v<v>.0.0`), `fn splice_cmd() -> assert_cmd::Command` (sets `SPLICE_CACHE_DIR`, `GIT_CONFIG_GLOBAL`=empty file, `GIT_AUTHOR_*`/`GIT_COMMITTER_*`), `fn tree_snapshot(dir: &Path) -> String` (sorted `path sha256` lines + text contents, ignoring `.git`).

- [x] **Step 1: Write failing tests**

```rust
#[test] fn parses_sources() {
    let s = TemplateSource::parse("gh:keelstack/hull//template").unwrap();
    assert_eq!(s.kind, SourceKind::Git { url: "https://github.com/keelstack/hull.git".into() });
    assert_eq!(s.subdir.as_deref(), Some("template"));
    let s = TemplateSource::parse("https://example.com/x/y.git//a/b").unwrap();
    assert_eq!((s.kind, s.subdir.as_deref()), (SourceKind::Git { url: "https://example.com/x/y.git".into() }, Some("a/b")));
    assert!(matches!(TemplateSource::parse("./tpl").unwrap().kind, SourceKind::Local { .. }));
    assert_eq!(TemplateSource::parse("../t//sub").unwrap().subdir.as_deref(), Some("sub"));
    assert!(TemplateSource::parse("gh:onlyowner").is_err());
    assert!(TemplateSource::parse("gh:a/b//../x").is_err()); // subdir must not escape
}
// tests/git.rs
#[test] fn lists_tags_and_extracts_tree() {
    let repo = template_repo(&["v1", "v2"]);
    let tags = Git::ls_remote_tags(repo.path().to_str().unwrap()).unwrap();
    assert_eq!(tags.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["v1.0.0", "v2.0.0"]);
    let dest = tempdir().unwrap().path().join("t");
    Git::extract_tree(&repo.path().join(".git"), &tags[0].1, &dest).unwrap();
    assert!(dest.join("splice.toml").exists());
}
```

- [x] **Step 2: Run — expect FAIL**
- [x] **Step 3: Implement both modules.** `git@host:path` and `ssh://`/`file://` are Git; anything else without a scheme is Local. Split `//subdir` on the first `//` after the scheme's own `//`. Map non-zero git exits to `SpliceError::Env` with git's stderr (secrets in URLs are git's concern; never add tokens to URLs). Create the v1 fixture minimal now (`splice.toml` + `pieces/base/piece.toml`); Task 4 fills it.
- [x] **Step 4: Run — expect PASS**
- [x] **Step 5: Commit** — `feat: template source parsing and git CLI wrapper`

---

### Task 3: Fetch and cache resolution

**Files:**
- Create: `src/fetch.rs`
- Test: `tests/fetch.rs`

**Interfaces:**
- Consumes: `TemplateSource`, `Git` (Task 2)
- Produces:
  - `struct ResolvedTemplate { source: TemplateSource, version: semver::Version, commit: String, root: PathBuf /* tree + subdir, contains splice.toml */ }`
  - `struct Fetcher { cache: PathBuf }` with `fn from_env() -> Self` (`$SPLICE_CACHE_DIR` else `dirs::cache_dir()/splice`) and `fn new(cache: PathBuf) -> Self` (tests; no `std::env::set_var`, which is `unsafe` in edition 2024)
  - `fn Fetcher::resolve(&self, src: &TemplateSource, git_ref: Option<&str>, pre: bool) -> Result<ResolvedTemplate>` — latest stable `v<semver>` tag, or `--ref` (tag → its version; other → `0.0.0+g<sha7>`).
  - `fn Fetcher::resolve_version(&self, src, version: &semver::Version) -> Result<ResolvedTemplate>` (tag `v<version>`)
  - `fn Fetcher::checkout(&self, src: &TemplateSource, commit: &str, version: semver::Version) -> Result<ResolvedTemplate>` — no network when the tree is cached.
  - `fn Fetcher::available_versions(&self, src, pre: bool) -> Result<Vec<semver::Version>>` (sorted ascending)

- [x] **Step 1: Write failing tests**

```rust
#[test] fn latest_stable_and_cache_reuse() {
    let repo = template_repo(&["v1", "v2"]); let cache = tempdir().unwrap();
    let f = Fetcher::new(cache.path().into());
    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    let r = f.resolve(&src, None, false).unwrap();
    assert_eq!(r.version, Version::new(2, 0, 0));
    assert!(r.root.starts_with(cache.path().join("trees")));
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();          // source gone
    assert!(f.checkout(&src, &r.commit, r.version.clone()).is_ok());     // served from cache
}
#[test] fn ref_and_prerelease() {
    let repo = template_repo(&["v1"]); tag(&repo, "v1.1.0-rc.1");
    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    let f = Fetcher::new(tempdir().unwrap().keep());
    assert_eq!(f.resolve(&src, None, false).unwrap().version, Version::parse("1.0.0").unwrap());
    assert_eq!(f.resolve(&src, None, true).unwrap().version, Version::parse("1.1.0-rc.1").unwrap());
    assert!(f.resolve(&src, Some("main"), false).unwrap().version.to_string().starts_with("0.0.0+g"));
}
#[test] fn missing_subdir_or_commit_is_template_error() { /* `//nope` → exit-3 error naming the path; unknown sha → names the sha */ }
```

- [x] **Step 2: Run — expect FAIL**
- [x] **Step 3: Implement.** Local sources use their own `.git` as the mirror; remote ones `ensure_mirror` first. Extraction goes to `trees/<key>/<commit>.tmp-<pid>` then renames (another process may win the rename — treat an existing dest as success).
- [x] **Step 4: Run — expect PASS**
- [x] **Step 5: Commit** — `feat: resolve template versions and cache trees by commit`

---

### Task 4: Manifests, validation and the fixture template

**Files:**
- Create: `src/manifest.rs`, `tests/fixtures/template/v1/**`
- Test: `src/manifest.rs` (unit, inline TOML), `tests/manifest.rs` (fixture)

**Interfaces:**
- Produces:
  - `struct VarSpec { prompt: String, default: Option<String>, pattern: Option<regex::Regex> }`
  - `struct PostCmd { run: String, when: Vec<When> }`, `enum When { New, Add, Update }`
  - `struct TemplateManifest { format: u32, name: String, min_splice: semver::Version, base: String, vars: IndexMap<String, VarSpec> }`
  - `struct PieceManifest { id: String, description: String, multiple: bool, requires: Vec<String>, target: Option<String>, vars: IndexMap<String, VarSpec>, owned: Vec<String>, shared: Vec<String>, seed: Vec<String>, merge: IndexMap<String, MergeStrategy>, post: Vec<PostCmd>, files_dir: PathBuf }`
  - `struct Template { root: PathBuf, manifest: TemplateManifest, pieces: IndexMap<String, PieceManifest> }` (pieces sorted by id); `fn Template::load(root: &Path) -> Result<Template>` runs every check in spec §5 except `min_splice`; `fn check_min_splice(&self, running: &semver::Version) -> Result<()>`.
  - `enum MergeStrategy { Json, Yaml, Toml, Text }` (defined here, re-exported by classify).

Fixture v1 (every contract feature; plan 02 adds v2/v3):

| Piece | Feature |
|---|---|
| `base` (`multiple=false`) | owned `.editorconfig`, `tool.config.mts`, binary owned `logo.png`; shared `package.json.jinja` (json), `config/settings.toml.jinja` (toml), `.github/workflows/ci.yml.jinja` (yaml), `notes.txt.jinja` (text); seed `README.md.jinja`; `{{ project }}.code-workspace.jinja` (path var, owned by default); `apps.txt.jinja` using `{% for p in pieces %}`; `only-with-app.txt.jinja` empty unless `"app" in pieces`; `[[post]] run = "echo post> .post-ran"`, `when = ["new","update"]` |
| `app` (`multiple=true`, `requires=["base"]`, `target="apps/{{ name }}"`) | var `name` (pattern `^[a-z][a-z0-9-]*$`, default `web`); owned `tsconfig.json`; shared `package.json.jinja` (json); seed `src/**`; `{{ name | upper }}` and `{% if %}/{% elif %}/{% else %}` in `src/main.ts.jinja`; `{# comment #}` |
| `lib` (`multiple=false`, `requires=["app"]`) | requires check; owned `lib.txt` |
| template vars | `scope` (pattern `^[a-z0-9-]+$`, default `acme`), `project` (no default) |

- [x] **Step 1: Write failing tests**

```rust
#[test] fn loads_fixture() {
    let t = Template::load(Path::new("tests/fixtures/template/v1")).unwrap();
    assert_eq!((t.manifest.name.as_str(), t.manifest.base.as_str()), ("fixture", "base"));
    assert_eq!(t.pieces.keys().collect::<Vec<_>>(), ["app", "base", "lib"]);
    assert!(t.pieces["app"].multiple);
}
#[test] fn rejects_bad_manifests() {
    for (toml, needle) in [
        (r#"format = 2 …"#, "format"),                     // unknown format
        (r#"… base = "nope""#, "base"),                     // missing base piece
        (/* piece var named "pieces" */ "…", "reserved"),
        (/* piece var shadows template var "scope" */ "…", "scope"),
        (/* default "Bad!" vs pattern */ "…", "pattern"),
        (/* [merge] "x.json" = "xml" */ "…", "merge"),
        (/* requires = ["ghost"] */ "…", "ghost"),
    ] { assert!(load_inline(toml).unwrap_err().to_string().contains(needle)); }
}
#[test] fn min_splice_too_new() {
    let t = load_inline_with_min("99.0.0");
    let e = t.check_min_splice(&Version::new(0, 1, 0)).unwrap_err();
    assert_eq!(e.exit_code(), 3); assert!(e.to_string().contains("99.0.0"));
}
```

(`load_inline` writes the strings into a temp template dir; the implementer fills in the full TOML for each case.)

- [x] **Step 2: Run — expect FAIL**
- [x] **Step 3: Implement** with `toml` + serde raw structs, then convert + validate into the types above. Unknown keys → a warning string collected in `Template::warnings: Vec<String>` (reported as messages).
- [x] **Step 4: Run — expect PASS**
- [x] **Step 5: Commit** — `feat: parse and validate template and piece manifests`

---

### Task 5: Subset gate, renderer and conformance corpus

**Files:**
- Create: `src/subset.rs`, `src/render.rs` (engine part), `tests/fixtures/subset/*`, `tests/conformance.rs`
- Test: `src/subset.rs` (unit), `tests/conformance.rs`

**Interfaces:**
- Produces:
  - `fn subset::validate(text: &str, origin: &str) -> Result<(), SpliceError>` — error names `origin:line` and the tag.
  - `fn subset::validate_path_segment(seg: &str, origin: &str) -> Result<()>` (only `{{ expr }}`).
  - `struct Renderer { env: minijinja::Environment<'static> }` with `fn new() -> Self` and `fn render_str(&self, text: &str, ctx: &minijinja::Value, origin: &str) -> Result<String>` (validate → render → normalise `\r\n` → `\n`).
- Corpus cases (each `name.jinja`, `name.ctx.json`, `name.out`): `var`, `filters`, `if-elif-else`, `in-pieces`, `for-pieces`, `comment`, `dotted`, `trailing-newline`, `no-trailing-newline`, `blank-lines-around-blocks`, `unicode`.

- [x] **Step 1: Write failing tests**

```rust
#[test] fn accepts_subset() {
    for t in ["{{ name }}", "{{ name | upper }}", "{{piece.name|lower}}", "{% if a %}x{% elif \"app\" in pieces %}y{% else %}z{% endif %}",
              "{% for p in pieces %}{{ p }}{% endfor %}", "{# any } thing #}"] { assert!(validate(t, "t").is_ok(), "{t}"); }
}
#[test] fn rejects_everything_else() {
    for t in ["{% set x = 1 %}", "{{ a ~ b }}", "{%- if a %}{% endif %}", "{{ loop.index }}x", "{% include 'x' %}",
              "{% macro m() %}{% endmacro %}", "{{ name | title }}", "{% if not a %}{% endif %}", "{% if pieces %}{% endif %}",
              "{% for x in items %}{% endfor %}", "{% raw %}{% endraw %}", "{{ \"lit\" }}"] {
        let e = validate(t, "f.jinja").unwrap_err(); assert_eq!(e.exit_code(), 3, "{t}"); assert!(e.to_string().contains("f.jinja:1"));
    }
}
// tests/conformance.rs
#[test] fn corpus_renders_exactly() { /* for each case: Renderer::new().render_str(jinja, ctx) == out, byte for byte */ }
#[test] fn strict_undefined() { assert_eq!(Renderer::new().render_str("{{ nope }}", &ctx!{}, "t").unwrap_err().exit_code(), 3); }
```

The gate rejects the path roots `loop`, `self`, `range` and `namespace`; unknown names are left to strict undefined at render time.

- [x] **Step 2: Run — expect FAIL**
- [x] **Step 3: Implement.** The gate is a hand-written scanner over tag boundaries (`{{`, `{%`, `{#`) plus a tiny tokenizer per tag; do not use regex for nesting — `if`/`for` blocks must balance (`endif`/`endfor` in order) or it is an error. Renderer config exactly as Global Constraints.
- [x] **Step 4: Run — expect PASS**
- [x] **Step 5: Commit** — `feat: rendering subset gate, MiniJinja renderer, conformance corpus`

---

### Task 6: Path rendering, classification and render_tree

**Files:**
- Create: `src/paths.rs`, `src/classify.rs`, `src/fsio.rs`; Modify: `src/render.rs`
- Test: unit tests in each file, `tests/render_tree.rs`

**Interfaces:**
- Consumes: `Template`, `PieceManifest` (Task 4), `Renderer` (Task 5)
- Produces:
  - `struct Classifier { owned: GlobSet, shared: GlobSet, seed: GlobSet, merge: Vec<(GlobMatcher, MergeStrategy)>, globs: … }` with `fn for_piece(p: &PieceManifest) -> Result<Self>` and `fn classify(&self, rel: &str) -> (Class, MergeStrategy, Option<String> /* matching glob */)`.
  - `fn render_rel_path(src_rel: &str, target: Option<&str>, renderer, ctx) -> Result<(String /*final*/, String /*class key: rendered, no .jinja, no target*/)>` — rejects absolute, `..`, `.`, empty segments, `\`, and `.splice/lock.toml`.
  - `struct PieceInstance { id: String, name: String, vars: IndexMap<String, String> }`
  - `struct RenderInput { template_vars: IndexMap<String, String>, instances: Vec<PieceInstance>, version: semver::Version }`
  - `struct RenderedFile { path: String, bytes: Vec<u8>, class: Class, strategy: MergeStrategy, piece_id: String, piece_name: String, glob: Option<String> }`; `type RenderedTree = BTreeMap<String, RenderedFile>`
  - `fn render_tree(t: &Template, input: &RenderInput) -> Result<RenderedTree>`
  - `fn sha256_hex(bytes: &[u8]) -> String`; `fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> io::Result<()>`; `fn read_opt(root, rel) -> io::Result<Option<Vec<u8>>>`

- [x] **Step 1: Write failing tests**

```rust
#[test] fn first_match_owned_shared_seed_default_owned() {
    let c = classifier(&["vite.config.ts"], &["package.json"], &["src/**"], &[("package.json", MergeStrategy::Json)]);
    assert_eq!(c.classify("package.json").0, Class::Shared);
    assert_eq!(c.classify("package.json").1, MergeStrategy::Json);
    assert_eq!(c.classify("src/a/b.ts").0, Class::Seed);
    assert_eq!(c.classify("other.md"), (Class::Owned, MergeStrategy::Text, None));
    assert_eq!(c.classify("x/package.json").0, Class::Owned);       // `*`-free glob does not match deeper paths
}
#[test] fn unsafe_paths_rejected() {
    for v in ["../../etc", "a/b", "", "."] {
        assert_eq!(render_with_name_var(v).unwrap_err().exit_code(), 3, "{v}");     // via `{{ name }}` segment
    }
}
// tests/render_tree.rs (fixture v1, base + app "web" + app "admin")
#[test] fn renders_fixture() {
    let tree = render_fixture(&["base", "app:web", "app:admin"], &[("project", "demo")]);
    assert_eq!(tree["apps/web/package.json"].class, Class::Shared);
    assert_eq!(tree["apps/web/src/main.ts"].class, Class::Seed);
    assert!(tree.contains_key("demo.code-workspace"));
    assert!(tree.contains_key("only-with-app.txt"));
    assert_eq!(tree["logo.png"].bytes, std::fs::read("tests/fixtures/template/v1/pieces/base/files/logo.png").unwrap());
    assert_eq!(std::str::from_utf8(&tree["apps.txt"].bytes).unwrap(), "base\napp\n");      // unique ids
    insta::assert_snapshot!(tree_listing(&tree));
}
#[test] fn empty_render_not_written() { assert!(!render_fixture(&["base"], &[("project","demo")]).contains_key("only-with-app.txt")); }
#[test] fn same_path_twice_is_error() { /* two app instances with the same name var → exit 3 naming the path */ }
#[test] fn binary_shared_is_error() { /* temp template with shared logo.png → exit 3 */ }
```

- [x] **Step 2: Run — expect FAIL**
- [x] **Step 3: Implement.** Walk `files/` sorted; build the context per instance (`pieces` = unique ids of `input.instances` in order). Non-`.jinja` files are never decoded.
- [x] **Step 4: Run — expect PASS**
- [x] **Step 5: Commit** — `feat: render template trees with ownership classes`

---

### Task 7: Lock file

**Files:**
- Create: `src/lock.rs`
- Test: `src/lock.rs` (unit)

**Interfaces:**
- Consumes: `RenderedTree`, `PieceInstance`, `ResolvedTemplate`
- Produces:
  - `struct LockFile { class: Class, sha256: String }`; `struct LockPiece { id, name, vars: IndexMap<String, String> }`; `struct LockTemplate { source: String, version: semver::Version, commit: String }`
  - `struct Lock { format: u32, splice: semver::Version, template: LockTemplate, vars: IndexMap<String, String>, pieces: Vec<LockPiece>, files: BTreeMap<String, LockFile> }`
  - `fn Lock::from_tree(t: &ResolvedTemplate, vars, instances: &[PieceInstance], tree: &RenderedTree) -> Lock` (skips seeds)
  - `fn Lock::read(repo: &Path) -> Result<Option<Lock>>` (unknown `format` → Template error)
  - `fn Lock::write_atomic(&self, repo: &Path) -> Result<()>` (`.splice/lock.toml.tmp` + fsync + rename)
  - `fn Lock::instances(&self) -> Vec<PieceInstance>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn roundtrip_matches_contract_shape() {
    let lock = sample_lock();                  // one owned, one shared, one seed in the tree
    let dir = tempdir().unwrap(); lock.write_atomic(dir.path()).unwrap();
    let text = std::fs::read_to_string(dir.path().join(".splice/lock.toml")).unwrap();
    insta::assert_snapshot!(text);             // format/splice/[template]/[vars]/[[pieces]] vars inline/[files] sorted
    assert_eq!(Lock::read(dir.path()).unwrap().unwrap(), lock);
    assert!(!text.contains("README.md"));      // seed not listed
    assert!(!dir.path().join(".splice/lock.toml.tmp").exists());
}
#[test] fn unknown_format_is_template_error() { /* write format = 9 → read → exit 3 */ }
```

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** reading with `toml` + serde and writing with `toml_edit` (inline tables for `vars` and each `[files]` entry, as in contract §6).
- [ ] **Step 4: Run — expect PASS**
- [ ] **Step 5: Commit** — `feat: lock file read and atomic write`

---

### Task 8: Vars and `splice new`

**Files:**
- Create: `src/vars.rs`, `src/ops/mod.rs`, `src/ops/new.rs`, `src/post.rs`
- Test: `src/vars.rs` (unit), `tests/cli_new.rs`

**Interfaces:**
- Consumes: Tasks 1–7
- Produces:
  - `struct VarSource<'a> { data: &'a IndexMap<String, String>, interactive: bool }`; `fn resolve_vars(specs: &IndexMap<String, VarSpec>, known: &IndexMap<String,String>, src: &VarSource) -> Result<IndexMap<String, String>>` — order: `known` → `--data` → prompt (dialoguer `Input` with default, re-asks on pattern miss) → default → `Usage("missing value for var `x`; pass --data x=…")`. `--data` values failing `pattern` → Usage.
  - `fn interactive(global: &GlobalArgs) -> bool` = `!json && !yes && stdin().is_terminal()`
  - `fn instance_name(piece: &PieceManifest, explicit: Option<&str>, vars: &IndexMap<String,String>) -> String` (spec §8.1)
  - `fn run_post(repo: &Path, cmds: &[PostCmd], when: When, json: bool) -> Result<()>` — `sh -c` / `cmd /C`, stdout+stderr to our stderr when `json`, dedupe identical `run` strings, piece order; non-zero → Env error naming the command.
  - `fn ops::new::run(g: &GlobalArgs, a: &NewArgs) -> Result<Report>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn new_renders_fixture_and_writes_lock() {
    let tpl = template_repo(&["v1"]); let dest = tempdir().unwrap();
    let out = splice_cmd().args(["new", tpl_str(&tpl), dest_str(&dest), "--piece", "app:web", "--data", "project=demo", "--yes", "--json"])
        .assert().success().get_output().stdout.clone();
    let r: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(r["ok"], true); assert_eq!(r["command"], "new");
    assert!(dest.path().join(".splice/lock.toml").exists());
    assert!(dest.path().join(".git").exists());                         // git init
    assert!(dest.path().join(".post-ran").exists());                    // post ran
    insta::assert_snapshot!(tree_snapshot(dest.path()));
    insta::assert_json_snapshot!(r);
}
#[test] fn missing_var_noninteractive_is_usage_json() {
    // no --data project=…, stdin not a TTY
    let out = splice_cmd().args(["new", tpl, dest, "--json"]).assert().code(2).get_output().stdout.clone();
    let r: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(r["messages"][0].as_str().unwrap().contains("--data project="));
}
#[test] fn nonempty_dest_refused_without_force() { /* file in dest → exit 2; with --force → ok and seed kept */ }
#[test] fn requires_checked() { /* --piece lib without app → exit 2 naming "app" */ }
#[test] fn bad_pattern_data_is_usage() { /* --data scope=Bad! → exit 2 */ }
#[test] fn no_post_flag() { /* --no-post → .post-ran absent */ }
#[test] fn min_splice_refused() { /* template_repo(&["v1","v2","v3"]) + --ref v3.0.0 → exit 3, dest empty */ }
```

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** spec §8.1 in order: resolve → load → min_splice → dest check → instances → vars → render → write files (seeds only if absent) → lock → `git init` → post. Report actions: `create` per written file, `skip` per kept seed.
- [ ] **Step 4: Run — expect PASS**
- [ ] **Step 5: Commit** — `feat: splice new`

---

### Task 9: `splice add` (reduced planner)

**Files:**
- Create: `src/ops/add.rs`
- Test: `tests/cli_add.rs`

**Interfaces:**
- Consumes: `Lock`, `Fetcher::checkout`, `render_tree`, `resolve_vars`, `run_post`
- Produces: `fn ops::add::run(g: &GlobalArgs, a: &AddArgs) -> Result<Report>`; `fn plan_reduced(base: &RenderedTree, theirs: &RenderedTree, repo: &Path, lock: &Lock, force: bool) -> Result<Vec<Planned>>` with `struct Planned { path: String, action: Action, class: Class, bytes: Option<Vec<u8>> }` — plan 02 replaces this with the full planner.

Reduced rules: new path → create (seed: only if absent); path in both with changed render → replace when disk sha = lock sha (owned/shared) else `conflict` without writing (message: "edited locally; run `splice update` after milestone 2 or merge by hand"); seed → skip; path only in base → delete when unedited. Any conflict → exit 1 after writing the rest and the lock.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn add_second_app_updates_pieces_dependent_files() {
    let (tpl, repo) = new_repo(&["--data", "project=demo"]);                 // base only
    splice_cmd().current_dir(&repo).args(["add", "app", "--name", "admin", "--yes", "--json"]).assert().success();
    assert!(repo.path().join("apps/admin/package.json").exists());
    assert!(repo.path().join("only-with-app.txt").exists());                  // `"app" in pieces` now true
    assert_eq!(read(&repo, "apps.txt"), "base\napp\n");
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.pieces.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["base", "admin"]);
}
#[test] fn duplicate_name_and_single_piece_twice_are_usage() {
    // add app --name web twice → 2nd exit 2; add lib twice → 2nd exit 2; tree and lock bytes unchanged after each failure
}
#[test] fn edited_file_reported_not_overwritten() { /* edit apps.txt (owned by default), add app → exit 1, apps.txt unchanged, change action "conflict" */ }
#[test] fn add_without_lock_is_usage() { /* empty dir → exit 2 */ }
```

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** spec §8.2 with `plan_reduced`; base and theirs both render at the lock commit.
- [ ] **Step 4: Run — expect PASS**
- [ ] **Step 5: Commit** — `feat: splice add with reduced planner`

---

### Task 10: `splice explain`

**Files:**
- Create: `src/ops/explain.rs`
- Test: `tests/cli_explain.rs`

**Interfaces:**
- Consumes: `Lock`, `Fetcher::checkout`, `render_tree`
- Produces: `fn ops::explain::run(g: &GlobalArgs, a: &ExplainArgs) -> Result<Report>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn explains_classes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web", "--data", "project=demo"]);
    splice_cmd().current_dir(&repo).args(["explain", "apps/web/package.json"]).assert().success()
        .stdout(predicates::str::contains("shared").and(predicates::str::contains("app/web")).and(predicates::str::contains("merge=json")));
    let v = json_of(splice_cmd().current_dir(&repo).args(["explain", "apps/web/src/main.ts", "--json"]));
    assert_eq!(v["changes"][0], serde_json::json!({"path":"apps/web/src/main.ts","action":"skip","class":"seed"}));
    assert!(v["messages"].as_array().unwrap().iter().any(|m| m == "glob=src/**"));
}
#[test] fn unmanaged_path() { /* explain notes.md (not rendered) → exit 0, no changes, message "unmanaged" */ }
#[test] fn works_offline_after_new() { /* delete the template repo dir, explain still succeeds (cache) */ }
```

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** spec §8.9. Accept paths with `./` prefix or `\` separators by normalising first.
- [ ] **Step 4: Run — expect PASS**
- [ ] **Step 5: Commit** — `feat: splice explain`

---

## Self-review notes

- Spec §8.3–8.8 (`update`, planner, merges, `check`, `diff`) and fixtures v2/v3 contents are plan 02; Task 8 only needs v3's `min_splice` (create `v3` as a copy of v1 with `min_splice = "99.0.0"` in Task 8).
- `update`/`check`/`diff` subcommands parse in Task 1 and return a usage error until plan 02.
- Spec §12–14 (release, npm, Action, skill) are plan 03.
