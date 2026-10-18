use std::collections::HashMap;
use std::path::PathBuf;

use chrono::DateTime;
use disksage_core::path_safety::DeleteGuard;
use disksage_core::{
    ActionSpec, EnvMap, Evidence, EvidenceKind, EvidenceSource, Finding, MatchedPath,
    Recoverability, RiskLevel, SampleFile, SizeProvenance,
};
use rayon::prelude::*;
use uuid::Uuid;

use crate::error::RuleError;
use crate::matcher::{CompiledRule, MatchedFile, compile_rules};
use crate::schema::RulePack;

pub struct QuickScanResult {
    pub findings: Vec<Finding>,
    /// Exact files stay on the Rust side and are used by the cleanup planner.
    pub matches: HashMap<String, Vec<PathBuf>>,
}

pub fn quick_scan(
    pack: &RulePack,
    env: &EnvMap,
    include_admin: bool,
) -> Result<QuickScanResult, RuleError> {
    quick_scan_impl(pack, env, include_admin, false)
}

/// Scan from the elevated helper: admin-rule sizes and evidence are marked as
/// measured by the helper instead of estimated by the ordinary process.
pub fn quick_scan_elevated(
    pack: &RulePack,
    env: &EnvMap,
    include_admin: bool,
) -> Result<QuickScanResult, RuleError> {
    quick_scan_impl(pack, env, include_admin, true)
}

fn quick_scan_impl(
    pack: &RulePack,
    env: &EnvMap,
    include_admin: bool,
    admin_measured: bool,
) -> Result<QuickScanResult, RuleError> {
    let rules = compile_rules(pack, env, include_admin)?;
    let guard = DeleteGuard::new(env);

    let results = rules
        .par_iter()
        .filter_map(|compiled| {
            let files = compiled.matched_files(&guard);
            if files.is_empty()
                && !compiled.rule.requires_admin
                && !is_action_only(&compiled.action)
            {
                return None;
            }
            let finding = build_finding(compiled, &files, admin_measured);
            let paths = files.into_iter().map(|file| file.path).collect();
            Some((finding, paths))
        })
        .collect::<Vec<_>>();

    let mut findings = Vec::with_capacity(results.len());
    let mut matches = HashMap::with_capacity(results.len());
    for (finding, paths) in results {
        matches.insert(finding.id.clone(), paths);
        findings.push(finding);
    }

    findings.sort_by(|a, b| {
        risk_order(a.risk)
            .cmp(&risk_order(b.risk))
            .then_with(|| b.allocated_bytes.cmp(&a.allocated_bytes))
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });

    Ok(QuickScanResult { findings, matches })
}

fn risk_order(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::Safe => 0,
        RiskLevel::Caution => 1,
        RiskLevel::Danger => 2,
    }
}

fn is_action_only(action: &ActionSpec) -> bool {
    matches!(
        action,
        ActionSpec::EmptyRecycleBin | ActionSpec::Guided { .. }
    )
}

