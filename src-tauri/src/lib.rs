use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use disksage_clean::{
    AuditLog, ExecutionOptions, PlanCandidate, create_plan as build_plan, execute_plan as run_plan,
};
use disksage_core::{
    ActionSpec, CleanupPlan, CleanupResult, DirNode, Finding, GuideId, LlmAssessment, PlanWarning,
    Recoverability, RiskLevel, ScanSnapshot, VolumeInfo,
};
use disksage_llm::{
    LlmClient, LlmConfig, LlmPreset, PRESETS, RedactContext, build_finding_payload,
    config::default_config_path, delete_key, get_key, payload_json, set_key,
};
use disksage_rules::{load_builtin_rules, load_user_rules, quick_scan as run_quick_scan};
use disksage_scan::{CancelToken, ScanRequest, scan_volume as run_scan};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone)]
struct StoredFinding {
    finding: Finding,
    #[allow(dead_code)]
    files: Vec<PathBuf>,
}

#[derive(Default)]
struct AppState {
    findings: Mutex<HashMap<String, StoredFinding>>,
    plans: Mutex<HashMap<String, CleanupPlan>>,
    assessments: Mutex<HashMap<String, LlmAssessment>>,
    snapshots: Mutex<HashMap<String, ScanSnapshot>>,
    scan_cancel: Mutex<Option<CancelToken>>,
}

#[derive(Serialize)]
struct TreemapNode {
    name: String,
    path: String,
    value: u64,
    file_count: u64,
    children: Vec<TreemapNode>,
}

#[derive(Serialize)]
struct TreemapResponse {
    root: TreemapNode,
    total_bytes: u64,
    file_count: u64,
    truncated: bool,
}

fn prune_snapshot(snapshot: &ScanSnapshot) -> ScanSnapshot {
    let mut summary = snapshot.clone();
    summary.top_dirs = snapshot
        .top_dirs
        .iter()
        .take(50)
        .map(|node| prune_node(node, 0))
        .collect();
    summary.large_files.truncate(200);
    summary
}

fn prune_node(node: &DirNode, depth: usize) -> DirNode {
    let mut copy = node.clone();
    if depth < 2 {
        copy.children = node
            .children
            .iter()
            .take(100)
            .map(|child| prune_node(child, depth + 1))
            .collect();
    } else {
        copy.children.clear();
    }
    copy
}

fn to_treemap(
    node: &DirNode,
    depth: usize,
    budget: &mut usize,
    truncated: &mut bool,
) -> TreemapNode {
    *budget = budget.saturating_sub(1);
    let mut children = Vec::new();
    if depth < 2 && *budget > 0 {
        for child in node.children.iter().take(50) {
            if *budget == 0 {
                *truncated = true;
                break;
            }
            children.push(to_treemap(child, depth + 1, budget, truncated));
        }
        if node.children.len() > 50 {
            *truncated = true;
        }
    } else if !node.children.is_empty() {
        *truncated = true;
    }
    TreemapNode {
        name: node.name.clone(),
        path: node.path.display().to_string(),
        value: node.allocated_bytes.max(1),
        file_count: node.file_count,
        children,
    }
}

#[derive(Serialize)]
struct LlmConfigPublic {
    provider_id: String,
    base_url: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    timeout_secs: u64,
    redact_paths: bool,
    send_sample_names: bool,
    sample_name_limit: usize,
    has_key: bool,
    presets: Vec<LlmPreset>,
}

fn public_llm_config(config: &LlmConfig, has_key: bool) -> LlmConfigPublic {
    LlmConfigPublic {
        provider_id: config.provider_id.clone(),
        base_url: config.base_url.clone(),
        model: config.model.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        timeout_secs: config.timeout_secs,
        redact_paths: config.redact_paths,
        send_sample_names: config.send_sample_names,
        sample_name_limit: config.sample_name_limit,
        has_key,
        presets: PRESETS.to_vec(),
    }
}

#[derive(Serialize)]
struct PlanPreviewItem {
    finding_id: String,
    rule_id: String,
    title_zh: String,
    risk: RiskLevel,
    bytes: u64,
    file_count: u64,
    recoverability: Recoverability,
    requires_admin: bool,
    action: String,
}

