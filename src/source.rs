//! Template source specs: `gh:owner/repo//subdir`, git URLs and local paths.

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
    /// The spec exactly as the user wrote it.
    pub spec: String,
    pub kind: SourceKind,
    /// Template root inside the repo, from `//subdir`.
    pub subdir: Option<String>,
}

fn bad(spec: &str, why: &str) -> SpliceError {
    SpliceError::Usage(format!("invalid template source `{spec}`: {why}"))
}

/// A scheme URL that carries a secret: any userinfo, except a bare user on an SSH scheme
/// (`ssh://git@host`; an encoded or plain `:` still counts as a password), or a secret-looking
/// query parameter. Returns the URL with the secret parts masked, never the raw text.
fn masked_secret(url: &str) -> Option<String> {
    let n = scheme_len(url)?;
    let (scheme, rest) = (&url[..n - 3], &url[n..]);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (userinfo, host) = match authority.split_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let ssh = scheme.to_ascii_lowercase().contains("ssh");
    let user_secret = userinfo.is_some_and(|u| {
        !u.is_empty() && (!ssh || u.contains(':') || u.to_ascii_lowercase().contains("%3a"))
    });
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (tail, None),
    };
    let secret_param = |kv: &str| {
        let key = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
        ["key", "pat", "auth"].contains(&key.as_str())
            || [
                "token",
                "secret",
                "passw",
                "apikey",
                "api_key",
                "access_key",
            ]
            .iter()
            .any(|w| key.contains(w))
    };
    let query_secret = query.is_some_and(|q| q.split(['&', '#']).any(secret_param));
    if !user_secret && !query_secret {
        return None;
    }
    let who = match userinfo {
        Some(_) if user_secret => "***@".to_string(),
        Some(u) => format!("{u}@"),
        None => String::new(),
    };
    let q = query.map(|q| {
        let masked: Vec<String> = q
            .split('&')
            .map(|kv| match kv.split_once('=') {
                Some((k, _)) if secret_param(kv) => format!("{k}=***"),
                _ => kv.to_string(),
            })
            .collect();
        format!("?{}", masked.join("&"))
    });
    Some(format!(
        "{scheme}://{who}{host}{path}{}",
        q.unwrap_or_default()
    ))
}

/// Length of a leading `scheme://`, if the spec has one.
fn scheme_len(s: &str) -> Option<usize> {
    let i = s.find("://")?;
    let scheme = &s[..i];
    let mut chars = scheme.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    ok.then_some(i + 3)
}

/// `user@host:path` (scp-like git address), not a path and not a URL.
fn is_scp_like(s: &str) -> bool {
    match (s.find('@'), s.find(':'), s.find('/')) {
        (Some(at), Some(colon), slash) => at < colon && slash.is_none_or(|sl| colon < sl),
        _ => false,
    }
}

fn check_subdir(spec: &str, sub: &str) -> Result<Option<String>, SpliceError> {
    let sub = sub.trim_end_matches('/');
    if sub.is_empty() {
        return Ok(None);
    }
    let p = Path::new(sub);
    let safe = p.is_relative() && p.components().all(|c| matches!(c, Component::Normal(_)));
    if !safe || sub.contains('\\') {
        return Err(bad(spec, "subdir must be a relative path without `..`"));
    }
    Ok(Some(sub.to_string()))
}

impl TemplateSource {
    /// Parse a spec; a relative local path stays relative to the process cwd.
    pub fn parse(spec: &str) -> Result<Self, SpliceError> {
        Self::parse_raw(spec)
    }

    /// Parse a spec read from a lock: a relative local path resolves against `base` (the repo
    /// root, the `-C` dir), never the process cwd, and becomes absolute. `spec` stays as written.
    pub fn parse_from(spec: &str, base: &Path) -> Result<Self, SpliceError> {
        let mut s = Self::parse_raw(spec)?;
        if let SourceKind::Local { path } = &mut s.kind
            && path.is_relative()
        {
            // Absolute, so git calls made from other directories resolve it too.
            let joined = base.join(&*path);
            *path = lenient_canonical(&joined).unwrap_or(joined);
        }
        Ok(s)
    }

