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

/// A template variable new in the update target that the repo had no answer for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewQuestion {
    pub name: String,
    pub prompt: String,
    /// `defaulted`: the template's default was used; the owner may override it.
    pub status: String,
    pub value: String,
}

/// A variable a template or piece declares (`pieces --json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VarInfo {
    pub name: String,
    pub prompt: String,
    pub default: Option<String>,
}

/// One piece of a template (`pieces --json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PieceInfo {
    pub id: String,
    pub description: String,
    pub base: bool,
    pub multiple: bool,
    /// `multiple` pieces only: the instance name `--piece ID` gets without `:NAME`.
    pub default_name: Option<String>,
    pub requires: Vec<String>,
    pub vars: Vec<VarInfo>,
}

/// The template-level part of `pieces --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateInfo {
    pub name: String,
    pub version: Option<String>,
    pub vars: Vec<VarInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub ok: bool,
    pub command: String,
    pub changes: Vec<Change>,
    pub messages: Vec<String>,
    /// `update` only: vars new in the target that took their default (omitted when none).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub new_questions: Vec<NewQuestion>,
    /// `pieces` only: the template's pieces (omitted when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<PieceInfo>,
    /// `pieces` only: the template's own name, version and variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<TemplateInfo>,
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
            new_questions: Vec::new(),
            pieces: Vec::new(),
            template: None,
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

    /// Adds a message; a text of several lines becomes one message per line.
    pub fn msg(&mut self, s: impl Into<String>) {
        let s = s.into();
        if s.contains('\n') {
            self.messages.extend(s.lines().map(str::to_string));
        } else {
            self.messages.push(s);
        }
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