fn action_kind(action: &ActionSpec) -> &'static str {
    match action {
        ActionSpec::Trash | ActionSpec::Delete => "file",
        ActionSpec::EmptyRecycleBin => "recycle_bin",
        ActionSpec::Guided { .. } => "guided",
        ActionSpec::ServiceCleanup { .. } => "service_cleanup",
        ActionSpec::DismAnalyze => "dism_analyze",
        ActionSpec::DismStartComponentCleanup => "dism_component_cleanup",
        ActionSpec::HibernateOff => "hibernate_off",
        ActionSpec::Quarantine => "quarantine",
        ActionSpec::CreateRestorePoint => "create_restore_point",
    }
}

#[derive(Serialize)]
struct PlanPreview {
    plan_id: String,
    total_bytes: u64,
    file_count: u64,
    items: Vec<PlanPreviewItem>,
    warnings: Vec<PlanWarning>,
}

fn user_rules_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|base| base.join("cc-disksage").join("rules"))
}

#[tauri::command]
fn get_volumes() -> Vec<VolumeInfo> {
    disksage_scan::enumerate_volumes()
}

#[tauri::command]
async fn scan_volume(
    state: tauri::State<'_, AppState>,
    volume: String,
    on_progress: tauri::ipc::Channel<disksage_scan::ScanProgress>,
) -> Result<ScanSnapshot, String> {
    let letter = volume.trim().trim_end_matches(':').to_ascii_uppercase();
    if letter.len() != 1
        || !letter
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return Err("invalid volume letter".to_string());
    }

    let root = format!("{letter}:\\");
    let cancel = CancelToken::new();
    *state
        .scan_cancel
        .lock()
        .map_err(|_| "scan cancel lock poisoned".to_string())? = Some(cancel.clone());

    let scan_result = tauri::async_runtime::spawn_blocking(move || {
        let request = ScanRequest::new(root);
        run_scan(
            request,
            move |progress| {
                let _ = on_progress.send(progress);
            },
            cancel,
        )
    })
    .await
    .map_err(|error| format!("scan task failed: {error}"));

    if let Ok(mut guard) = state.scan_cancel.lock() {
        *guard = None;
    }

    let snapshot = scan_result?.map_err(|error| error.to_string())?;
    let summary = prune_snapshot(&snapshot);
    state
        .snapshots
        .lock()
        .map_err(|_| "snapshot store lock poisoned".to_string())?
        .insert(snapshot.id.clone(), snapshot);
    Ok(summary)
}

