//! `--only` / `--exclude` path filters, also stored in a channel lock's `[channel]` table.

use globset::{GlobBuilder, GlobMatcher};

use crate::error::SpliceError;
use crate::lock::ChannelMeta;
use crate::render::RenderedTree;

type Result<T> = std::result::Result<T, SpliceError>;

/// Keeps a path when it matches `only` (or `only` is empty) and matches no `exclude` glob.
/// `*` does not cross `/`.
#[derive(Debug, Clone)]
pub struct Filter {
    only: Vec<(String, GlobMatcher)>,
    exclude: Vec<(String, GlobMatcher)>,
}

fn compile(globs: &[String]) -> std::result::Result<Vec<(String, GlobMatcher)>, (String, String)> {
    globs
        .iter()
        .map(|g| {
            GlobBuilder::new(g)
                .literal_separator(true)
                .build()
                .map(|b| (g.clone(), b.compile_matcher()))
                .map_err(|e| (g.clone(), e.to_string()))
        })
        .collect()
}

impl Filter {
    /// A bad glob is a usage error (exit 2) naming the flag.
    pub fn new(only: &[String], exclude: &[String]) -> Result<Filter> {
        let usage = |flag: &str, (g, e): (String, String)| {
            SpliceError::Usage(format!("{flag} glob `{g}`: {e}"))
        };
        Ok(Filter {
            only: compile(only).map_err(|e| usage("--only", e))?,
            exclude: compile(exclude).map_err(|e| usage("--exclude", e))?,
        })
    }

    /// A bad stored glob means a corrupt lock (exit 3) naming the lock file.
    pub fn from_meta(m: &ChannelMeta, lock_path: &str) -> Result<Filter> {
        let corrupt = |key: &str, (g, e): (String, String)| {
            SpliceError::Template(format!(
                "{lock_path}: `{key}` glob `{g}`: {e}; restore it from git or re-run \
                 `splice channel add`"
            ))
        };
        Ok(Filter {
            only: compile(&m.only).map_err(|e| corrupt("only", e))?,
            exclude: compile(&m.exclude).map_err(|e| corrupt("exclude", e))?,
        })
    }

    pub fn keeps(&self, path: &str) -> bool {
        (self.only.is_empty() || self.only.iter().any(|(_, m)| m.is_match(path)))
            && !self.exclude.iter().any(|(_, m)| m.is_match(path))
    }

    pub fn apply_tree(&self, tree: RenderedTree) -> RenderedTree {
        tree.into_iter().filter(|(p, _)| self.keeps(p)).collect()
    }

    /// One message per glob that matches none of `paths`.
    pub fn unused(&self, paths: &[&str]) -> Vec<String> {
        self.only
            .iter()
            .chain(&self.exclude)
            .filter(|(_, m)| !paths.iter().any(|p| m.is_match(p)))
            .map(|(g, _)| format!("filter `{g}` matches nothing"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn empty_filter_keeps_everything() {
        assert!(Filter::new(&[], &[]).unwrap().keeps("a/b.md"));
    }

    #[test]
    fn only_then_exclude() {
        let f = Filter::new(&s(&[".claude/**"]), &s(&[".claude/skills/caveman/**"])).unwrap();
        assert!(f.keeps(".claude/agents/x.md"));
        assert!(!f.keeps(".claude/skills/caveman/SKILL.md"));
        assert!(!f.keeps("AGENTS.md"));
    }

    #[test]
    fn star_does_not_cross_slash() {
        let f = Filter::new(&[], &s(&[".claude/*"])).unwrap();
        assert!(!f.keeps(".claude/a.md"));
        assert!(f.keeps(".claude/agents/a.md"));
    }

    #[test]
    fn bad_glob_is_usage() {
        assert_eq!(Filter::new(&s(&["a/[b"]), &[]).unwrap_err().exit_code(), 2);
        let e = Filter::new(&[], &s(&["a/[b"])).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("--exclude"), "{e}");
    }

    #[test]
    fn bad_stored_glob_is_corrupt_lock() {
        let m = ChannelMeta {
            name: "rig".into(),
            exclude: s(&["a/[b"]),
            ..Default::default()
        };
        let e = Filter::from_meta(&m, ".splice/channels/rig.toml").unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains(".splice/channels/rig.toml"), "{e}");
    }

    #[test]
    fn unused_globs_reported() {
        let f = Filter::new(&[], &s(&["nope/**", "a.md"])).unwrap();
        assert_eq!(
            f.unused(&["a.md"]),
            vec!["filter `nope/**` matches nothing".to_string()]
        );
    }
}
