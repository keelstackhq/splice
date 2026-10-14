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
    pub fn parse(spec: &str) -> Result<Self, SpliceError> {
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
        } else if has_scheme(repo) || is_scp_like(repo) {
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
}
