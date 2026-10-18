use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;

use crate::error::CleanError;

#[derive(Debug, Clone, Serialize)]
pub struct AuditRecord {
    pub id: String,
    pub plan_id: String,
    pub finding_id: String,
    pub rule_id: String,
    pub action: String,
    pub paths: Vec<String>,
    pub bytes: u64,
    pub result: String,
    pub timestamp: i64,
    pub elevation: bool,
}

#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_location() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("cc-disksage")
            .join("logs")
            .join("audit.jsonl")
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn append(&self, record: &AuditRecord) -> Result<(), CleanError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(record)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }
}

pub fn record_for_item(
    plan_id: &str,
    finding_id: &str,
    rule_id: &str,
    action: &str,
    paths: Vec<String>,
    bytes: u64,
    result: &str,
) -> AuditRecord {
    AuditRecord {
        id: uuid::Uuid::new_v4().to_string(),
        plan_id: plan_id.to_string(),
        finding_id: finding_id.to_string(),
        rule_id: rule_id.to_string(),
        action: action.to_string(),
        paths,
        bytes,
        result: result.to_string(),
        timestamp: Utc::now().timestamp(),
        elevation: false,
    }
}
