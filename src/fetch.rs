//! Resolve a template source to a version and a cached tree, keyed by commit.
//!
//! Layout under the cache root: `git/<key>.git` (mirrors of remote sources) and
//! `trees/<key>/<commit>/` (extracted trees, no `.git`). A tree is immutable once present, so
//! a cached commit needs no network and no source repository.

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
    /// The extracted tree plus the source's subdir; contains `splice.toml`.
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Fetcher {
    pub cache: PathBuf,
}

/// The semver of a `v<semver>` tag, if it is one.
fn tag_version(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v')?).ok()
}

impl Fetcher {
    /// `$SPLICE_CACHE_DIR`, else the platform cache directory plus `splice`.
    pub fn from_env() -> Self {
        let cache = match std::env::var_os("SPLICE_CACHE_DIR") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("splice"),
        };
        Fetcher { cache }
    }

    pub fn new(cache: PathBuf) -> Self {
        Fetcher { cache }
    }

    /// The git directory to read from, and where to list tags. Local sources are used in
    /// place; remote ones are mirrored (created or updated) first.
    fn repo(&self, src: &TemplateSource) -> Result<(PathBuf, String)> {
        match &src.kind {
            SourceKind::Local { path } => {
                let dot_git = path.join(".git");
                let git_dir = if dot_git.exists() {
                    dot_git
                } else if path.join("HEAD").is_file() && path.join("objects").is_dir() {
                    path.clone()
                } else {
                    return Err(SpliceError::Template(format!(
                        "template source `{}` is not a git repository; point splice at a git \
                         repository or use a `gh:owner/repo` source",
                        path.display()
                    )));
                };
                Ok((git_dir, path.to_string_lossy().into_owned()))
            }
            SourceKind::Git { url } => {
                let mirror = self
                    .cache
                    .join("git")
                    .join(format!("{}.git", src.cache_key()));
                Git::ensure_mirror(url, &mirror)?;
                let tags_from = mirror.to_string_lossy().into_owned();
                Ok((mirror, tags_from))
            }
        }
    }

    /// `(tag, version, commit)` for every `v<semver>` tag, prereleases only when `pre`.
    fn version_tags(&self, tags_from: &str, pre: bool) -> Result<Vec<(String, Version, String)>> {
        let mut out: Vec<_> = Git::ls_remote_tags(tags_from)?
            .into_iter()
            .filter_map(|(tag, sha)| {
                let v = tag_version(&tag)?;
                (pre || v.pre.is_empty()).then_some((tag, v, sha))
            })
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(out)
    }

    /// Every released version, ascending. Prereleases only when `pre`.
    pub fn available_versions(&self, src: &TemplateSource, pre: bool) -> Result<Vec<Version>> {
        let (_, tags_from) = self.repo(src)?;
        let mut versions: Vec<Version> = self
            .version_tags(&tags_from, pre)?
            .into_iter()
            .map(|t| t.1)
            .collect();
        versions.dedup();
        Ok(versions)
    }

    /// The latest stable version, or `git_ref`: a `v<semver>` tag gives its version, anything
    /// else `0.0.0+g<sha7>`.
    pub fn resolve(
        &self,
        src: &TemplateSource,
        git_ref: Option<&str>,
        pre: bool,
    ) -> Result<ResolvedTemplate> {
        let (git_dir, tags_from) = self.repo(src)?;
        if let Some(r) = git_ref {
            let commit = Git::rev_parse(&git_dir, r)?;
            let version = tag_version(r).unwrap_or_else(|| {
                Version::parse(&format!("0.0.0+g{}", &commit[..7])).expect("valid semver")
            });
            return self.checkout_from(src, &git_dir, &commit, version);
        }
        let tags = self.version_tags(&tags_from, pre)?;
        let Some((_, version, commit)) = tags.into_iter().next_back() else {
            return Err(SpliceError::Template(format!(
                "template `{}` has no version tags (`v<major>.<minor>.<patch>`){}; tag a \
                 release or pass --ref",
                src.spec,
                if pre {
                    ""
                } else {
                    " (try --pre for prereleases)"
                }
            )));
        };
        self.checkout_from(src, &git_dir, &commit, version)
    }

    /// The template at tag `v<version>`.
    pub fn resolve_version(
        &self,
        src: &TemplateSource,
        version: &Version,
    ) -> Result<ResolvedTemplate> {
        let (git_dir, _) = self.repo(src)?;
        let tag = format!("v{version}");
        let commit = Git::rev_parse(&git_dir, &format!("refs/tags/{tag}")).map_err(|_| {
            SpliceError::Template(format!("template `{}` has no tag `{tag}`", src.spec))
        })?;
        self.checkout_from(src, &git_dir, &commit, version.clone())
    }

    /// The tree of `commit`. A cached tree is served without touching the source or network.
    pub fn checkout(
        &self,
        src: &TemplateSource,
        commit: &str,
        version: Version,
    ) -> Result<ResolvedTemplate> {
        // The commit becomes a cache path segment: never accept anything but a full sha.
        validate_commit(commit, crate::lock::LOCK_PATH)?;
        if let Some(found) = self.cached(src, commit, &version)? {
            return Ok(found);
        }
        let (git_dir, _) = self.repo(src)?;
        self.checkout_from(src, &git_dir, commit, version)
    }

    fn tree_dir(&self, src: &TemplateSource, commit: &str) -> PathBuf {
        self.cache.join("trees").join(src.cache_key()).join(commit)
    }

    fn cached(
        &self,
        src: &TemplateSource,
        commit: &str,
        version: &Version,
    ) -> Result<Option<ResolvedTemplate>> {
        let tree = self.tree_dir(src, commit);
        if tree.is_dir() {
            return template_at(src, commit, version, &tree).map(Some);
        }
        Ok(None)
    }

    fn checkout_from(
        &self,
        src: &TemplateSource,
        git_dir: &Path,
        commit: &str,
        version: Version,
    ) -> Result<ResolvedTemplate> {
        if let Some(found) = self.cached(src, commit, &version)? {
            return Ok(found);
        }
        let commit = Git::rev_parse(git_dir, commit)?;
        let tree = self.tree_dir(src, &commit);
        if let Err(e) = Git::extract_tree(git_dir, &commit, &tree) {
            // Another process may have won the rename; a present tree is a hit.
            if !tree.is_dir() {
                return Err(e);
            }
        }
        template_at(src, &commit, &version, &tree)
    }
}

/// A commit taken from a lock must be a full lowercase sha: it is used as a cache path segment.
pub fn validate_commit(commit: &str, lock_path: &str) -> Result<()> {
    if commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(SpliceError::Template(format!(
            "invalid commit `{commit}`: expected 40 lowercase hex characters; \
             fix `commit` in {lock_path}"
        )))
    }
}

fn template_at(
    src: &TemplateSource,
    commit: &str,
    version: &Version,
    tree: &Path,
) -> Result<ResolvedTemplate> {
    let root = match &src.subdir {
        Some(sub) => tree.join(sub),
        None => tree.to_path_buf(),
    };
    if !root.join("splice.toml").is_file() {
        let where_ = src.subdir.as_deref().unwrap_or(".");
        return Err(SpliceError::Template(format!(
            "no splice.toml at `{where_}` in template `{}` (commit {}); check the \
             `//subdir` of the source",
            src.spec,
            &commit[..commit.len().min(7)]
        )));
    }
    Ok(ResolvedTemplate {
        source: src.clone(),
        version: version.clone(),
        commit: commit.to_string(),
        root,
    })
}
