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
    pub fn parse(spec: &str) -> Result<Self, SpliceError> {
        // Split `//subdir` on the first `//` after the scheme's own `//`.
        let skip = scheme_len(spec).unwrap_or(0);
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
        let abs = path.canonicalize().unwrap_or_else(|_| path.clone());
        match &self.subdir {
            Some(sub) => format!("{}//{sub}", abs.display()),
            None => abs.display().to_string(),
        }
    }

    /// First 16 hex digits of sha256 of the URL (git) or canonical path (local).
    pub fn cache_key(&self) -> String {
        let id = match &self.kind {
            SourceKind::Git { url } => url.clone(),
            SourceKind::Local { path } => path
                .canonicalize()
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .into_owned(),
        };
        key16(&id)
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
}
