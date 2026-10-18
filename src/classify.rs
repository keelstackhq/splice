//! Ownership classification (contract §5, §9.10).

use globset::{GlobBuilder, GlobMatcher};

use crate::error::SpliceError;
use crate::manifest::{MergeStrategy, PieceManifest};
use crate::output::Class;

type Result<T> = std::result::Result<T, SpliceError>;

struct Entry {
    matcher: GlobMatcher,
    text: String,
}

pub struct Classifier {
    owned: Vec<Entry>,
    shared: Vec<Entry>,
    seed: Vec<Entry>,
    merge: Vec<(GlobMatcher, MergeStrategy)>,
}

fn compile(globs: &[String]) -> Result<Vec<Entry>> {
    globs
        .iter()
        .map(|g| {
            Ok(Entry {
                matcher: compile_one(g)?,
                text: g.clone(),
            })
        })
        .collect()
}

fn compile_one(g: &str) -> Result<GlobMatcher> {
    GlobBuilder::new(g)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| SpliceError::Template(format!("glob `{g}`: {e}")))
}

impl Classifier {
    pub fn for_piece(p: &PieceManifest) -> Result<Self> {
        let merge: Vec<(&str, MergeStrategy)> =
            p.merge.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        Self::from_globs(&p.owned, &p.shared, &p.seed, &merge)
    }

    pub fn from_globs(
        owned: &[String],
        shared: &[String],
        seed: &[String],
        merge: &[(&str, MergeStrategy)],
    ) -> Result<Self> {
        Ok(Classifier {
            owned: compile(owned)?,
            shared: compile(shared)?,
            seed: compile(seed)?,
            merge: merge
                .iter()
                .map(|(k, s)| Ok((compile_one(k)?, *s)))
                .collect::<Result<_>>()?,
        })
    }

    /// Class, merge strategy and the matching glob for a path relative to `files/`, rendered and
    /// without `.jinja`: owned, then shared, then seed; no match is owned. Shared files without a
    /// `[merge]` match use `text`.
    pub fn classify(&self, rel: &str) -> (Class, MergeStrategy, Option<String>) {
        let find = |list: &[Entry]| {
            list.iter()
                .find(|e| e.matcher.is_match(rel))
                .map(|e| e.text.clone())
        };
        if let Some(g) = find(&self.owned) {
            return (Class::Owned, MergeStrategy::Text, Some(g));
        }
        if let Some(g) = find(&self.shared) {
            let strategy = self
                .merge
                .iter()
                .find(|(m, _)| m.is_match(rel))
                .map_or(MergeStrategy::Text, |(_, s)| *s);
            return (Class::Shared, strategy, Some(g));
        }
        if let Some(g) = find(&self.seed) {
            return (Class::Seed, MergeStrategy::Text, Some(g));
        }
        (Class::Owned, MergeStrategy::Text, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier(
        owned: &[&str],
        shared: &[&str],
        seed: &[&str],
        merge: &[(&str, MergeStrategy)],
    ) -> Classifier {
        let strs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Classifier::from_globs(&strs(owned), &strs(shared), &strs(seed), merge).unwrap()
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
    fn owned_beats_shared_beats_seed_and_glob_is_reported() {
        let c = classifier(
            &["a.txt"],
            &["a.txt", "b.txt"],
            &["a.txt", "b.txt", "c.txt"],
            &[],
        );
        assert_eq!(c.classify("a.txt").0, Class::Owned);
        assert_eq!(c.classify("b.txt").0, Class::Shared);
        assert_eq!(c.classify("c.txt").2.as_deref(), Some("c.txt"));
        assert_eq!(c.classify("b.txt").1, MergeStrategy::Text);
    }

    #[test]
    fn star_stays_in_one_segment() {
        let c = classifier(&[], &[], &["src/*.ts"], &[]);
        assert_eq!(c.classify("src/a.ts").0, Class::Seed);
        assert_eq!(c.classify("src/x/a.ts").0, Class::Owned);
    }
}