fn build_finding(compiled: &CompiledRule, files: &[MatchedFile], admin_measured: bool) -> Finding {
    let rule = &compiled.rule;
    let mut allocated_bytes = 0u64;
    let mut file_count = 0u64;
    let mut oldest: Option<i64> = None;
    let mut newest: Option<i64> = None;
    let mut ext_counts: HashMap<String, u64> = HashMap::new();
    let mut sample_files = Vec::new();
    let mut target_stats: HashMap<PathBuf, (u64, u64)> = HashMap::new();

    for file in files {
        allocated_bytes = allocated_bytes.saturating_add(file.bytes);
        file_count = file_count.saturating_add(1);

        let extension = file
            .path
            .extension()
            .map(|value| format!(".{}", value.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_else(|| "(none)".to_string());
        *ext_counts.entry(extension).or_insert(0) += 1;

        if sample_files.len() < 10 {
            sample_files.push(SampleFile {
                name: file
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.path.display().to_string()),
                bytes: file.bytes,
                modified_at: file.modified_at,
            });
        }

        if let Some(timestamp) = file.modified_at {
            oldest = Some(oldest.map_or(timestamp, |value| value.min(timestamp)));
            newest = Some(newest.map_or(timestamp, |value| value.max(timestamp)));
        }

        if let Some(target) = compiled.targets.get(file.target_index) {
            let entry = target_stats.entry(target.root.clone()).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(file.bytes);
            entry.1 = entry.1.saturating_add(1);
        }
    }

    let mut ext_histogram = ext_counts.into_iter().collect::<Vec<_>>();
    ext_histogram.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ext_histogram.truncate(10);

    let target_summaries = compiled
        .targets
        .iter()
        .map(|target| {
            let (bytes, count) = target_stats.get(&target.root).copied().unwrap_or((0, 0));
            MatchedPath {
                template: target.template.clone(),
                resolved: target.root.clone(),
                bytes,
                file_count: count,
            }
        })
        .collect::<Vec<_>>();

    let evidence = build_evidence(
        rule,
        compiled,
        &target_summaries,
        allocated_bytes,
        file_count,
        oldest,
        newest,
        &ext_histogram,
        admin_measured,
    );

    Finding {
        id: Uuid::new_v4().to_string(),
        rule_id: rule.id.clone(),
        rule_version: rule.version,
        category: rule.category.clone(),
        title_zh: rule.title_zh.clone(),
        title_en: rule.title_en.clone(),
        risk: rule.risk,
        recoverability: rule.recoverability,
        requires_admin: rule.requires_admin,
        default_selected: rule.default_selected,
        targets: target_summaries,
        allocated_bytes,
        logical_bytes: allocated_bytes,
        file_count,
        size_provenance: if rule.requires_admin {
            if admin_measured {
                SizeProvenance::Helper
            } else {
                SizeProvenance::Estimated
            }
        } else if is_action_only(&compiled.action) {
            SizeProvenance::Estimated
        } else {
            SizeProvenance::Scanner
        },
        oldest_mtime: oldest,
        newest_mtime: newest,
        ext_histogram,
        sample_files,
        evidence,
        consequence: {
            let prefix = if rule.requires_admin {
                if admin_measured {
                    "管理员项已由提权 helper 扫描；执行时仍会重新扫描并校验。"
                } else {
                    "需要管理员权限；执行时由提权 helper 重新扫描并校验，预览大小可能不完整。"
                }
            } else if is_action_only(&compiled.action) {
                "该动作不基于文件匹配，会通过 Windows 系统界面或系统 API 完成；本工具不会绕过系统确认。"
            } else {
                ""
            };
            format!(
                "{prefix} {} {}",
                rule.explain.impact_zh, rule.explain.exclude_notes_zh
            )
            .trim()
            .to_string()
        },
        action: compiled.action.clone(),
        llm: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_evidence(
    rule: &crate::schema::Rule,
    compiled: &CompiledRule,
    targets: &[MatchedPath],
    bytes: u64,
    file_count: u64,
    oldest: Option<i64>,
    newest: Option<i64>,
    ext_histogram: &[(String, u64)],
    admin_measured: bool,
) -> Vec<Evidence> {
    let roots = compiled
        .targets
        .iter()
        .map(|target| target.root.display().to_string())
        .collect::<Vec<_>>()
        .join("; ");
    let action_only = is_action_only(&compiled.action);

    let mut evidence = Vec::new();
    evidence.push(Evidence {
        id: "E1".into(),
        source: EvidenceSource::Rule,
        kind: EvidenceKind::RuleProvenance,
        claim: format!(
            "命中内置规则 {} v{}（{} 核验）",
            rule.id,
            rule.version,
            rule.verified_at.as_deref().unwrap_or("未标注日期")
        ),
        path: None,
        bytes: None,
        timestamp: None,
        rule_id: Some(rule.id.clone()),
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E2".into(),
        source: EvidenceSource::AppDetection,
        kind: EvidenceKind::AppDetection,
        claim: if action_only {
            "该规则为系统引导或回收站动作，不扫描文件。".to_string()
        } else if rule.requires_admin {
            if admin_measured {
                format!("提权扫描检测到目标目录：{roots}")
            } else {
                format!("检测到目标目录（未提权，大小可能不完整）：{roots}")
            }
        } else {
            format!("检测到目标目录：{roots}")
        },
        path: compiled.targets.first().map(|target| target.root.clone()),
        bytes: None,
        timestamp: None,
        rule_id: Some(rule.id.clone()),
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E3".into(),
        source: EvidenceSource::Rule,
        kind: EvidenceKind::Path,
        claim: if action_only {
            "不会由规则直接删除文件；由 Windows 系统界面或系统 API 执行。".into()
        } else {
            "删除范围仅限于规则模板下匹配的文件，目录根本身不会被删除。".into()
        },
        path: compiled.targets.first().map(|target| target.root.clone()),
        bytes: None,
        timestamp: None,
        rule_id: Some(rule.id.clone()),
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E4".into(),
        source: EvidenceSource::Scanner,
        kind: EvidenceKind::Size,
        claim: if action_only {
            "该动作不基于文件匹配，大小以 Windows 系统界面为准。".to_string()
        } else {
            format!(
                "{}：{}，共 {} 个文件",
                if rule.requires_admin {
                    "提权前估算"
                } else {
                    "扫描器实测"
                },
                format_bytes(bytes),
                file_count
            )
        },
        path: None,
        bytes: Some(bytes),
        timestamp: None,
        rule_id: None,
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E5".into(),
        source: EvidenceSource::Scanner,
        kind: EvidenceKind::Age,
        claim: format!(
            "文件最后修改时间范围：{} 至 {}",
            oldest
                .map(format_timestamp)
                .unwrap_or_else(|| "未知".into()),
            newest
                .map(format_timestamp)
                .unwrap_or_else(|| "未知".into())
        ),
        path: None,
        bytes: None,
        timestamp: newest,
        rule_id: None,
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E6".into(),
        source: EvidenceSource::Scanner,
        kind: EvidenceKind::FileType,
        claim: format!(
            "文件类型分布：{}",
            format_extension_histogram(ext_histogram, file_count)
        ),
        path: None,
        bytes: None,
        timestamp: None,
        rule_id: None,
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E7".into(),
        source: EvidenceSource::Rule,
        kind: EvidenceKind::SafetyCheck,
        claim: "红线校验通过：未命中系统受保护目录、浏览器登录数据或聊天记录。".into(),
        path: None,
        bytes: None,
        timestamp: None,
        rule_id: Some(rule.id.clone()),
        confidence: 1.0,
    });
    evidence.push(Evidence {
        id: "E8".into(),
        source: EvidenceSource::Rule,
        kind: EvidenceKind::RuleProvenance,
        claim: recoverability_text(rule.recoverability),
        path: None,
        bytes: None,
        timestamp: None,
        rule_id: Some(rule.id.clone()),
        confidence: 1.0,
    });
    let _ = targets;
    evidence
}

fn recoverability_text(recoverability: Recoverability) -> String {
    match recoverability {
        Recoverability::Regenerable => "删除后应用会自动重建或重新下载；不是用户数据。".to_string(),
        Recoverability::RecycleBin => "删除进入系统回收站，可手动恢复。".to_string(),
        Recoverability::Quarantine => "删除进入应用隔离区，可在保留期内恢复。".to_string(),
        Recoverability::Irreversible => "该操作不可恢复，删除前请确认。".to_string(),
        Recoverability::UserData => "包含用户数据，规则不应删除此类内容。".to_string(),
    }
}

fn format_extension_histogram(histogram: &[(String, u64)], total: u64) -> String {
    if histogram.is_empty() {
        return "无文件".into();
    }
    let total = total.max(1);
    histogram
        .iter()
        .take(5)
        .map(|(extension, count)| {
            format!(
                "{} {}%",
                extension,
                (*count as f64 / total as f64 * 100.0).round() as u64
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_timestamp(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "未知".into())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