#[tauri::command]
fn cancel_scan(state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(token) = state
        .scan_cancel
        .lock()
        .map_err(|_| "scan cancel lock poisoned".to_string())?
        .as_ref()
    {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
fn get_treemap(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
) -> Result<TreemapResponse, String> {
    let snapshots = state
        .snapshots
        .lock()
        .map_err(|_| "snapshot store lock poisoned".to_string())?;
    let snapshot = snapshots
        .get(&snapshot_id)
        .ok_or_else(|| "snapshot not found; run a full scan first".to_string())?;

    let mut budget = 2500usize;
    let mut truncated = false;
    let children = snapshot
        .top_dirs
        .iter()
        .map(|node| to_treemap(node, 0, &mut budget, &mut truncated))
        .collect();

    Ok(TreemapResponse {
        root: TreemapNode {
            name: format!("{}: 空间分布", snapshot.volume),
            path: format!("{}:\\", snapshot.volume),
            value: snapshot.allocated_bytes.max(1),
            file_count: snapshot.file_count,
            children,
        },
        total_bytes: snapshot.allocated_bytes,
        file_count: snapshot.file_count,
        truncated,
    })
}

#[tauri::command]
async fn quick_scan(state: tauri::State<'_, AppState>) -> Result<Vec<Finding>, String> {
    let result = tauri::async_runtime::spawn_blocking(|| -> Result<_, String> {
        let env: HashMap<String, String> = std::env::vars().collect();
        let mut pack = load_builtin_rules().map_err(|error| error.to_string())?;
        if let Some(directory) = user_rules_dir() {
            let user = load_user_rules(&directory).map_err(|error| error.to_string())?;
            pack.rules.extend(user.rules);
        }
        run_quick_scan(&pack, &env, true).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("quick scan task failed: {error}"))??;

    let mut store = state
        .findings
        .lock()
        .map_err(|_| "finding store lock poisoned".to_string())?;
    store.clear();
    for finding in &result.findings {
        store.insert(
            finding.id.clone(),
            StoredFinding {
                finding: finding.clone(),
                files: result.matches.get(&finding.id).cloned().unwrap_or_default(),
            },
        );
    }

    Ok(result.findings)
}

#[tauri::command]
fn get_finding(state: tauri::State<'_, AppState>, finding_id: String) -> Result<Finding, String> {
    state
        .findings
        .lock()
        .map_err(|_| "finding store lock poisoned".to_string())?
        .get(&finding_id)
        .map(|stored| stored.finding.clone())
        .ok_or_else(|| "finding not found; run a quick scan first".to_string())
}

#[tauri::command]
async fn create_plan(
    state: tauri::State<'_, AppState>,
    finding_ids: Vec<String>,
) -> Result<PlanPreview, String> {
    let candidates = {
        let store = state
            .findings
            .lock()
            .map_err(|_| "finding store lock poisoned".to_string())?;
        let mut candidates = Vec::new();
        for finding_id in finding_ids {
            let Some(stored) = store.get(&finding_id) else {
                continue;
            };
            let finding = &stored.finding;
            candidates.push(PlanCandidate {
                finding_id: finding.id.clone(),
                rule_id: finding.rule_id.clone(),
                title_zh: finding.title_zh.clone(),
                risk: finding.risk,
                recoverability: finding.recoverability,
                requires_admin: finding.requires_admin,
                action: finding.action.clone(),
                files: stored.files.clone(),
                allowed_roots: finding
                    .targets
                    .iter()
                    .map(|target| target.resolved.clone())
                    .collect(),
            });
        }
        candidates
    };

    if candidates.is_empty() {
        return Err("没有可用的清理项，请先重新扫描".to_string());
    }

    let env: HashMap<String, String> = std::env::vars().collect();
    let plan = tauri::async_runtime::spawn_blocking(move || build_plan(candidates, &env))
        .await
        .map_err(|error| format!("plan task failed: {error}"))?
        .map_err(|error| error.to_string())?;

    let preview = PlanPreview {
        plan_id: plan.id.clone(),
        total_bytes: plan.total_allocated_bytes,
        file_count: plan.items.iter().map(|item| item.file_count).sum(),
        items: plan
            .items
            .iter()
            .map(|item| PlanPreviewItem {
                finding_id: item.finding_id.clone(),
                rule_id: item.rule_id.clone(),
                title_zh: item.title_zh.clone(),
                risk: item.risk,
                bytes: item.estimated_bytes,
                file_count: item.file_count,
                recoverability: item.recoverability,
                requires_admin: item.requires_admin,
                action: action_kind(&item.action).to_string(),
            })
            .collect(),
        warnings: plan.warnings.clone(),
    };

    state
        .plans
        .lock()
        .map_err(|_| "plan store lock poisoned".to_string())?
        .insert(plan.id.clone(), plan);

    Ok(preview)
}

#[tauri::command]
async fn execute_plan(
    state: tauri::State<'_, AppState>,
    plan_id: String,
) -> Result<CleanupResult, String> {
    let plan = state
        .plans
        .lock()
        .map_err(|_| "plan store lock poisoned".to_string())?
        .remove(&plan_id)
        .ok_or_else(|| "plan not found; create a new preview".to_string())?;

    let mut normal_items = Vec::new();
    let mut admin_items = Vec::new();
    let mut guided = Vec::new();
    for item in plan.items {
        match &item.action {
            ActionSpec::Guided { guide } => guided.push(*guide),
            _ => {
                if item.requires_admin {
                    admin_items.push(item);
                } else {
                    normal_items.push(item);
                }
            }
        }
    }

    let env: HashMap<String, String> = std::env::vars().collect();
    let mut result = CleanupResult {
        job_id: Uuid::new_v4().to_string(),
        ..CleanupResult::default()
    };

    if !normal_items.is_empty() {
        let normal_plan = CleanupPlan {
            id: plan.id.clone(),
            snapshot_id: plan.snapshot_id.clone(),
            created_at: plan.created_at,
            items: normal_items,
            total_allocated_bytes: plan.total_allocated_bytes,
            requires_admin: false,
            warnings: plan.warnings.clone(),
            status: plan.status,
        };
        let normal_env = env.clone();
        let normal_result = tauri::async_runtime::spawn_blocking(move || {
            let audit = AuditLog::new(AuditLog::default_location());
            run_plan(
                &normal_plan,
                &normal_env,
                ExecutionOptions {
                    confirm: true,
                    dry_run: false,
                    elevation: false,
                },
                Some(&audit),
            )
        })
        .await
        .map_err(|error| format!("cleanup task failed: {error}"))?
        .map_err(|error| error.to_string())?;
        merge_cleanup_result(&mut result, normal_result);
    }

    if !admin_items.is_empty() {
        let rule_ids = admin_items
            .iter()
            .map(|item| item.rule_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        match run_admin_job(rule_ids).await {
            Ok(admin_result) => merge_cleanup_result(&mut result, admin_result),
            Err(error) => result.notes.push(format!("管理员清理未执行：{error}")),
        }
    }

    for guide in guided {
        match open_guide(guide) {
            Ok(()) => result
                .notes
                .push(format!("已打开系统设置：{}", guide_label_zh(guide))),
            Err(error) => result
                .notes
                .push(format!("打开 {} 失败：{error}", guide_label_zh(guide))),
        }
    }

    Ok(result)
}

fn open_guide(guide: GuideId) -> Result<(), String> {
    let windows = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let system32 = windows.join("System32");
    let (program, args): (&str, &[&str]) = match guide {
        GuideId::WindowsStorageSettings => ("explorer.exe", &["ms-settings:storagesense"]),
        GuideId::WindowsUpdateSettings => ("explorer.exe", &["ms-settings:windowsupdate"]),
        GuideId::SystemProtection => ("SystemPropertiesProtection.exe", &[]),
        GuideId::PagefileSettings => ("SystemPropertiesAdvanced.exe", &[]),
        GuideId::CleanMgr => ("cleanmgr.exe", &[]),
    };
    let executable = if program.eq_ignore_ascii_case("explorer.exe") {
        windows.join(program)
    } else {
        system32.join(program)
    };
    std::process::Command::new(executable)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn guide_label_zh(guide: GuideId) -> &'static str {
    match guide {
        GuideId::WindowsStorageSettings => "存储设置",
        GuideId::WindowsUpdateSettings => "Windows 更新设置",
        GuideId::SystemProtection => "系统保护（还原点）",
        GuideId::PagefileSettings => "系统属性 → 高级 → 虚拟内存",
        GuideId::CleanMgr => "磁盘清理（Windows.old）",
    }
}

fn merge_cleanup_result(target: &mut CleanupResult, mut partial: CleanupResult) {
    target.freed_bytes = target.freed_bytes.saturating_add(partial.freed_bytes);
    target.deleted = target.deleted.saturating_add(partial.deleted);
    target.skipped_locked.append(&mut partial.skipped_locked);
    target.failed.append(&mut partial.failed);
    target.cancelled |= partial.cancelled;
    target.notes.append(&mut partial.notes);
}

fn jobs_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("cc-disksage")
        .join("jobs")
}

fn helper_executable() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let directory = current.parent()?;
    for name in [
        "disksage-helper.exe",
        "disksage-helper-x86_64-pc-windows-msvc.exe",
    ] {
        let candidate = directory.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    for profile in ["debug", "release"] {
        let candidate = workspace
            .join("target")
            .join(profile)
            .join("disksage-helper.exe");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[derive(Serialize)]
struct HelperJobFile {
    task: String,
    job_id: String,
    nonce: String,
    expires_at: i64,
    rule_ids: Vec<String>,
}

async fn run_admin_job(rule_ids: Vec<String>) -> Result<CleanupResult, String> {
    let helper = helper_executable()
        .ok_or_else(|| "未找到 disksage-helper.exe（请重新安装或构建 helper）".to_string())?;
    let jobs = jobs_directory();
    std::fs::create_dir_all(&jobs).map_err(|error| error.to_string())?;

    let job_id = Uuid::new_v4().to_string();
    let job_path = jobs.join(format!("{job_id}.json"));
    let result_path = job_path.with_extension("result.json");
    let _ = std::fs::remove_file(&result_path);

    let job = HelperJobFile {
        task: "clean".into(),
        job_id: job_id.clone(),
        nonce: Uuid::new_v4().to_string(),
        expires_at: chrono::Utc::now().timestamp() + 600,
        rule_ids,
    };
    let text = serde_json::to_string_pretty(&job).map_err(|error| error.to_string())?;
    std::fs::write(&job_path, text).map_err(|error| error.to_string())?;

    let helper_for_launch = helper.clone();
    let job_for_launch = job_path.clone();
    let launch = tauri::async_runtime::spawn_blocking(move || {
        launch_elevated_helper(&helper_for_launch, &job_for_launch)
    })
    .await
    .map_err(|error| format!("helper launch task failed: {error}"))?;

    if !result_path.exists() {
        return Err(match launch {
            Ok(()) => "提权 helper 未产生结果文件".into(),
            Err(error) => error,
        });
    }

    let text = std::fs::read_to_string(&result_path).map_err(|error| error.to_string())?;
    let mut result: CleanupResult =
        serde_json::from_str(&text).map_err(|error| format!("无效的 helper 结果：{error}"))?;
    if let Err(error) = launch {
        result.notes.push(format!("helper 启动提示：{error}"));
    }
    Ok(result)
}

#[cfg(windows)]
fn launch_elevated_helper(helper: &std::path::Path, job: &std::path::Path) -> Result<(), String> {
    let quote =
        |path: &std::path::Path| format!("'{}'", path.display().to_string().replace('\'', "''"));
    let script = format!(
        "try {{ $p = Start-Process -FilePath {} -ArgumentList '--job',{} -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop; exit $p.ExitCode }} catch {{ Write-Error $_.Exception.Message; exit 1223 }}",
        quote(helper),
        quote(job)
    );
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|error| format!("无法启动提权 helper：{error}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let lower = stderr.to_ascii_lowercase();
    if output.status.code() == Some(1223)
        || lower.contains("canceled")
        || lower.contains("cancelled")
        || stderr.contains("取消")
    {
        Err("用户取消了管理员授权".into())
    } else {
        Err(format!(
            "提权 helper 失败（退出码 {}）：{}",
            output.status.code().unwrap_or(-1),
            stderr
        ))
    }
}

#[cfg(not(windows))]
fn launch_elevated_helper(_helper: &std::path::Path, _job: &std::path::Path) -> Result<(), String> {
    Err("elevated cleanup is only supported on Windows".into())
}

fn stored_finding(state: &AppState, finding_id: &str) -> Result<Finding, String> {
    state
        .findings
        .lock()
        .map_err(|_| "finding store lock poisoned".to_string())?
        .get(finding_id)
        .map(|stored| stored.finding.clone())
        .ok_or_else(|| "finding not found; run a quick scan first".to_string())
}

#[tauri::command]
fn get_audit_history(limit: usize) -> Result<Vec<serde_json::Value>, String> {
    let path = AuditLog::default_location();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let limit = limit.clamp(1, 200);
    let lines = text.lines().rev().take(limit).collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(lines.len());
    for line in lines.into_iter().rev() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            entries.push(value);
        }
    }
    Ok(entries)
}

#[tauri::command]
fn llm_get_config() -> Result<LlmConfigPublic, String> {
    let config = LlmConfig::load(&default_config_path()).map_err(|error| error.to_string())?;
    let has_key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .is_some();
    Ok(public_llm_config(&config, has_key))
}

#[tauri::command]
fn llm_set_config(config: LlmConfig) -> Result<LlmConfigPublic, String> {
    let path = default_config_path();
    config.save(&path).map_err(|error| error.to_string())?;
    let has_key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .is_some();
    Ok(public_llm_config(&config, has_key))
}

#[tauri::command]
fn llm_set_key(provider_id: String, api_key: String) -> Result<(), String> {
    set_key(&provider_id, &api_key).map_err(|error| error.to_string())
}

#[tauri::command]
fn llm_clear_key(provider_id: String) -> Result<(), String> {
    delete_key(&provider_id).map_err(|error| error.to_string())
}

#[tauri::command]
async fn llm_test_connection() -> Result<(), String> {
    let config = LlmConfig::load(&default_config_path()).map_err(|error| error.to_string())?;
    let key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中保存 API Key".to_string())?;
    let client = LlmClient::new(config, key).map_err(|error| error.to_string())?;
    client
        .test_connection()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn llm_preview_payload(
    state: tauri::State<'_, AppState>,
    finding_id: String,
) -> Result<serde_json::Value, String> {
    let finding = stored_finding(&state, &finding_id)?;
    let config = LlmConfig::load(&default_config_path()).map_err(|error| error.to_string())?;
    let redact = RedactContext::from_env(config.redact_paths);
    let payload = build_finding_payload(&finding, &config, &redact);
    Ok(payload_json(&payload))
}

#[tauri::command]
async fn llm_assess_finding(
    state: tauri::State<'_, AppState>,
    finding_id: String,
) -> Result<LlmAssessment, String> {
    let finding = stored_finding(&state, &finding_id)?;
    let config = LlmConfig::load(&default_config_path()).map_err(|error| error.to_string())?;
    let key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中保存 API Key".to_string())?;
    let client = LlmClient::new(config, key).map_err(|error| error.to_string())?;
    let assessment = client
        .assess_finding(&finding)
        .await
        .map_err(|error| error.to_string())?;
    state
        .assessments
        .lock()
        .map_err(|_| "assessment store lock poisoned".to_string())?
        .insert(finding_id, assessment.clone());
    Ok(assessment)
}

#[tauri::command]
async fn llm_ask_followup(
    state: tauri::State<'_, AppState>,
    finding_id: String,
    question: String,
) -> Result<String, String> {
    let finding = stored_finding(&state, &finding_id)?;
    let assessment = state
        .assessments
        .lock()
        .map_err(|_| "assessment store lock poisoned".to_string())?
        .get(&finding_id)
        .cloned()
        .ok_or_else(|| "请先对该项进行 AI 分析".to_string())?;
    let config = LlmConfig::load(&default_config_path()).map_err(|error| error.to_string())?;
    let key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中保存 API Key".to_string())?;
    let client = LlmClient::new(config, key).map_err(|error| error.to_string())?;
    client
        .ask_followup(&finding, &assessment, &question)
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_volumes,
            scan_volume,
            cancel_scan,
            get_treemap,
            quick_scan,
            get_finding,
            create_plan,
            execute_plan,
            get_audit_history,
            llm_get_config,
            llm_set_config,
            llm_set_key,
            llm_clear_key,
            llm_test_connection,
            llm_preview_payload,
            llm_assess_finding,
            llm_ask_followup
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn node(name: &str, bytes: u64, children: Vec<DirNode>) -> DirNode {
        DirNode {
            name: name.into(),
            path: PathBuf::from(format!(r"C:\{name}")),
            allocated_bytes: bytes,
            logical_bytes: bytes,
            file_count: 1,
            children,
        }
    }

    #[test]
    fn prune_node_stops_at_depth_two() {
        let leaf = node("leaf", 1, vec![]);
        let depth2 = node("depth2", 2, vec![leaf]);
        let depth1 = node("depth1", 3, vec![depth2]);
        let pruned = prune_node(&depth1, 0);

        assert_eq!(pruned.children.len(), 1);
        assert_eq!(pruned.children[0].children.len(), 1);
        assert!(pruned.children[0].children[0].children.is_empty());
    }

    #[test]
    fn treemap_budget_marks_truncated() {
        let children = (0..10)
            .map(|index| node(&format!("n{index}"), index + 1, vec![]))
            .collect();
        let root = node("root", 100, children);
        let mut budget = 3;
        let mut truncated = false;
        let result = to_treemap(&root, 0, &mut budget, &mut truncated);

        assert!(truncated);
        assert!(result.children.len() < 10);
    }

    #[test]
    fn prune_snapshot_limits_top_dirs_and_large_files() {
        let mut snapshot = ScanSnapshot {
            volume: 'C',
            ..ScanSnapshot::default()
        };
        snapshot.top_dirs = (0..60)
            .map(|index| node(&format!("d{index}"), index as u64 + 1, vec![]))
            .collect();
        snapshot.large_files = (0..300)
            .map(|index| disksage_core::LargeFile {
                path: PathBuf::from(format!(r"C:\f{index}")),
                allocated_bytes: index as u64,
                logical_bytes: index as u64,
                modified_at: None,
                extension: "tmp".into(),
            })
            .collect();

        let summary = prune_snapshot(&snapshot);
        assert_eq!(summary.top_dirs.len(), 50);
        assert_eq!(summary.large_files.len(), 200);
    }
}
