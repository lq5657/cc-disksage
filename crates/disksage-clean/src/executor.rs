use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use disksage_core::path_safety::{
    is_forbidden_delete_target, is_strict_descendant, is_system_red_line,
};
use disksage_core::{ActionSpec, CleanupPlan, CleanupResult, EnvMap, PlanItem};
use serde::Serialize;
use uuid::Uuid;

use crate::audit::{AuditLog, record_for_item};
use crate::error::CleanError;
use crate::fs_safety::{is_reparse_point, looks_locked};

/// Number of files handed to one IFileOperation call. Batching is the main
/// speed-up for recycle-bin deletion: creating a COM object per file is far
/// more expensive than the deletion itself.
const TRASH_BATCH_SIZE: usize = 256;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionOptions {
    pub confirm: bool,
    pub dry_run: bool,
    pub elevation: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CleanupProgress {
    /// preparing / deleting / recycle_bin / guided / admin / done
    pub phase: String,
    pub processed: u64,
    pub total: u64,
    pub deleted: u64,
    pub failed: u64,
    pub bytes: u64,
    pub current: String,
    pub elapsed_ms: u64,
}

pub type ProgressCallback<'a> = &'a mut dyn FnMut(CleanupProgress);

/// Execute a previewed plan. Exact paths are re-validated immediately before
/// each deletion, and every failure is reported instead of being forced.
pub fn execute_plan(
    plan: &CleanupPlan,
    env: &EnvMap,
    options: ExecutionOptions,
    audit: Option<&AuditLog>,
) -> Result<CleanupResult, CleanError> {
    execute_plan_with_progress(plan, env, options, audit, None)
}

pub fn execute_plan_with_progress(
    plan: &CleanupPlan,
    env: &EnvMap,
    options: ExecutionOptions,
    audit: Option<&AuditLog>,
    mut progress: Option<ProgressCallback<'_>>,
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

    let total = plan
        .items
        .iter()
        .filter(|item| matches!(item.action, ActionSpec::Trash | ActionSpec::Delete))
        .map(|item| item.targets.len() as u64)
        .sum();
    let started = Instant::now();
    let mut state = CleanupProgress {
        phase: "preparing".into(),
        total,
        ..Default::default()
    };
    let mut last_emit = started;
    emit_progress(&mut progress, &mut state, &mut last_emit, true, started);

    for item in &plan.items {
        if matches!(item.action, ActionSpec::EmptyRecycleBin) {
            state.phase = "recycle_bin".into();
            state.current = item.title_zh.clone();
            emit_progress(&mut progress, &mut state, &mut last_emit, true, started);

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
            state.phase = "guided".into();
            state.current = item.title_zh.clone();
            emit_progress(&mut progress, &mut state, &mut last_emit, true, started);
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

        state.phase = "deleting".into();
        state.current = item.title_zh.clone();
        emit_progress(&mut progress, &mut state, &mut last_emit, true, started);

        // Validate every target first so a batch never contains a path that
        // escaped its rule root or resolved to a reparse point.
        let mut valid: Vec<(PathBuf, u64)> = Vec::with_capacity(item.targets.len());
        for path in &item.targets {
            match validate_target(path, item, env) {
                Ok(bytes) => valid.push((path.clone(), bytes)),
                Err(error) => {
                    state.processed = state.processed.saturating_add(1);
                    state.failed = state.failed.saturating_add(1);
                    item_failed = item_failed.saturating_add(1);
                    record_failure(&mut result, path, &error.to_string());
                }
            }
        }

        if options.dry_run {
            for (path, bytes) in &valid {
                item_deleted = item_deleted.saturating_add(1);
                item_bytes = item_bytes.saturating_add(*bytes);
                state.deleted = state.deleted.saturating_add(1);
                state.bytes = state.bytes.saturating_add(*bytes);
                state.processed = state.processed.saturating_add(1);
                state.current = path.display().to_string();
                emit_progress(&mut progress, &mut state, &mut last_emit, false, started);
            }
        } else if matches!(item.action, ActionSpec::Delete) {
            for (path, bytes) in &valid {
                match fs::remove_file(path) {
                    Ok(()) => {
                        item_deleted = item_deleted.saturating_add(1);
                        item_bytes = item_bytes.saturating_add(*bytes);
                        state.deleted = state.deleted.saturating_add(1);
                        state.bytes = state.bytes.saturating_add(*bytes);
                    }
                    Err(error) => {
                        item_failed = item_failed.saturating_add(1);
                        state.failed = state.failed.saturating_add(1);
                        record_failure(&mut result, path, &error.to_string());
                    }
                }
                state.processed = state.processed.saturating_add(1);
                state.current = path.display().to_string();
                emit_progress(&mut progress, &mut state, &mut last_emit, false, started);
            }
        } else {
            for chunk in valid.chunks(TRASH_BATCH_SIZE) {
                delete_trash_batch(
                    chunk,
                    &mut result,
                    &mut state,
                    &mut item_deleted,
                    &mut item_failed,
                    &mut item_bytes,
                );
                state.processed = state.processed.saturating_add(chunk.len() as u64);
                state.current = chunk
                    .last()
                    .map(|(path, _)| path.display().to_string())
                    .unwrap_or_default();
                emit_progress(&mut progress, &mut state, &mut last_emit, true, started);
            }
        }

        result.deleted = result.deleted.saturating_add(item_deleted);
        result.freed_bytes = result.freed_bytes.saturating_add(item_bytes);

        paths_for_audit.extend(
            valid
                .iter()
                .filter(|(path, _)| options.dry_run || !path.exists())
                .map(|(path, _)| path.display().to_string()),
        );

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

    state.phase = "done".into();
    state.current.clear();
    emit_progress(&mut progress, &mut state, &mut last_emit, true, started);

    Ok(result)
}

fn delete_trash_batch(
    chunk: &[(PathBuf, u64)],
    result: &mut CleanupResult,
    state: &mut CleanupProgress,
    item_deleted: &mut u64,
    item_failed: &mut u64,
    item_bytes: &mut u64,
) {
    let batch = trash::delete_all(chunk.iter().map(|(path, _)| path.as_path()));
    if batch.is_ok() {
        for (path, bytes) in chunk {
            // IFileOperation can report success while skipping an individual
            // item, so verify the path is actually gone.
            if path.symlink_metadata().is_err() {
                *item_deleted = item_deleted.saturating_add(1);
                *item_bytes = item_bytes.saturating_add(*bytes);
                state.deleted = state.deleted.saturating_add(1);
                state.bytes = state.bytes.saturating_add(*bytes);
            } else {
                *item_failed = item_failed.saturating_add(1);
                state.failed = state.failed.saturating_add(1);
                result.skipped_locked.push(path.clone());
            }
        }
        return;
    }

    // A batch can fail as a whole when one file is locked. Retry each item to
    // keep precise per-file results (and to delete everything not locked).
    for (path, bytes) in chunk {
        match trash::delete(path) {
            Ok(()) => {
                *item_deleted = item_deleted.saturating_add(1);
                *item_bytes = item_bytes.saturating_add(*bytes);
                state.deleted = state.deleted.saturating_add(1);
                state.bytes = state.bytes.saturating_add(*bytes);
            }
            Err(error) => {
                *item_failed = item_failed.saturating_add(1);
                state.failed = state.failed.saturating_add(1);
                record_failure(result, path, &error.to_string());
            }
        }
    }
}

fn emit_progress(
    sink: &mut Option<ProgressCallback<'_>>,
    progress: &mut CleanupProgress,
    last_emit: &mut Instant,
    force: bool,
    started: Instant,
) {
    if force || last_emit.elapsed() >= PROGRESS_INTERVAL {
        progress.elapsed_ms = started.elapsed().as_millis() as u64;
        if let Some(callback) = sink.as_mut() {
            callback(progress.clone());
        }
        *last_emit = Instant::now();
    }
}

fn validate_target(path: &Path, item: &PlanItem, env: &EnvMap) -> Result<u64, CleanError> {
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
    Ok(metadata.len())
}

fn record_failure(result: &mut CleanupResult, path: &Path, message: &str) {
    if looks_locked(message) {
        result.skipped_locked.push(path.to_path_buf());
    } else {
        result
            .failed
            .push((path.to_path_buf(), message.to_string()));
    }
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
