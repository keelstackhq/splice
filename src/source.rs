use crate::error::SpliceError;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    Git { url: String },
    Local { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSource {
    /// The spec exactly as given (stored in the lock).
    pub spec: String,
    pub kind: SourceKind,
    pub subdir: Option<String>,
}

/// URL schemes spec §4 allows.
const ALLOWED_SCHEMES: [&str; 3] = ["https", "ssh", "file"];

fn has_scheme(s: &str) -> bool {
    matches!(s.split_once("://"), Some((scheme, _)) if !scheme.is_empty()
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)))
}

/// Split `<repo>//<subdir>`, ignoring the `//` that follows a URL scheme.
fn split_subdir(s: &str) -> (&str, Option<&str>) {
    let start = if has_scheme(s) {
        s.find("://").unwrap() + 3
    } else {
        0
    };
    match s[start..].find("//") {
        Some(i) => (&s[..start + i], Some(&s[start + i + 2..])),
        None => (s, None),
    }
}

fn check_subdir(spec: &str, sub: &str) -> Result<String, SpliceError> {
    let sub = sub.trim_matches('/');
    let bad = sub.is_empty()
        || sub.starts_with('-')
        || Path::new(sub)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || sub.contains('\\');
    if bad {
        return Err(SpliceError::Usage(format!(
            "invalid subdirectory in template source `{spec}`: must be a relative path without `..`"
        )));
    }
    Ok(sub.to_string())
}

impl TemplateSource {
    /// Parse a spec; a relative local path stays relative to the process cwd.
    pub fn parse(spec: &str) -> Result<Self, SpliceError> {
        Self::parse_from(spec, Path::new(""))
    }

    /// Parse a spec, resolving a relative local path against `base` (the repo root for a lock's
    /// source, the `-C` dir for `new`). `spec` stays as given.
    pub fn parse_from(spec: &str, base: &Path) -> Result<Self, SpliceError> {
        let mut s = Self::parse_raw(spec)?;
        if let SourceKind::Local { path } = &mut s.kind
            && path.is_relative()
        {
            *path = base.join(&*path);
        }
        Ok(s)
    }

    /// The spec to store in a lock written at `dest`: a relative local path is rewritten to be
    /// relative to `dest` (it resolves from the repo root); everything else is unchanged.
    pub fn spec_for_dest(&self, dest: &Path) -> String {
        let SourceKind::Local { path } = &self.kind else {
            return self.spec.clone();
        };
        if !Path::new(split_subdir(&self.spec).0).is_relative() {
            return self.spec.clone();
        }
        let (Some(src), Some(dst)) = (lenient_canonical(path), lenient_canonical(dest)) else {
            return self.spec.clone();
        };
        // no relative path (e.g. another drive): store the absolute canonical path
        let mut out = relative_to(&src, &dst).unwrap_or_else(|| src.to_string_lossy().into_owned());
        let is_rel = out == "." || out == ".." || out.starts_with("../");
        if Path::new(&out).is_relative() && !is_rel {
            out = format!("./{out}"); // never starts with `-`
        }
        if let Some(sub) = &self.subdir {
            out = format!("{out}//{sub}");
        }
        out
    }

    fn parse_raw(spec: &str) -> Result<Self, SpliceError> {
        if spec.starts_with('-') {
            return Err(SpliceError::Usage(format!(
                "invalid template source `{spec}`: must not start with `-`"
            )));
        }
        let (repo, sub) = split_subdir(spec);
        if repo.contains("::") {
            return Err(SpliceError::Usage(format!(
                "invalid template source `{spec}`: remote-helper (`::`) sources are not supported"
            )));
        }
        let subdir = sub.map(|s| check_subdir(spec, s)).transpose()?;
        let kind = if let Some(rest) = repo.strip_prefix("gh:") {
            let ok = matches!(rest.split_once('/'), Some((o, r))
                if !o.is_empty() && !r.is_empty() && !r.contains('/'));
            if !ok {
                return Err(SpliceError::Usage(format!(
                    "invalid template source `{spec}`: expected gh:<owner>/<repo>"
                )));
            }
            SourceKind::Git {
                url: format!("https://github.com/{rest}.git"),
            }
        } else if has_scheme(repo) {
            let scheme = repo.split_once("://").map_or("", |(s, _)| s);
            if !ALLOWED_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) {
                return Err(SpliceError::Usage(format!(
                    "invalid template source `{spec}`: unsupported scheme `{scheme}` \
                     (use https, ssh, file, git@host:path, gh:owner/repo or a local path)"
                )));
            }
            SourceKind::Git {
                url: repo.to_string(),
            }
        } else if is_scp_like(repo) {
            SourceKind::Git {
                url: repo.to_string(),
            }
        } else if repo.is_empty() {
            return Err(SpliceError::Usage("empty template source".into()));
        } else {
            SourceKind::Local {
                path: PathBuf::from(repo),
            }
        };
        Ok(Self {
            spec: spec.to_string(),
            kind,
            subdir,
        })
    }

    /// First 16 hex chars of sha256 of the URL or canonical local path.
    pub fn cache_key(&self) -> String {
        let id = match &self.kind {
            SourceKind::Git { url } => url.clone(),
            SourceKind::Local { path } => std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .into_owned(),
        };
        let digest = Sha256::digest(id.as_bytes());
        digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
    }
}

