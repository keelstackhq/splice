//! MiniJinja engine configured for the portable subset (spec section 6).

use crate::error::SpliceError;
use crate::subset;
use minijinja::{Environment, UndefinedBehavior, Value};

pub struct Renderer {
    env: Environment<'static>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let mut env = Environment::empty();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_keep_trailing_newline(true);
        env.add_filter("lower", |s: String| s.to_lowercase());
        env.add_filter("upper", |s: String| s.to_uppercase());
        Renderer { env }
    }

    /// Gate, render, then normalise `\r\n` to `\n`.
    pub fn render_str(&self, text: &str, ctx: &Value, origin: &str) -> Result<String, SpliceError> {
        subset::validate(text, origin)?;
        let out = self
            .env
            .render_str(text, ctx)
            .map_err(|e| SpliceError::Template(format!("{origin}: {e}")))?;
        Ok(out.replace("\r\n", "\n"))
    }
}
