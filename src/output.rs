//! The `Report` printed by every command: the `--json` object or terse human lines.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Owned,
    Shared,
    Seed,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Owned => "owned",
            Class::Shared => "shared",
            Class::Seed => "seed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Merge,
    Conflict,
    Delete,
    Skip,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Create => "create",
            Action::Update => "update",
            Action::Merge => "merge",
            Action::Conflict => "conflict",
            Action::Delete => "delete",
            Action::Skip => "skip",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    pub action: Action,
    pub class: Class,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub ok: bool,
    pub command: String,
    pub changes: Vec<Change>,
    pub messages: Vec<String>,
    /// Human-mode stdout lines that replace the default change lines (not in `--json`).
    #[serde(skip)]
    pub lines: Vec<String>,
}

impl Report {
    pub fn new(command: &str) -> Self {
        Report {
            ok: true,
            command: command.to_string(),
            changes: Vec::new(),
            messages: Vec::new(),
            lines: Vec::new(),
        }
    }

    pub fn push(&mut self, path: impl Into<String>, action: Action, class: Class) {
        self.changes.push(Change {
            path: path.into(),
            action,
            class,
        });
    }

    pub fn msg(&mut self, s: impl Into<String>) {
        self.messages.push(s.into());
    }

    /// The one JSON object of `--json`; changes are sorted by path.
    pub fn to_json(&self) -> String {
        let mut sorted = self.clone();
        sorted.changes.sort_by(|a, b| a.path.cmp(&b.path));
        serde_json::to_string(&sorted).expect("a Report always serializes")
    }

    /// `--json`: the object on stdout. Otherwise one line per change (or `lines`, when set) on
    /// stdout and the messages on stderr.
    pub fn print(&self, json: bool) {
        if json {
            println!("{}", self.to_json());
            return;
        }
        if self.lines.is_empty() {
            let mut changes: Vec<&Change> = self.changes.iter().collect();
            changes.sort_by(|a, b| a.path.cmp(&b.path));
            for c in changes {
                println!("{}  {}  {}", c.action.as_str(), c.path, c.class.as_str());
            }
        }
        for l in &self.lines {
            println!("{l}");
        }
        for m in &self.messages {
            eprintln!("{m}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_json_shape() {
        let mut r = Report::new("new");
        r.push("b.txt", Action::Create, Class::Seed);
        r.push("a.txt", Action::Merge, Class::Shared);
        let v: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"ok":true,"command":"new",
              "changes":[{"path":"a.txt","action":"merge","class":"shared"},{"path":"b.txt","action":"create","class":"seed"}],
              "messages":[]})
        );
    }
}
