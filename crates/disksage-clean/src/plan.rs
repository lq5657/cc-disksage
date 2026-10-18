use std::collections::HashSet;
use std::path::PathBuf;

use chrono::Utc;
use disksage_core::path_safety::{
    is_forbidden_delete_target, is_strict_descendant, is_system_red_line,
};
use disksage_core::{
    ActionSpec, CleanupPlan, EnvMap, PlanItem, PlanStatus, PlanWarning, Recoverability, RiskLevel,
};
use uuid::Uuid;

use crate::error::CleanError;
use crate::fs_safety::is_reparse_point;

#[derive(Debug, Clone)]
pub struct PlanCandidate {
    pub finding_id: String,
    pub rule_id: String,
    pub title_zh: String,
    pub risk: RiskLevel,
    pub recoverability: Recoverability,
    pub requires_admin: bool,
    pub action: ActionSpec,
    pub files: Vec<PathBuf>,
    pub allowed_roots: Vec<PathBuf>,
}

/// Build a preview plan from findings that were produced by the local scanner.
/// Exact files are re-validated here; the frontend never supplies paths.
pub fn create_plan(
    candidates: Vec<PlanCandidate>,
    env: &EnvMap,
) -> Result<CleanupPlan, CleanError> {
    let mut items = Vec::new();
    let mut warnings = Vec::new();

    for candidate in candidates {
        let is_admin = candidate.requires_admin;

        if !is_admin && !matches!(candidate.action, ActionSpec::Trash | ActionSpec::Delete) {
            warnings.push(PlanWarning {
                code: "unsupported_action".into(),
                message_zh: format!("动作 {:?} 需要管理员权限，已跳过。", candidate.action),
                message_en: format!(
                    "Action {:?} requires administrator rights and was skipped.",
                    candidate.action
                ),
            });
            continue;
        }

        let mut seen = HashSet::new();
        let mut valid_files = Vec::new();
        let mut bytes = 0u64;
        let mut missing = 0u64;
        let mut rejected = 0u64;

        for file in candidate.files {
            if !seen.insert(file.clone()) {
                continue;
            }
            let metadata = match file.symlink_metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    missing = missing.saturating_add(1);
                    continue;
                }
            };
            if is_reparse_point(&metadata) || !metadata.is_file() {
                rejected = rejected.saturating_add(1);
                continue;
            }
            if candidate.allowed_roots.is_empty()
                || !candidate
                    .allowed_roots
                    .iter()
                    .any(|root| is_strict_descendant(&file, root))
            {
                rejected = rejected.saturating_add(1);
                continue;
            }
            if is_system_red_line(&file, env) || is_forbidden_delete_target(&file, env) {
                rejected = rejected.saturating_add(1);
                continue;
            }

            bytes = bytes.saturating_add(metadata.len());
            valid_files.push(file);
        }

        if missing > 0 {
            warnings.push(PlanWarning {
                code: "missing_files".into(),
                message_zh: format!("{missing} 个文件在扫描后已不存在，已跳过。"),
                message_en: format!("{missing} files disappeared after the scan and were skipped."),
            });
        }
        if rejected > 0 {
            warnings.push(PlanWarning {
                code: "rejected_files".into(),
                message_zh: format!("{rejected} 个路径未通过安全校验，已跳过。"),
                message_en: format!("{rejected} paths failed safety validation and were skipped."),
            });
        }
        if valid_files.is_empty() && !is_admin {
            continue;
        }

        let file_count = valid_files.len() as u64;
        items.push(PlanItem {
            finding_id: candidate.finding_id,
            rule_id: candidate.rule_id,
            title_zh: candidate.title_zh,
            risk: candidate.risk,
            action: candidate.action,
            targets: valid_files,
            allowed_roots: candidate.allowed_roots,
            estimated_bytes: bytes,
            file_count,
            recoverability: candidate.recoverability,
            requires_admin: is_admin,
        });
    }

    if items.iter().any(|item| item.requires_admin) {
        warnings.push(PlanWarning {
            code: "admin_helper".into(),
            message_zh: "管理员项将在执行时通过 UAC 提权 helper 重新扫描并校验；预览大小为提权前估算。".into(),
            message_en: "Administrator items are re-scanned and validated by the elevated helper; preview sizes may be incomplete.".into(),
        });
    }

    if items.is_empty() {
        return Err(CleanError::EmptyPlan);
    }

    let total_allocated_bytes = items.iter().map(|item| item.estimated_bytes).sum::<u64>();
    let requires_admin = items.iter().any(|item| item.requires_admin);

    Ok(CleanupPlan {
        id: Uuid::new_v4().to_string(),
        snapshot_id: "quick-scan".into(),
        created_at: Utc::now().timestamp(),
        items,
        total_allocated_bytes,
        requires_admin,
        warnings,
        status: PlanStatus::Previewed,
    })
}
