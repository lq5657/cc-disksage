#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("API key is not configured")]
    MissingKey,

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("provider returned HTTP {status}: {body}")]
    Status { status: u16, body: String },

    #[error("invalid JSON from provider: {0}")]
    Json(#[from] serde_json::Error),

    #[error("provider response failed validation: {0}")]
    Validation(String),

    #[error("Windows Credential Manager error: {0}")]
    Keyring(String),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}
