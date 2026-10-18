//! Shared domain types and safety primitives for CC DiskSage.

pub mod domain;
pub mod error;
pub mod path_safety;

pub use domain::*;
pub use error::{CoreError, Result};
pub use path_safety::EnvMap;
