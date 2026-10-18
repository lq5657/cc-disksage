use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::Utc;
use disksage_clean::{
    AuditLog, CleanError, ExecutionOptions, PlanCandidate, create_plan, execute_plan,
};
use disksage_core::{ActionSpec, CleanupResult, EnvMap, Finding, Recoverability, RiskLevel};
use disksage_rules::{load_builtin_rules, quick_scan};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const TASK_NAME: &str = "CC DiskSage Auto Clean";
const RUN_TIME: &str = "12:00";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoCleanConfig {
    pub enabled: bool,
    pub frequency: String,
    pub min_free_percent: u8,
    pub permanent_delete: bool,
    pub last_run: Option<i64>,
    pub last_freed_bytes: u64,
    pub last_deleted: u64,
    pub last_status: String,
}

impl Default for AutoCleanConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency: "weekly".into(),
            min_free_percent: 10,
            permanent_delete: false,
            last_run: None,
            last_freed_bytes: 0,
            last_deleted: 0,
            last_status: "never".into(),
        }
    }
}

impl AutoCleanConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.frequency != "daily" && self.frequency != "weekly" {
            return Err("frequency must be daily or weekly".into());
        }
        if self.min_free_percent > 50 {
            return Err("min_free_percent must be <= 50".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct AutoCleanStatus {
    pub config: AutoCleanConfig,
    pub task_registered: bool,
    pub task_name: String,
    pub executable: Option<String>,
}

pub fn config_path() -> PathBuf {
    disksage_core::paths::auto_clean_config_path()
}

pub fn load_config() -> Result<AutoCleanConfig, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(AutoCleanConfig::default());
    }
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let config: AutoCleanConfig = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    config.validate()?;
    Ok(config)
}

pub fn save_config(config: &AutoCleanConfig) -> Result<(), String> {
    config.validate()?;
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| error.to_string())
}

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn output_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    };
    text.chars().take(500).collect()
}

