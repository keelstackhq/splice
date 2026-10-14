//! Ownership classification by glob (spec §7, contract §5).

use crate::error::SpliceError;
use crate::manifest::{MergeStrategy, PieceManifest};
use crate::output::Class;
use globset::{Glob, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};

pub struct Classifier {
    owned: GlobSet,
    shared: GlobSet,
    seed: GlobSet,
    merge: Vec<(GlobMatcher, MergeStrategy)>,
    globs: Vec<Vec<String>>,
}

impl Classifier {
    pub fn for_piece(p: &PieceManifest) -> Result<Self, SpliceError> {
        let glob = |g: &str| -> Result<Glob, SpliceError> {
            GlobBuilder::new(g)
                .literal_separator(true)
                .build()
                .map_err(|e| {
                    SpliceError::Template(format!(
                        "pieces/{}/piece.toml: invalid glob {g:?}: {e}",
                        p.id
                    ))
                })
        };
        let set = |globs: &[String]| -> Result<GlobSet, SpliceError> {
            let mut b = GlobSetBuilder::new();
            for g in globs {
                b.add(glob(g)?);
            }
            b.build()
                .map_err(|e| SpliceError::Template(format!("pieces/{}/piece.toml: {e}", p.id)))
        };
        let mut merge = Vec::new();
        for (k, v) in &p.merge {
            merge.push((glob(k)?.compile_matcher(), *v));
        }
        Ok(Self {
            owned: set(&p.owned)?,
            shared: set(&p.shared)?,
            seed: set(&p.seed)?,
            merge,
            globs: vec![p.owned.clone(), p.shared.clone(), p.seed.clone()],
        })
    }

    /// Class, merge strategy (only meaningful for `shared`; `Text` otherwise) and the glob that
    /// matched (`None` for the default `owned`).
    pub fn classify(&self, rel: &str) -> (Class, MergeStrategy, Option<String>) {
        let sets = [
            (Class::Owned, &self.owned),
            (Class::Shared, &self.shared),
            (Class::Seed, &self.seed),
        ];
        for (i, (class, set)) in sets.into_iter().enumerate() {
            if let Some(&idx) = set.matches(rel).first() {
                let strategy = if class == Class::Shared {
                    self.merge
                        .iter()
                        .find(|(m, _)| m.is_match(rel))
                        .map_or(MergeStrategy::Text, |(_, s)| *s)
                } else {
                    MergeStrategy::Text
                };
                return (class, strategy, Some(self.globs[i][idx].clone()));
            }
        }
        (Class::Owned, MergeStrategy::Text, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::path::PathBuf;

    fn classifier(
        owned: &[&str],
        shared: &[&str],
        seed: &[&str],
        merge: &[(&str, MergeStrategy)],
    ) -> Classifier {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let mut m = IndexMap::new();
        for (k, v) in merge {
            m.insert(k.to_string(), *v);
        }
        Classifier::for_piece(&PieceManifest {
            id: "p".into(),
            description: String::new(),
            multiple: false,
            requires: vec![],
            target: None,
            vars: IndexMap::new(),
            owned: s(owned),
            shared: s(shared),
            seed: s(seed),
            merge: m,
            post: vec![],
            files_dir: PathBuf::new(),
        })
        .unwrap()
    }

    #[test]
    fn first_match_owned_shared_seed_default_owned() {
        let c = classifier(
            &["vite.config.ts"],
            &["package.json"],
            &["src/**"],
            &[("package.json", MergeStrategy::Json)],
        );
        assert_eq!(c.classify("package.json").0, Class::Shared);
        assert_eq!(c.classify("package.json").1, MergeStrategy::Json);
        assert_eq!(c.classify("src/a/b.ts").0, Class::Seed);
        assert_eq!(
            c.classify("other.md"),
            (Class::Owned, MergeStrategy::Text, None)
        );
        assert_eq!(c.classify("x/package.json").0, Class::Owned);
    }

    #[test]
    fn owned_beats_shared_beats_seed_and_reports_glob() {
        let c = classifier(&["a.txt"], &["a.txt", "b.txt"], &["*.txt"], &[]);
        assert_eq!(c.classify("a.txt").0, Class::Owned);
        assert_eq!(
            c.classify("b.txt"),
            (Class::Shared, MergeStrategy::Text, Some("b.txt".into()))
        );
        assert_eq!(c.classify("c.txt").2.as_deref(), Some("*.txt"));
        assert_eq!(c.classify("d/c.txt").0, Class::Owned); // `*` stays in one segment
    }

    #[test]
    fn bad_glob_is_template_error() {
        let e = Classifier::for_piece(&PieceManifest {
            id: "p".into(),
            description: String::new(),
            multiple: false,
            requires: vec![],
            target: None,
            vars: IndexMap::new(),
            owned: vec!["[".into()],
            shared: vec![],
            seed: vec![],
            merge: IndexMap::new(),
            post: vec![],
            files_dir: PathBuf::new(),
        })
        .err()
        .unwrap();
        assert_eq!(e.exit_code(), 3);
    }
}
