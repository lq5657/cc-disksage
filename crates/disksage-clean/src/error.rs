use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CleanError {
    #[error("plan has no executable items")]
    EmptyPlan,

    #[error("explicit confirmation is required before deleting anything")]
    ConfirmationRequired,

    #[error("action is not implemented in this build: {0}")]
    UnsupportedAction(String),

    #[error("administrator actions require the elevated helper")]
    RequiresAdmin,

    #[error("invalid deletion path: {0}")]
    InvalidPath(PathBuf),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
