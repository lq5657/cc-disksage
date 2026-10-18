//! Elevated helper. It re-loads the compiled-in rules, re-scans the requested
//! admin targets, and executes only fixed action types. It never trusts paths
//! supplied by the UI.

pub mod actions;
pub mod job;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use disksage_clean::audit::record_for_item;
use disksage_clean::{
    AuditLog, CleanError, ExecutionOptions, PlanCandidate, create_plan, execute_plan,
};
use disksage_core::{ActionSpec, CleanupResult, EnvMap, ScanPayload};
use disksage_rules::{RulePack, load_builtin_rules, quick_scan, quick_scan_elevated};
#[cfg(windows)]
use disksage_scan::{CancelToken, MftScanRequest, scan_volume_mft};

use crate::job::{HelperJob, scan_payload_path, write_scan_payload};
#[cfg(windows)]
use crate::job::{snapshot_path, write_snapshot};

pub fn run_job(job: &HelperJob) -> CleanupResult {
    let mut result = CleanupResult {
        job_id: job.job_id.clone(),
        ..CleanupResult::default()
    };

    if !actions::is_elevated() {
        fatal(&mut result, "helper 未以管理员身份运行，拒绝执行");
        return result;
    }

    let env: EnvMap = std::env::vars().collect();
    let all_rules = match load_builtin_rules() {
        Ok(pack) => pack,
        Err(error) => {
            fatal(&mut result, format!("加载内置规则失败：{error}"));
            return result;
        }
    };

    let mut selected = Vec::new();
    for rule_id in &job.rule_ids {
        match all_rules.rules.iter().find(|rule| &rule.id == rule_id) {
            Some(rule) if rule.requires_admin => selected.push(rule.clone()),
            Some(_) => result
                .notes
                .push(format!("规则 {rule_id} 不需要管理员权限，已跳过")),
            None => result.notes.push(format!("未知规则 {rule_id}，已跳过")),
        }
    }
    if selected.is_empty() {
        fatal(&mut result, "没有可执行的管理员规则");
        return result;
    }

    let pack = RulePack {
        schema_version: all_rules.schema_version,
        rules: selected,
    };
    let scan = match quick_scan(&pack, &env, true) {
        Ok(scan) => scan,
        Err(error) => {
            fatal(&mut result, format!("提权扫描失败：{error}"));
            return result;
        }
    };

    let audit = AuditLog::new(AuditLog::default_location());
    let mut seen_rules = HashSet::new();

    for finding in scan.findings {
        seen_rules.insert(finding.rule_id.clone());
        let files = scan.matches.get(&finding.id).cloned().unwrap_or_default();

        match finding.action.clone() {
            ActionSpec::Trash | ActionSpec::Delete => {
                let candidate = PlanCandidate {
                    finding_id: finding.id.clone(),
                    rule_id: finding.rule_id.clone(),
                    title_zh: finding.title_zh.clone(),
                    risk: finding.risk,
                    recoverability: finding.recoverability,
                    requires_admin: true,
                    action: finding.action.clone(),
                    files,
                    allowed_roots: finding
                        .targets
                        .iter()
                        .map(|target| target.resolved.clone())
                        .collect(),
                };
                match create_plan(vec![candidate], &env) {
                    Ok(plan) => match execute_plan(
                        &plan,
                        &env,
                        ExecutionOptions {
                            confirm: true,
                            dry_run: false,
                            elevation: true,
                        },
                        Some(&audit),
                    ) {
                        Ok(mut partial) => merge(&mut result, &mut partial),
                        Err(error) => result
                            .notes
                            .push(format!("{} 执行失败：{error}", finding.rule_id)),
                    },
                    Err(CleanError::EmptyPlan) => result
                        .notes
                        .push(format!("{}：没有可清理的文件", finding.rule_id)),
                    Err(error) => result
                        .notes
                        .push(format!("{} 计划失败：{error}", finding.rule_id)),
                }
            }
            ActionSpec::ServiceCleanup { services, paths } => {
                actions::service_cleanup(&services, &paths, &env, &mut result);
                audit_action(&audit, job, &finding.rule_id, "service_cleanup");
            }
            ActionSpec::DismAnalyze => {
                actions::dism_analyze(&mut result);
                audit_action(&audit, job, &finding.rule_id, "dism_analyze");
            }
            ActionSpec::DismStartComponentCleanup => {
                actions::dism_cleanup(&mut result);
                audit_action(&audit, job, &finding.rule_id, "dism_component_cleanup");
            }
            ActionSpec::HibernateOff => {
                actions::hibernate_off(&mut result);
                audit_action(&audit, job, &finding.rule_id, "hibernate_off");
            }
            ActionSpec::EmptyRecycleBin => {
                actions::empty_recycle_bin(&mut result);
                audit_action(&audit, job, &finding.rule_id, "empty_recycle_bin");
            }
            ActionSpec::Guided { .. } => result.notes.push(format!(
                "{}：该动作需要在主界面打开系统设置执行",
                finding.rule_id
            )),
            ActionSpec::Quarantine => result
                .notes
                .push(format!("{}：隔离区尚未实现", finding.rule_id)),
            ActionSpec::CreateRestorePoint => result
                .notes
                .push(format!("{}：恢复点动作尚未实现", finding.rule_id)),
        }
    }

    for rule_id in &job.rule_ids {
        if !seen_rules.contains(rule_id) {
            result.notes.push(format!(
                "规则 {rule_id} 未产生发现项（目标路径可能不存在或不可访问）"
            ));
        }
    }

    if result.failed.is_empty() {
        result.notes.push("helper 执行完成".into());
    } else {
        result.notes.push(format!(
            "helper 执行完成，{} 个文件失败",
            result.failed.len()
        ));
    }

    result
}

