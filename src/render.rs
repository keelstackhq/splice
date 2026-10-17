//! MiniJinja engine configured for the portable subset (spec §6).

use crate::error::SpliceError;
use crate::subset;

type Result<T> = std::result::Result<T, SpliceError>;

pub struct Renderer {
    env: minijinja::Environment<'static>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let mut env = minijinja::Environment::empty();
        env.add_filter("lower", |s: String| s.to_lowercase());
        env.add_filter("upper", |s: String| s.to_uppercase());
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
        env.set_keep_trailing_newline(true);
        Renderer { env }
    }

    /// Validate against the subset, render, and normalise `\r\n` to `\n`.
    pub fn render_str(&self, text: &str, ctx: &minijinja::Value, origin: &str) -> Result<String> {
        subset::validate(text, origin)?;
        let out = self
            .env
            .render_named_str(origin, text, ctx)
            .map_err(|e| SpliceError::Template(format!("{origin}: {e}")))?;
        Ok(out.replace("\r\n", "\n"))
    }
}
