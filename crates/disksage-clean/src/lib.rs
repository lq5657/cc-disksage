//! Cleanup planning, execution, and auditing.

pub mod audit;
pub mod error;
pub mod executor;
pub mod fs_safety;
pub mod plan;

pub use audit::{AuditLog, AuditRecord};
pub use disksage_core as core;
pub use error::CleanError;
pub use executor::{ExecutionOptions, execute_plan};
pub use plan::{PlanCandidate, create_plan};