/// Absolute, `.`/`..` collapsed, symlinks resolved for the longest existing ancestor.
fn lenient_canonical(p: &Path) -> Option<PathBuf> {
    let abs = std::path::absolute(p).ok()?;
    let mut cur = PathBuf::new();
    let mut exists = true;
    for c in abs.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => cur.push(c),
            Component::CurDir => {}
            // `cur` is canonical while `exists`, so popping is what the filesystem does
            Component::ParentDir => {
                cur.pop();
            }
            Component::Normal(n) => {
                cur.push(n);
                if exists {
                    match cur.canonicalize() {
                        Ok(real) => cur = real,
                        Err(_) => exists = false,
                    }
                }
            }
        }
    }
    Some(strip_verbatim(cur))
}

/// Drop a Windows `\\?\` prefix (not `\\?\UNC\`) so paths compare and print normally.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    }
}

/// `target` relative to `from` with `/` separators; None across different roots.
fn relative_to(target: &Path, from: &Path) -> Option<String> {
    let t: Vec<_> = target.components().collect();
    let f: Vec<_> = from.components().collect();
    let common = t.iter().zip(&f).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".into(); f.len() - common];
    for c in &t[common..] {
        parts.push(c.as_os_str().to_str()?.to_string());
    }
    Some(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
}

/// `git@host:path` style (user@host:path); a Windows drive like `C:\x` is not.
fn is_scp_like(s: &str) -> bool {
    match s.split_once(':') {
        Some((host, path)) => host.contains('@') && !host.contains('/') && !path.is_empty(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sources() {
        let s = TemplateSource::parse("gh:keelstack/hull//template").unwrap();
        assert_eq!(
            s.kind,
            SourceKind::Git {
                url: "https://github.com/keelstack/hull.git".into()
            }
        );
        assert_eq!(s.subdir.as_deref(), Some("template"));
        let s = TemplateSource::parse("https://example.com/x/y.git//a/b").unwrap();
        assert_eq!(
            (s.kind, s.subdir.as_deref()),
            (
                SourceKind::Git {
                    url: "https://example.com/x/y.git".into()
                },
                Some("a/b")
            )
        );
        assert!(matches!(
            TemplateSource::parse("./tpl").unwrap().kind,
            SourceKind::Local { .. }
        ));
        assert_eq!(
            TemplateSource::parse("../t//sub")
                .unwrap()
                .subdir
                .as_deref(),
            Some("sub")
        );
        assert!(TemplateSource::parse("gh:onlyowner").is_err());
        assert!(TemplateSource::parse("gh:a/b//../x").is_err());
        let s = TemplateSource::parse("git@host:p//s").unwrap();
        assert_eq!(
            (s.kind, s.subdir.as_deref()),
            (
                SourceKind::Git {
                    url: "git@host:p".into()
                },
                Some("s")
            )
        );
        let s = TemplateSource::parse("ssh://h/r.git//s").unwrap();
        assert_eq!(
            (s.kind, s.subdir.as_deref()),
            (
                SourceKind::Git {
                    url: "ssh://h/r.git".into()
                },
                Some("s")
            )
        );
        let s = TemplateSource::parse("file:///abs//s").unwrap();
        assert_eq!(
            (s.kind, s.subdir.as_deref()),
            (
                SourceKind::Git {
                    url: "file:///abs".into()
                },
                Some("s")
            )
        );
    }

    #[test]
    fn rejects_unlisted_schemes() {
        for bad in [
            "foo://x",
            "ftp://h/r.git",
            "http://h/r.git",
            "git://h/r.git",
        ] {
            let e = TemplateSource::parse(bad).unwrap_err();
            assert_eq!(e.exit_code(), 2, "{bad}");
        }
        assert!(TemplateSource::parse("https://h/r.git").is_ok());
    }

    #[test]
    fn rejects_option_like_and_helper_specs() {
        for bad in [
            "--upload-pack=x",
            "-oProxyCommand=x",
            "-x//s",
            "ext::sh -c id",
            "a@b::c",
            "./t//-s",
            "gh:a/b//-s",
        ] {
            assert!(TemplateSource::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rewritten_relative_source_never_starts_with_dash() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("-dash")).unwrap();
        let s = TemplateSource::parse_from("sub/../-dash", d.path()).unwrap();
        assert_eq!(s.spec_for_dest(d.path()), "./-dash");
    }

    #[cfg(unix)]
    #[test]
    fn lenient_canonical_follows_symlinks_before_dotdot() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("real/deep")).unwrap();
        std::fs::create_dir_all(root.join("real/tpl")).unwrap();
        std::os::unix::fs::symlink(root.join("real/deep"), root.join("link")).unwrap();
        // link/../tpl is real/tpl on the filesystem, not <root>/tpl
        assert_eq!(
            lenient_canonical(&root.join("link/../tpl")).unwrap(),
            root.join("real/tpl")
        );
        // a missing tail is applied lexically
        assert_eq!(
            lenient_canonical(&root.join("link/../nope/x")).unwrap(),
            root.join("real/nope/x")
        );
    }

    #[test]
    fn strips_verbatim_prefix() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\x\y")),
            PathBuf::from(r"C:\x\y")
        );
        assert_eq!(strip_verbatim(PathBuf::from("/a/b")), PathBuf::from("/a/b"));
    }
}
