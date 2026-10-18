//! Optional OpenAI-compatible LLM assessment. The LLM only annotates local
//! findings; it never supplies paths, commands, numbers, or deletion actions.

pub mod client;
pub mod config;
pub mod error;
pub mod key_store;
pub mod payload;
pub mod validate;

pub use client::LlmClient;
pub use config::{LlmConfig, LlmPreset, PRESETS, config_for_preset, preset};
pub use disksage_core as core;
pub use error::LlmError;
pub use key_store::{delete_key, get_key, set_key};
pub use payload::{RedactContext, build_finding_payload, payload_json};
pub use validate::validate_assessment;
