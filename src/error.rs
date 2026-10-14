use crate::output::Report;

#[derive(Debug, thiserror::Error)]
pub enum SpliceError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Template(String),
    #[error("{0}")]
    Env(String),
    #[error("conflicts or refusal")]
    Conflict(Report),
    #[error("drift detected")]
    Drift(Report),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl SpliceError {
    pub fn exit_code(&self) -> i32 {
        match self {
            SpliceError::Usage(_) => 2,
            SpliceError::Template(_) | SpliceError::Env(_) | SpliceError::Io(_) => 3,
            SpliceError::Conflict(_) | SpliceError::Drift(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(SpliceError::Usage("x".into()).exit_code(), 2);
        assert_eq!(SpliceError::Template("x".into()).exit_code(), 3);
        assert_eq!(SpliceError::Conflict(Report::new("update")).exit_code(), 1);
    }
}
