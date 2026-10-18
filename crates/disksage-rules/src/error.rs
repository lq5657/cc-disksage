#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("failed to parse rule pack {pack}: {message}")]
    Parse { pack: String, message: String },

    #[error("rule validation failed: {0}")]
    Validation(String),

    #[error("environment error for {0}: {1}")]
    Env(String, String),

    #[error("glob error: {0}")]
    Glob(#[from] globset::Error),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("core error: {0}")]
    Core(#[from] disksage_core::CoreError),
}
