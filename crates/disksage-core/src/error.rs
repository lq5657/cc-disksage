use std::path::PathBuf;

/// Errors shared by the scanner, rule engine and cleanup pipeline.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("path is outside the allowed roots: {0}")]
    OutsideAllowedRoot(PathBuf),

    #[error("path is a protected system red-line location: {0}")]
    RedLine(PathBuf),

    #[error("environment variable is not allowed: {0}")]
    EnvNotAllowed(String),

    #[error("environment variable is missing: {0}")]
    EnvMissing(String),

    #[error("path contains a wildcard where a literal path is required: {0}")]
    WildcardInLiteral(String),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