    fn parse_raw(spec: &str) -> Result<Self, SpliceError> {
        // Split `//subdir` on the first `//` after the scheme's own `//`.
        let skip = scheme_len(spec).unwrap_or(0);
        // Before anything can echo the spec: a URL with a secret is never recorded or shown.
        let head = &spec[..spec[skip..].find("//").map_or(spec.len(), |i| skip + i)];
        if let Some(shown) = masked_secret(head) {
            return Err(bad(
                &shown,
                "the URL embeds a credential, which would be written to the lock and git \
                 traces; use a git credential helper or an ssh:// or git@host: URL",
            ));
        }
        let (main, subdir) = match spec[skip..].find("//") {
            Some(i) => {
                let at = skip + i;
                (&spec[..at], check_subdir(spec, &spec[at + 2..])?)
            }
            None => (spec, None),
        };
        let kind = if let Some(rest) = main.strip_prefix("gh:") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() != 2 || parts.iter().any(|p| p.is_empty()) {
                return Err(bad(spec, "expected gh:owner/repo"));
            }
            let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
            SourceKind::Git {
                url: format!("https://github.com/{}/{repo}.git", parts[0]),
            }
        } else if scheme_len(main).is_some() || is_scp_like(main) {
            SourceKind::Git {
                url: main.to_string(),
            }
        } else if main.is_empty() {
            return Err(bad(spec, "empty source"));
        } else {
            SourceKind::Local {
                path: PathBuf::from(main),
            }
        };
        Ok(TemplateSource {
            spec: spec.to_string(),
            kind,
            subdir,
        })
    }

    /// The spec the lock stores: as given, except that a local path is made absolute (resolved
    /// against the current directory) so `add`, `update` and `explain` work from any directory.
    pub fn lock_spec(&self) -> String {
        let SourceKind::Local { path } = &self.kind else {
            return self.spec.clone();
        };
        let abs = lenient_canonical(path).unwrap_or_else(|| path.clone());
        match &self.subdir {
            Some(sub) => format!("{}//{sub}", abs.display()),
            None => abs.display().to_string(),
        }
    }

    /// A local source written as a relative path.
    pub fn is_relative_local(&self) -> bool {
        matches!(&self.kind, SourceKind::Local { path } if path.is_relative())
    }

    /// First 16 hex digits of sha256 of the URL (git) or canonical path (local).
    pub fn cache_key(&self) -> String {
        let id = match &self.kind {
            SourceKind::Git { url } => url.clone(),
            // Lenient: the key must not change when the source is later deleted or renamed.
            SourceKind::Local { path } => lenient_canonical(path)
                .unwrap_or_else(|| path.clone())
                .to_string_lossy()
                .into_owned(),
        };
        key16(&id)
    }
}