pub fn run_quick_scan_job(job: &HelperJob, job_path: &Path) -> CleanupResult {
    let mut result = CleanupResult {
        job_id: job.job_id.clone(),
        ..CleanupResult::default()
    };

    if !actions::is_elevated() {
        fatal(&mut result, "helper 未以管理员身份运行，拒绝执行");
        return result;
    }

    let env: EnvMap = std::env::vars().collect();
    let all_rules = match load_builtin_rules() {
        Ok(pack) => pack,
        Err(error) => {
            fatal(&mut result, format!("加载内置规则失败：{error}"));
            return result;
        }
    };
    let schema_version = all_rules.schema_version;
    let rules = all_rules
        .rules
        .into_iter()
        .filter(|rule| rule.requires_admin)
        .collect::<Vec<_>>();
    if rules.is_empty() {
        fatal(&mut result, "没有需要管理员权限的内置规则");
        return result;
    }

    let pack = RulePack {
        schema_version,
        rules,
    };
    match quick_scan_elevated(&pack, &env, true) {
        Ok(scan) => {
            let mut targets = std::collections::HashMap::new();
            for finding in &scan.findings {
                targets.insert(finding.id.clone(), finding.targets.clone());
            }
            let payload = ScanPayload {
                findings: scan.findings,
                matches: scan.matches,
                targets,
            };
            match write_scan_payload(&scan_payload_path(job_path), &payload) {
                Ok(()) => result
                    .notes
                    .push(format!("管理员扫描完成：{} 项", payload.findings.len())),
                Err(error) => fatal(&mut result, error),
            }
        }
        Err(error) => fatal(&mut result, format!("管理员扫描失败：{error}")),
    }

    result
}

#[cfg(windows)]
pub fn run_mft_job(job: &HelperJob, job_path: &Path) -> CleanupResult {
    let mut result = CleanupResult {
        job_id: job.job_id.clone(),
        ..CleanupResult::default()
    };

    if !actions::is_elevated() {
        fatal(&mut result, "helper 未以管理员身份运行，拒绝执行");
        return result;
    }

    let Some(volume) = job.volume.as_deref() else {
        fatal(&mut result, "MFT 任务缺少卷盘符");
        return result;
    };
    let letter = volume.to_ascii_uppercase().chars().next().unwrap_or('C');
    if let Some(info) = disksage_scan::enumerate_volumes()
        .into_iter()
        .find(|info| info.letter == letter)
        && !info.filesystem.eq_ignore_ascii_case("NTFS")
    {
        fatal(
            &mut result,
            format!(
                "{} 盘不是 NTFS（{}），无法使用 MFT 扫描",
                letter, info.filesystem
            ),
        );
        return result;
    }

    match scan_volume_mft(MftScanRequest::new(letter), |_| {}, CancelToken::new()) {
        Ok(snapshot) => {
            let path = snapshot_path(job_path);
            match write_snapshot(&path, &snapshot) {
                Ok(()) => result.notes.push(format!(
                    "MFT 扫描完成：{} 文件 / {} 目录 / {:.2} GB，用时 {} ms",
                    snapshot.file_count,
                    snapshot.dir_count,
                    snapshot.allocated_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
                    snapshot.duration_ms
                )),
                Err(error) => fatal(&mut result, error),
            }
        }
        Err(error) => fatal(&mut result, format!("MFT 扫描失败：{error}")),
    }

    result
}

fn fatal(result: &mut CleanupResult, message: impl Into<String>) {
    let message = message.into();
    result.notes.push(message.clone());
    result.failed.push((PathBuf::from("<helper>"), message));
}

fn merge(target: &mut CleanupResult, partial: &mut CleanupResult) {
    target.freed_bytes = target.freed_bytes.saturating_add(partial.freed_bytes);
    target.deleted = target.deleted.saturating_add(partial.deleted);
    target.skipped_locked.append(&mut partial.skipped_locked);
    target.failed.append(&mut partial.failed);
    target.cancelled |= partial.cancelled;
    target.notes.append(&mut partial.notes);
}

fn audit_action(audit: &AuditLog, job: &HelperJob, rule_id: &str, action: &str) {
    let mut record = record_for_item(&job.job_id, "", rule_id, action, Vec::new(), 0, "success");
    record.elevation = true;
    let _ = audit.append(&record);
}
