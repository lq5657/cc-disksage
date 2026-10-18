use std::fs;
use std::path::{Path, PathBuf};

use disksage_core::path_safety::{
    is_forbidden_delete_target, is_strict_descendant, is_system_red_line,
};
use disksage_core::{ActionSpec, CleanupPlan, CleanupResult, EnvMap, PlanItem};
use uuid::Uuid;

use crate::audit::{AuditLog, record_for_item};
use crate::error::CleanError;
use crate::fs_safety::{is_reparse_point, looks_locked};

#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionOptions {
    pub confirm: bool,
    pub dry_run: bool,
    pub elevation: bool,
}

/// Execute a previewed plan. Exact paths are re-validated immediately before
/// each deletion, and every failure is reported instead of being forced.
pub fn execute_plan(
    plan: &CleanupPlan,
    env: &EnvMap,
    options: ExecutionOptions,
    audit: Option<&AuditLog>,
) -> Result<CleanupResult, CleanError> {
    if !options.confirm {
        return Err(CleanError::ConfirmationRequired);
    }
    if plan.items.is_empty() {
        return Err(CleanError::EmptyPlan);
    }

    let mut result = CleanupResult {
        job_id: Uuid::new_v4().to_string(),
        ..Default::default()
    };

    for item in &plan.items {
        if matches!(item.action, ActionSpec::EmptyRecycleBin) {
            if options.dry_run {
                result.notes.push("dry-run：将清空回收站（未执行）".into());
            } else {
                match crate::recycle_bin::empty_recycle_bin() {
                    Ok(()) => {
                        result.notes.push("回收站已清空".into());
                        if let Some(audit) = audit {
                            let mut record = record_for_item(
                                &plan.id,
                                &item.finding_id,
                                &item.rule_id,
                                "empty_recycle_bin",
                                Vec::new(),
                                0,
                                "success",
                            );
                            record.elevation = options.elevation;
                            audit.append(&record)?;
                        }
                    }
                    Err(error) => {
                        result
                            .failed
                            .push((PathBuf::from("<recycle-bin>"), error.clone()));
                        result.notes.push(format!("清空回收站失败：{error}"));
                        if let Some(audit) = audit {
                            let mut record = record_for_item(
                                &plan.id,
                                &item.finding_id,
                                &item.rule_id,
                                "empty_recycle_bin",
                                Vec::new(),
                                0,
                                "failed",
                            );
                            record.elevation = options.elevation;
                            audit.append(&record)?;
                        }
                    }
                }
            }
            continue;
        }

        if matches!(item.action, ActionSpec::Guided { .. }) {
            result.notes.push(format!(
                "{}：系统引导动作需由主界面打开系统设置执行",
                item.rule_id
            ));
            continue;
        }

        if !matches!(item.action, ActionSpec::Trash | ActionSpec::Delete) {
            result.notes.push(format!("{}：动作尚未实现", item.rule_id));
            continue;
        }

        let mut item_deleted = 0u64;
        let mut item_failed = 0u64;
        let mut item_bytes = 0u64;
        let mut paths_for_audit = Vec::new();

        for path in &item.targets {
            match delete_one(path, item, env, options.dry_run) {
                Ok(bytes) => {
                    result.deleted = result.deleted.saturating_add(1);
                    result.freed_bytes = result.freed_bytes.saturating_add(bytes);
                    item_deleted = item_deleted.saturating_add(1);
                    item_bytes = item_bytes.saturating_add(bytes);
                    paths_for_audit.push(path.display().to_string());
                }
                Err(error) => {
                    item_failed = item_failed.saturating_add(1);
                    if looks_locked(&error.to_string()) {
                        result.skipped_locked.push(path.clone());
                    } else {
                        result.failed.push((path.clone(), error.to_string()));
                    }
                }
            }
        }

        if let Some(audit) = audit
            && !options.dry_run
        {
            let status = if item_failed == 0 {
                "success"
            } else if item_deleted > 0 {
                "partial"
            } else {
                "failed"
            };
            let mut record = record_for_item(
                &plan.id,
                &item.finding_id,
                &item.rule_id,
                action_label(&item.action),
                paths_for_audit,
                item_bytes,
                status,
            );
            record.elevation = options.elevation;
            audit.append(&record)?;
        }
    }

    Ok(result)
}

fn delete_one(
    path: &Path,
    item: &PlanItem,
    env: &EnvMap,
    dry_run: bool,
) -> Result<u64, CleanError> {
    let metadata = path.symlink_metadata()?;
    if is_reparse_point(&metadata) || !metadata.is_file() {
        return Err(CleanError::InvalidPath(path.to_path_buf()));
    }
    if item.allowed_roots.is_empty()
        || !item
            .allowed_roots
            .iter()
            .any(|root| is_strict_descendant(path, root))
    {
        return Err(CleanError::InvalidPath(path.to_path_buf()));
    }
    if is_system_red_line(path, env) || is_forbidden_delete_target(path, env) {
        return Err(CleanError::InvalidPath(path.to_path_buf()));
    }

    let bytes = metadata.len();
    if dry_run {
        return Ok(bytes);
    }

    match &item.action {
        ActionSpec::Trash => trash::delete(path)
            .map_err(|error| CleanError::Io(std::io::Error::other(error.to_string())))?,
        ActionSpec::Delete => fs::remove_file(path)?,
        other => {
            return Err(CleanError::UnsupportedAction(format!("{other:?}")));
        }
    }

    Ok(bytes)
}

fn action_label(action: &ActionSpec) -> &'static str {
    match action {
        ActionSpec::Trash => "trash",
        ActionSpec::Delete => "delete",
        ActionSpec::Quarantine => "quarantine",
        ActionSpec::EmptyRecycleBin => "empty_recycle_bin",
        ActionSpec::ServiceCleanup { .. } => "service_cleanup",
        ActionSpec::DismAnalyze => "dism_analyze",
        ActionSpec::DismStartComponentCleanup => "dism_component_cleanup",
        ActionSpec::Guided { .. } => "guided",
        ActionSpec::HibernateOff => "hibernate_off",
        ActionSpec::CreateRestorePoint => "create_restore_point",
    }
}
