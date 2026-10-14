use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Owned,
    Shared,
    Seed,
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

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub path: String,
    pub action: Action,
    pub class: Class,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ok: bool,
    pub command: String,
    pub changes: Vec<Change>,
    pub messages: Vec<String>,
}

impl Report {
    pub fn new(command: &str) -> Self {
        Report {
            ok: true,
            command: command.to_string(),
            changes: Vec::new(),
            messages: Vec::new(),
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

    fn sorted(&self) -> Report {
        let mut r = self.clone();
        r.changes.sort_by(|a, b| a.path.cmp(&b.path));
        r
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.sorted()).expect("report serializes")
    }

    /// JSON: exactly one object on stdout. Human: one line per change, then messages.
    pub fn print(&self, json: bool) {
        if json {
            println!("{}", self.to_json());
            return;
        }
        let r = self.sorted();
        for c in &r.changes {
            println!("{:<8} {}  {}", lower(&c.action), c.path, lower(&c.class));
        }
        for m in &r.messages {
            println!("{m}");
        }
    }
}

fn lower<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
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