/// Absolute, `.`/`..` collapsed, symlinks resolved for the longest existing ancestor (symlinks
/// are followed before `..`, as the filesystem does). Never fails on a missing path.
fn lenient_canonical(p: &Path) -> Option<PathBuf> {
    let abs = std::path::absolute(p).ok()?;
    let mut cur = PathBuf::new();
    let mut exists = true;
    for c in abs.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => cur.push(c),
            Component::CurDir => {}
            // `cur` is canonical while `exists`, so popping is what the filesystem does.
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

/// First 16 hex digits of sha256 of `id`.
pub fn key16(id: &str) -> String {
    Sha256::digest(id.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_with_a_credential_is_refused_and_masked() {
        for (bad, shown) in [
            (
                "https://user:tok@example.com/o/r.git",
                "https://***@example.com/o/r.git",
            ),
            (
                "https://ghp_SECRET@github.com/o/r.git//t",
                "https://***@github.com/o/r.git",
            ),
            (
                "https://user%3Atok@example.com/o/r.git",
                "https://***@example.com/o/r.git",
            ),
            ("ssh://git:tok@host/o/r.git", "ssh://***@host/o/r.git"),
            (
                "https://example.com/o/r.git?access_token=abc",
                "https://example.com/o/r.git?access_token=***",
            ),
            (
                "https://h/r.git?a=1&token=abc//sub@x",
                "https://h/r.git?a=1&token=***",
            ),
        ] {
            let e = TemplateSource::parse(bad).unwrap_err().to_string();
            assert!(e.contains(shown) && e.contains("credential helper"), "{e}");
            for leak in ["tok@", "SECRET", "Atok", "=abc"] {
                assert!(!e.contains(leak), "{leak} leaked: {e}");
            }
        }
        for ok in [
            "ssh://git@host/o/r.git",
            "ssh://git@host:22/o/r.git//t",
            "git@host:o/r.git",
            "https://example.com/a:b/r.git",
            "https://example.com/r.git?path=x",
            "https://example.com/r.git//dir@x",
        ] {
            TemplateSource::parse(ok).unwrap();
        }
    }

    #[test]
    fn parses_sources() {
        let s = TemplateSource::parse("gh:keelstackhq/hull//template").unwrap();
        assert_eq!(
            s.kind,
            SourceKind::Git {
                url: "https://github.com/keelstackhq/hull.git".into()
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
        assert!(TemplateSource::parse("gh:a/b//../x").is_err()); // subdir must not escape
    }

    #[test]
    fn git_forms_and_errors() {
        for (spec, url) in [
            ("git@github.com:o/r.git", "git@github.com:o/r.git"),
            ("ssh://git@host/o/r.git", "ssh://git@host/o/r.git"),
            ("file:///tmp/r", "file:///tmp/r"),
        ] {
            let s = TemplateSource::parse(spec).unwrap();
            assert_eq!(s.kind, SourceKind::Git { url: url.into() }, "{spec}");
            assert_eq!(s.subdir, None);
        }
        let s = TemplateSource::parse("file:///tmp/r//t").unwrap();
        assert_eq!(
            s.kind,
            SourceKind::Git {
                url: "file:///tmp/r".into()
            }
        );
        assert_eq!(s.subdir.as_deref(), Some("t"));
        assert_eq!(
            TemplateSource::parse("git@h:o/r.git//t")
                .unwrap()
                .subdir
                .as_deref(),
            Some("t")
        );
        assert_eq!(
            TemplateSource::parse("gh:a/b//t/")
                .unwrap()
                .subdir
                .as_deref(),
            Some("t")
        );
        assert!(TemplateSource::parse("gh:a/b//").unwrap().subdir.is_none());
        assert!(TemplateSource::parse("gh:a/b//./x").is_err());
        assert!(TemplateSource::parse("gh:a/b///abs").is_err());
        assert!(TemplateSource::parse("").is_err());
        assert_eq!(TemplateSource::parse("gh:a/b").unwrap().spec, "gh:a/b");
    }

    #[test]
    fn cache_key_is_16_hex_and_stable() {
        let a = TemplateSource::parse("gh:a/b").unwrap();
        let k = a.cache_key();
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(k, TemplateSource::parse("gh:a/b//sub").unwrap().cache_key());
        assert_ne!(k, TemplateSource::parse("gh:a/c").unwrap().cache_key());
        assert_eq!(key16("x"), key16("x"));
    }

    #[test]
    fn lenient_canonical_follows_symlinks_before_dotdot() {
        let d = tempfile::tempdir().unwrap();
        // lenient_canonical drops Windows' `\\?\` prefix; canonicalize keeps it.
        let root = strip_verbatim(d.path().canonicalize().unwrap());
        std::fs::create_dir_all(root.join("a/deep")).unwrap();
        std::fs::create_dir(root.join("tpl")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("a/deep"), root.join("link")).unwrap();
            // `link/..` is `a`, not the root.
            assert_eq!(
                lenient_canonical(&root.join("link/../x")).unwrap(),
                root.join("a/x")
            );
        }
        assert_eq!(
            lenient_canonical(&root.join("a/../tpl/./")).unwrap(),
            root.join("tpl")
        );
        assert_eq!(
            lenient_canonical(&root.join("nope/y/../z")).unwrap(),
            root.join("nope/z")
        );
    }

    #[test]
    fn local_cache_key_is_stable_after_deletion_and_for_equivalent_paths() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("a/tpl")).unwrap();
        let key = |p: &Path| {
            TemplateSource::parse(&p.to_string_lossy())
                .unwrap()
                .cache_key()
        };
        let before = key(&root.join("a/tpl"));
        assert_eq!(key(&root.join("a/../a/./tpl")), before);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("a"), root.join("link")).unwrap();
            assert_eq!(key(&root.join("link/tpl")), before);
        }
        std::fs::remove_dir(root.join("a/tpl")).unwrap();
        assert_eq!(key(&root.join("a/tpl")), before);
        #[cfg(unix)]
        assert_eq!(key(&root.join("link/tpl")), before);
        assert_eq!(key(&root.join("a/x/../tpl")), before);
    }

    #[test]
    fn strips_verbatim_prefix() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\x\y")),
            PathBuf::from(r"C:\x\y")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\h\s")),
            PathBuf::from(r"\\?\UNC\h\s")
        );
        assert_eq!(strip_verbatim(PathBuf::from("/a/b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn parse_from_resolves_relative_local_against_base_only() {
        // A drive-less `/r` is relative on Windows; make every path absolute for this OS.
        let abs = |p: &str| std::path::absolute(p).unwrap();
        let s = TemplateSource::parse_from("../tpl//sub", &abs("/r/app")).unwrap();
        assert_eq!(
            s.kind,
            SourceKind::Local {
                path: abs("/r/tpl")
            }
        );
        assert_eq!(s.spec, "../tpl//sub");
        assert_eq!(s.subdir.as_deref(), Some("sub"));
        let a = abs("/abs/tpl");
        let s = TemplateSource::parse_from(a.to_str().unwrap(), &abs("/r/app")).unwrap();
        assert_eq!(s.kind, SourceKind::Local { path: a });
        let g = TemplateSource::parse_from("gh:a/b", Path::new("/r")).unwrap();
        assert!(matches!(g.kind, SourceKind::Git { .. }));
    }
}
