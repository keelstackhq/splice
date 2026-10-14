use crate::error::SpliceError;
use crate::git::Git;
use crate::source::{SourceKind, TemplateSource};
use semver::Version;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, SpliceError>;

#[derive(Debug, Clone)]
pub struct ResolvedTemplate {
    pub source: TemplateSource,
    pub version: Version,
    pub commit: String,
    /// Extracted tree plus subdir; contains `splice.toml`.
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Fetcher {
    pub cache: PathBuf,
}

impl Fetcher {
    pub fn from_env() -> Self {
        let cache = match std::env::var_os("SPLICE_CACHE_DIR") {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("splice"),
        };
        Self { cache }
    }

    pub fn new(cache: PathBuf) -> Self {
        Self { cache }
    }

    /// The URL or path handed to `git ls-remote` / `clone`.
    fn url(src: &TemplateSource) -> String {
        match &src.kind {
            SourceKind::Git { url } => url.clone(),
            SourceKind::Local { path } => path.to_string_lossy().into_owned(),
        }
    }

    /// Directory of the git repository to read objects from.
    fn mirror(&self, src: &TemplateSource) -> PathBuf {
        match &src.kind {
            SourceKind::Git { .. } => self
                .cache
                .join("git")
                .join(format!("{}.git", src.cache_key())),
            SourceKind::Local { path } => {
                if path.join(".git").exists() {
                    path.join(".git")
                } else {
                    path.clone()
                }
            }
        }
    }

    /// A local source must be a git repository of its own: never let git discover a parent
    /// repository (such as the generated repo) from a missing or plain directory.
    fn ensure_local(src: &TemplateSource) -> Result<()> {
        let SourceKind::Local { path } = &src.kind else {
            return Ok(());
        };
        let is_repo = path.join(".git").exists()
            || (path.join("HEAD").is_file() && path.join("objects").is_dir());
        if is_repo {
            return Ok(());
        }
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        Err(SpliceError::Template(format!(
            "template source {} not found (resolved to {})",
            src.spec,
            abs.display()
        )))
    }

    /// Make the mirror current (remote sources only) and return its path.
    fn sync(&self, src: &TemplateSource) -> Result<PathBuf> {
        Self::ensure_local(src)?;
        let mirror = self.mirror(src);
        if let SourceKind::Git { url } = &src.kind {
            Git::ensure_mirror(url, &mirror)?;
        }
        Ok(mirror)
    }

    /// Stable and (with `pre`) pre-release `v<semver>` tags with their commit shas, ascending.
    fn tagged(src: &TemplateSource, pre: bool) -> Result<Vec<(Version, String)>> {
        Self::ensure_local(src)?;
        let mut out: Vec<(Version, String)> = Git::ls_remote_tags(&Self::url(src))?
            .into_iter()
            .filter_map(|(name, sha)| {
                let v = Version::parse(name.strip_prefix('v')?).ok()?;
                (pre || v.pre.is_empty()).then_some((v, sha))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub fn available_versions(&self, src: &TemplateSource, pre: bool) -> Result<Vec<Version>> {
        Ok(Self::tagged(src, pre)?.into_iter().map(|t| t.0).collect())
    }

    pub fn resolve(
        &self,
        src: &TemplateSource,
        git_ref: Option<&str>,
        pre: bool,
    ) -> Result<ResolvedTemplate> {
        let (version, commit) = match git_ref {
            None => Self::tagged(src, pre)?.pop().ok_or_else(|| {
                SpliceError::Template(format!(
                    "no `v<semver>` release tags found in template source `{}`",
                    src.spec
                ))
            })?,
            Some(r) => {
                let mirror = self.sync(src)?;
                let tag_ref = format!("refs/tags/{r}");
                let (commit, is_tag) = match Git::rev_parse(&mirror, &tag_ref) {
                    Ok(c) => (c, true),
                    Err(SpliceError::Template(_)) => (Git::rev_parse(&mirror, r)?, false),
                    Err(e) => return Err(e),
                };
                let tag_version = r
                    .strip_prefix('v')
                    .and_then(|v| Version::parse(v).ok())
                    .filter(|_| is_tag);
                let version = match tag_version {
                    Some(v) => v,
                    None => Version::parse(&format!("0.0.0+g{}", &commit[..7]))
                        .expect("valid build metadata"),
                };
                (version, commit)
            }
        };
        self.checkout(src, &commit, version)
    }

    pub fn resolve_version(
        &self,
        src: &TemplateSource,
        version: &Version,
    ) -> Result<ResolvedTemplate> {
        let commit = Self::tagged(src, true)?
            .into_iter()
            .find(|(v, _)| v == version)
            .map(|t| t.1)
            .ok_or_else(|| {
                SpliceError::Template(format!(
                    "tag `v{version}` not found in template source `{}`",
                    src.spec
                ))
            })?;
        self.checkout(src, &commit, version.clone())
    }

    /// Extract (or reuse) the tree for `commit`; no network when it is already cached.
    pub fn checkout(
        &self,
        src: &TemplateSource,
        commit: &str,
        version: Version,
    ) -> Result<ResolvedTemplate> {
        let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(hex) {
            return Err(SpliceError::Template(format!(
                "invalid template commit `{commit}`"
            )));
        }
        let tree = self.cache.join("trees").join(src.cache_key()).join(commit);
        if !tree.is_dir() {
            let mirror = self.sync(src)?;
            Git::extract_tree(&mirror, commit, &tree)?;
        }
        let cache_tree = tree.clone();
        let root = match &src.subdir {
            Some(s) => tree.join(s),
            None => tree,
        };
        if !Path::new(&root).is_dir() {
            return Err(SpliceError::Template(format!(
                "template path `{}` not found in `{}` at commit {commit}",
                src.subdir.as_deref().unwrap_or("."),
                src.spec
            )));
        }
        // A committed symlink (the subdir or one of its parents) must not lead out of the tree.
        let escapes = match (root.canonicalize(), cache_tree.canonicalize()) {
            (Ok(r), Ok(t)) => !r.starts_with(&t),
            _ => true,
        };
        if escapes {
            return Err(SpliceError::Template(format!(
                "template path `{}` in `{}` resolves through a symlink outside the template tree",
                src.subdir.as_deref().unwrap_or("."),
                src.spec
            )));
        }
        Ok(ResolvedTemplate {
            source: src.clone(),
            version,
            commit: commit.to_string(),
            root,
        })
    }
}