pub fn task_exists() -> bool {
    hidden_command("schtasks.exe")
        .args(["/Query", "/TN", TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn build_task_args(exe: &Path, frequency: &str) -> Vec<String> {
    let run = format!("\"{}\" --auto-clean", exe.display());
    let mut args = vec![
        "/Create".to_string(),
        "/TN".to_string(),
        TASK_NAME.to_string(),
        "/TR".to_string(),
        run,
        "/SC".to_string(),
        if frequency == "daily" {
            "DAILY".to_string()
        } else {
            "WEEKLY".to_string()
        },
        "/ST".to_string(),
        RUN_TIME.to_string(),
        "/F".to_string(),
    ];
    if frequency != "daily" {
        args.push("/D".to_string());
        args.push("SUN".to_string());
    }
    args
}

pub fn register_task(config: &AutoCleanConfig) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    register_task_for(&exe, config)
}

fn register_task_for(exe: &Path, config: &AutoCleanConfig) -> Result<(), String> {
    let args = build_task_args(exe, &config.frequency);
    let output = hidden_command("schtasks.exe")
        .args(&args)
        .output()
        .map_err(|error| format!("failed to run schtasks: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("创建计划任务失败：{}", output_message(&output)))
    }
}

pub fn delete_task() -> Result<(), String> {
    if !task_exists() {
        return Ok(());
    }
    let output = hidden_command("schtasks.exe")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .output()
        .map_err(|error| format!("failed to run schtasks: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("删除计划任务失败：{}", output_message(&output)))
    }
}

pub fn status() -> Result<AutoCleanStatus, String> {
    let config = load_config()?;
    Ok(AutoCleanStatus {
        config,
        task_registered: task_exists(),
        task_name: TASK_NAME.to_string(),
        executable: std::env::current_exe()
            .ok()
            .map(|path| path.display().to_string()),
    })
}

pub fn set_config(mut config: AutoCleanConfig) -> Result<AutoCleanStatus, String> {
    config.validate()?;
    // The UI edits the schedule only; never let a stale status snapshot
    // overwrite the most recent run result.
    if let Ok(existing) = load_config() {
        config.last_run = existing.last_run;
        config.last_freed_bytes = existing.last_freed_bytes;
        config.last_deleted = existing.last_deleted;
        config.last_status = existing.last_status;
    }
    if config.enabled {
        register_task(&config)?;
    } else {
        delete_task()?;
    }
    save_config(&config)?;
    status()
}

pub fn is_auto_candidate(finding: &Finding) -> bool {
    finding.risk == RiskLevel::Safe
        && finding.default_selected
        && !finding.requires_admin
        && finding.recoverability == Recoverability::Regenerable
        && matches!(&finding.action, ActionSpec::Trash | ActionSpec::Delete)
}

fn append_log(result: &CleanupResult, status: &str) {
    let path = disksage_core::paths::logs_dir().join("auto-clean.jsonl");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let record = serde_json::json!({
        "timestamp": Utc::now().timestamp(),
        "status": status,
        "freed_bytes": result.freed_bytes,
        "deleted": result.deleted,
        "failed": result.failed.len(),
        "skipped_locked": result.skipped_locked.len(),
        "notes": result.notes,
    });
    if let Ok(line) = serde_json::to_string(&record)
        && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path)
    {
        let _ = writeln!(file, "{line}");
    }
}

pub fn log_error(message: &str) {
    let result = CleanupResult {
        job_id: Uuid::new_v4().to_string(),
        notes: vec![message.to_string()],
        ..CleanupResult::default()
    };
    append_log(&result, "error");
}

/// Headless failures should be visible in the settings page as well as the log.
pub fn mark_headless_failure(message: &str) {
    log_error(message);
    if config_path().exists()
        && let Ok(mut config) = load_config()
    {
        config.last_run = Some(Utc::now().timestamp());
        config.last_status = "error".into();
        let _ = save_config(&config);
    }
}

fn finish_config(
    config: &mut AutoCleanConfig,
    result: &CleanupResult,
    status: &str,
) -> Result<(), String> {
    config.last_run = Some(Utc::now().timestamp());
    config.last_freed_bytes = result.freed_bytes;
    config.last_deleted = result.deleted;
    config.last_status = status.to_string();
    save_config(config)
}

/// Run safe, default, regenerable, non-admin cleanup. In headless mode the
/// low-free-space threshold is enforced; `force` (UI button) ignores it.
pub fn run_auto_clean(force: bool) -> Result<CleanupResult, String> {
    let mut config = load_config()?;
    if !config.enabled && !force {
        return Err("自动清理未启用".into());
    }

    let system_drive = std::env::var("SystemDrive")
        .ok()
        .and_then(|value| value.chars().next())
        .unwrap_or('C')
        .to_ascii_uppercase();

    if !force
        && config.min_free_percent > 0
        && let Some(volume) = disksage_scan::enumerate_volumes()
            .into_iter()
            .find(|volume| volume.letter == system_drive)
        && volume.total_bytes > 0
    {
        let free_percent = volume.free_bytes as f64 / volume.total_bytes as f64 * 100.0;
        if free_percent >= f64::from(config.min_free_percent) {
            let result = CleanupResult {
                job_id: Uuid::new_v4().to_string(),
                notes: vec![format!(
                    "剩余空间 {free_percent:.1}% 高于阈值 {}%，跳过自动清理",
                    config.min_free_percent
                )],
                ..CleanupResult::default()
            };
            finish_config(&mut config, &result, "skipped")?;
            append_log(&result, "skipped");
            return Ok(result);
        }
    }

    let env: EnvMap = std::env::vars().collect();
    let pack = load_builtin_rules().map_err(|error| error.to_string())?;
    let scan = quick_scan(&pack, &env, false).map_err(|error| error.to_string())?;

    let mut candidates = Vec::new();
    for finding in scan
        .findings
        .iter()
        .filter(|finding| is_auto_candidate(finding))
    {
        let files = scan.matches.get(&finding.id).cloned().unwrap_or_default();
        if files.is_empty() {
            continue;
        }
        candidates.push(PlanCandidate {
            finding_id: finding.id.clone(),
            rule_id: finding.rule_id.clone(),
            title_zh: finding.title_zh.clone(),
            risk: finding.risk,
            recoverability: finding.recoverability,
            requires_admin: false,
            action: if config.permanent_delete {
                ActionSpec::Delete
            } else {
                // Auto mode defaults to the recycle bin, even if a rule
                // requested permanent deletion.
                ActionSpec::Trash
            },
            files,
            allowed_roots: finding
                .targets
                .iter()
                .map(|target| target.resolved.clone())
                .collect(),
        });
    }

    let mut result = if candidates.is_empty() {
        CleanupResult {
            job_id: Uuid::new_v4().to_string(),
            notes: vec!["没有需要自动清理的安全项".into()],
            ..CleanupResult::default()
        }
    } else {
        match create_plan(candidates, &env) {
            Ok(plan) => {
                let audit = AuditLog::new(AuditLog::default_location());
                execute_plan(
                    &plan,
                    &env,
                    ExecutionOptions {
                        confirm: true,
                        dry_run: false,
                        elevation: false,
                    },
                    Some(&audit),
                )
                .map_err(|error| error.to_string())?
            }
            Err(CleanError::EmptyPlan) => CleanupResult {
                job_id: Uuid::new_v4().to_string(),
                notes: vec!["没有需要自动清理的安全项".into()],
                ..CleanupResult::default()
            },
            Err(error) => return Err(error.to_string()),
        }
    };

    result.notes.insert(
        0,
        if config.permanent_delete {
            "自动清理：仅安全默认规则，永久删除可重建缓存".into()
        } else {
            "自动清理：仅安全默认规则，删除到回收站".into()
        },
    );
    let status = if result.failed.is_empty() {
        "success"
    } else {
        "partial"
    };
    finish_config(&mut config, &result, status)?;
    append_log(&result, status);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_args_for(frequency: &str) -> Vec<String> {
        build_task_args(
            Path::new(r"C:\Program Files\CC DiskSage\cc-disksage.exe"),
            frequency,
        )
    }

    #[test]
    fn builds_daily_and_weekly_task_args() {
        let daily = task_args_for("daily");
        assert!(daily.contains(&"/SC".to_string()));
        assert!(daily.contains(&"DAILY".to_string()));
        assert!(!daily.contains(&"/D".to_string()));

        let weekly = task_args_for("weekly");
        assert!(weekly.contains(&"WEEKLY".to_string()));
        assert!(weekly.contains(&"SUN".to_string()));
        assert!(weekly.iter().any(|arg| arg.contains("--auto-clean")));
    }

    #[test]
    fn validates_config() {
        let monthly = AutoCleanConfig {
            frequency: "monthly".into(),
            ..AutoCleanConfig::default()
        };
        assert!(monthly.validate().is_err());

        let too_high = AutoCleanConfig {
            frequency: "daily".into(),
            min_free_percent: 51,
            ..AutoCleanConfig::default()
        };
        assert!(too_high.validate().is_err());
    }

    fn finding() -> Finding {
        Finding {
            id: "f_test".into(),
            rule_id: "test.rule".into(),
            rule_version: 1,
            category: "test".into(),
            title_zh: "test".into(),
            title_en: "test".into(),
            risk: RiskLevel::Safe,
            recoverability: Recoverability::Regenerable,
            requires_admin: false,
            default_selected: true,
            targets: Vec::new(),
            allocated_bytes: 0,
            logical_bytes: 0,
            file_count: 0,
            size_provenance: disksage_core::SizeProvenance::Estimated,
            oldest_mtime: None,
            newest_mtime: None,
            ext_histogram: Vec::new(),
            sample_files: Vec::new(),
            evidence: Vec::new(),
            consequence: String::new(),
            action: ActionSpec::Trash,
            llm: None,
        }
    }

    fn decode_task_xml(bytes: &[u8]) -> String {
        if bytes.starts_with(&[0xFF, 0xFE]) {
            let (pairs, _) = bytes[2..].as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
            String::from_utf16_lossy(&units)
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }

    #[test]
    #[ignore = "creates and removes a real Task Scheduler task; run manually"]
    fn scheduled_task_roundtrip_with_spaced_path() {
        let config = AutoCleanConfig {
            frequency: "daily".into(),
            ..AutoCleanConfig::default()
        };
        let dir = std::env::temp_dir().join("CC DiskSage Probe");
        std::fs::create_dir_all(&dir).expect("create probe dir");
        let exe = dir.join("cc-disksage.exe");
        std::fs::write(&exe, b"probe").expect("write probe exe");

        register_task_for(&exe, &config).expect("register task");
        assert!(task_exists());

        let output = hidden_command("schtasks.exe")
            .args(["/Query", "/TN", TASK_NAME, "/XML"])
            .output()
            .expect("query task");
        let xml = decode_task_xml(&output.stdout);
        assert!(
            xml.contains("CC DiskSage Probe"),
            "task XML lost the executable path: {xml}"
        );
        assert!(
            xml.contains("--auto-clean"),
            "task XML lost the auto-clean argument: {xml}"
        );

        delete_task().expect("delete task");
        assert!(!task_exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "read-only real quick scan of the current user's caches"]
    fn reports_auto_candidate_rules_read_only() {
        let env: EnvMap = std::env::vars().collect();
        let pack = load_builtin_rules().expect("built-in rules");
        let scan = quick_scan(&pack, &env, false).expect("quick scan");
        for finding in scan
            .findings
            .iter()
            .filter(|finding| is_auto_candidate(finding))
        {
            println!(
                "{} files={} bytes={}",
                finding.rule_id, finding.file_count, finding.allocated_bytes
            );
        }
    }

    #[test]
    fn auto_candidates_are_safe_default_non_admin_regenerable() {
        assert!(is_auto_candidate(&finding()));

        let mut caution = finding();
        caution.risk = RiskLevel::Caution;
        assert!(!is_auto_candidate(&caution));

        let mut admin = finding();
        admin.requires_admin = true;
        assert!(!is_auto_candidate(&admin));

        let mut unselected = finding();
        unselected.default_selected = false;
        assert!(!is_auto_candidate(&unselected));

        let mut recycle = finding();
        recycle.recoverability = Recoverability::RecycleBin;
        assert!(!is_auto_candidate(&recycle));

        let mut guided = finding();
        guided.action = ActionSpec::Guided {
            guide: disksage_core::GuideId::CleanMgr,
        };
        assert!(!is_auto_candidate(&guided));
    }
}
