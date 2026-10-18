//! Declarative cleanup rules and the quick-scan engine.

pub mod error;
pub mod loader;
pub mod matcher;
pub mod quick_scan;
pub mod schema;

pub use disksage_core as core;
pub use error::RuleError;
pub use loader::{load_builtin_rules, load_user_rules, parse_rule_pack};
pub use matcher::{CompiledRule, CompiledTarget, compile_rules};
pub use quick_scan::{QuickScanResult, quick_scan, quick_scan_elevated};
pub use schema::{Rule, RuleAction, RulePack, Target};

pub fn schema_version() -> u32 {
    schema::SUPPORTED_SCHEMA_VERSION
}
