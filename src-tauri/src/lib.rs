mod auto_clean;
mod elevation;
mod storage;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use disksage_clean::{
    AuditLog, CleanupProgress, ExecutionOptions, PlanCandidate, create_plan as build_plan,
    execute_plan as run_plan, execute_plan_with_progress as run_plan_with_progress,
};
use disksage_core::path_safety::is_strict_descendant;
use disksage_core::{
    ActionSpec, CleanupPlan, CleanupResult, DirNode, Evidence, EvidenceKind, EvidenceSource,
    Finding, GuideId, LargeFile, LlmAssessment, MatchedPath, PlanWarning, Recoverability,
    RiskLevel, SampleFile, ScanPayload, ScanSnapshot, SizeProvenance, VolumeInfo, paths,
};
use disksage_llm::{
    LlmClient, LlmConfig, LlmPreset, PRESETS, RedactContext, build_finding_payload, delete_key,
    get_key, payload_json, set_key,
};
use disksage_rules::{
    QuickScanResult, load_builtin_rules, load_user_rules, quick_scan as run_quick_scan,
};
use disksage_scan::{
    CancelToken, DuplicateRequest, DuplicateScanProgress, DuplicateScanResult, ScanRequest,
    find_duplicates, scan_volume as run_scan,
};
use serde::Serialize;
use storage::{ScanHistoryEntry, Storage};
use tauri::Manager;
use uuid::Uuid;

#[derive(Clone)]
struct StoredFinding {
    finding: Finding,
    #[allow(dead_code)]
    files: Vec<PathBuf>,
}

struct AppState {
    findings: Mutex<HashMap<String, StoredFinding>>,
    plans: Mutex<HashMap<String, CleanupPlan>>,
    assessments: Mutex<HashMap<String, LlmAssessment>>,
    snapshots: Mutex<HashMap<String, ScanSnapshot>>,
    scan_cancel: Mutex<Option<CancelToken>>,
    duplicate_scans: Mutex<HashMap<String, DuplicateScanResult>>,
    duplicate_cancel: Mutex<Option<CancelToken>>,
    storage: Storage,
}

impl AppState {
    fn new(storage: Storage) -> Self {
        Self {
            findings: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            assessments: Mutex::new(HashMap::new()),
            snapshots: Mutex::new(HashMap::new()),
            scan_cancel: Mutex::new(None),
            duplicate_scans: Mutex::new(HashMap::new()),
            duplicate_cancel: Mutex::new(None),
            storage,
        }
    }
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
    size_provenance: SizeProvenance,
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
    Some(paths::rules_dir())
}

#[tauri::command]
fn get_volumes() -> Vec<VolumeInfo> {
    disksage_scan::enumerate_volumes()
}

#[tauri::command]
async fn scan_volume(
    app: tauri::AppHandle,
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
        .insert(summary.id.clone(), summary.clone());
    if let Err(error) = state.storage.save_scan(&summary) {
        eprintln!("failed to persist scan: {error}");
    }
    notify(
        &app,
        "扫描完成",
        format!(
            "{}: {} 个文件 / {} / {:.1}s",
            summary.volume,
            summary.file_count,
            format_bytes_short(summary.allocated_bytes),
            summary.duration_ms as f64 / 1000.0
        ),
    );
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
    let snapshot = {
        let snapshots = state
            .snapshots
            .lock()
            .map_err(|_| "snapshot store lock poisoned".to_string())?;
        snapshots.get(&snapshot_id).cloned()
    };
    let snapshot = match snapshot {
        Some(snapshot) => snapshot,
        None => state
            .storage
            .load_scan(&snapshot_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "snapshot not found; run a full scan first".to_string())?,
    };

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

fn snapshot_large_file(
    state: &AppState,
    snapshot_id: &str,
    file_index: usize,
) -> Result<(ScanSnapshot, LargeFile), String> {
    let snapshot = {
        let snapshots = state
            .snapshots
            .lock()
            .map_err(|_| "snapshot store lock poisoned".to_string())?;
        snapshots.get(snapshot_id).cloned()
    };
    let snapshot = match snapshot {
        Some(snapshot) => snapshot,
        None => state
            .storage
            .load_scan(snapshot_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "扫描结果不存在，请重新扫描".to_string())?,
    };
    let file = snapshot
        .large_files
        .get(file_index)
        .cloned()
        .ok_or_else(|| "大文件条目不存在，请重新扫描".to_string())?;
    Ok((snapshot, file))
}

/// Large-file deletion is intentionally restricted to non-system locations.
/// The rule engine protects its own red lines; this extra guard keeps a
/// user-selected file in Windows/Program Files from being recycled.
fn is_system_install_tree(path: &std::path::Path, env: &HashMap<String, String>) -> bool {
    ["WINDIR", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"]
        .iter()
        .filter_map(|key| env.get(*key))
        .map(PathBuf::from)
        .any(|root| {
            is_strict_descendant(path, &root)
                || path
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&root.to_string_lossy())
        })
}

#[tauri::command]
fn open_large_file(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
    file_index: usize,
) -> Result<(), String> {
    let (_snapshot, file) = snapshot_large_file(&state, &snapshot_id, file_index)?;
    let metadata = file
        .path
        .symlink_metadata()
        .map_err(|error| format!("无法读取文件：{error}"))?;
    if !metadata.is_file() {
        return Err("文件已不存在或不再是普通文件，请重新扫描".into());
    }
    let windows = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    std::process::Command::new(windows.join("explorer.exe"))
        .arg(format!("/select,{}", file.path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn large_file_ai_finding(snapshot: &ScanSnapshot, file_index: usize) -> Result<Finding, String> {
    let file = snapshot
        .large_files
        .get(file_index)
        .ok_or_else(|| "大文件条目不存在，请重新扫描".to_string())?;
    let metadata = file
        .path
        .symlink_metadata()
        .map_err(|error| format!("无法读取文件：{error}"))?;
    if !metadata.is_file() {
        return Err("文件已不存在或不再是普通文件，请重新扫描".into());
    }

    let parent = file
        .path
        .parent()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let masked_path = format!("{}\\<file>", parent.trim_end_matches(['\\', '/']));
    let file_name = file
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unknown>".into());
    let extension = if file.extension.trim().is_empty() {
        "(none)".to_string()
    } else {
        format!(".{}", file.extension)
    };

    Ok(Finding {
        id: format!("large_file:{}:{file_index}", snapshot.id),
        rule_id: "large_file.user_selected".into(),
        rule_version: 1,
        category: "large_file".into(),
        title_zh: "大文件（用户选择）".into(),
        title_en: "Large file (user selected)".into(),
        risk: RiskLevel::Caution,
        recoverability: Recoverability::RecycleBin,
        requires_admin: false,
        default_selected: false,
        targets: vec![MatchedPath {
            template: masked_path,
            resolved: file.path.clone(),
            bytes: file.allocated_bytes,
            file_count: 1,
        }],
        allocated_bytes: file.allocated_bytes,
        logical_bytes: file.logical_bytes,
        file_count: 1,
        size_provenance: SizeProvenance::Scanner,
        oldest_mtime: file.modified_at,
        newest_mtime: file.modified_at,
        ext_histogram: vec![(extension.clone(), 1)],
        sample_files: vec![SampleFile {
            name: file_name,
            bytes: file.allocated_bytes,
            modified_at: file.modified_at,
        }],
        evidence: vec![
            Evidence {
                id: "E1".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::Path,
                claim: format!("用户在大文件列表中手动选择的文件，所在目录：{parent}"),
                path: Some(file.path.clone()),
                bytes: None,
                timestamp: None,
                rule_id: None,
                confidence: 1.0,
            },
            Evidence {
                id: "E2".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::Size,
                claim: "分配大小与逻辑大小由本地扫描器提供。".into(),
                path: None,
                bytes: Some(file.allocated_bytes),
                timestamp: None,
                rule_id: None,
                confidence: 1.0,
            },
            Evidence {
                id: "E3".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::Age,
                claim: "最近修改时间由本地扫描器提供。".into(),
                path: None,
                bytes: None,
                timestamp: file.modified_at,
                rule_id: None,
                confidence: 1.0,
            },
            Evidence {
                id: "E4".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::FileType,
                claim: format!("文件扩展名为 {extension}。"),
                path: None,
                bytes: None,
                timestamp: None,
                rule_id: None,
                confidence: 1.0,
            },
            Evidence {
                id: "E5".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::SafetyCheck,
                claim: "该候选不来自规则缓存；删除默认进入回收站，AI 只提供建议，不能删除文件。"
                    .into(),
                path: None,
                bytes: None,
                timestamp: None,
                rule_id: None,
                confidence: 1.0,
            },
        ],
        consequence:
            "这是用户手动选择的大文件，不是规则匹配的缓存。删除会进入回收站；是否安全取决于文件用途。"
                .into(),
        action: ActionSpec::Trash,
        llm: None,
    })
}

#[tauri::command]
fn llm_preview_large_file(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
    file_index: usize,
) -> Result<serde_json::Value, String> {
    let (snapshot, _) = snapshot_large_file(&state, &snapshot_id, file_index)?;
    let finding = large_file_ai_finding(&snapshot, file_index)?;
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
    let redact = RedactContext::from_env(config.redact_paths);
    Ok(payload_json(&build_finding_payload(
        &finding, &config, &redact,
    )))
}

#[tauri::command]
async fn llm_assess_large_file(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
    file_index: usize,
) -> Result<LlmAssessment, String> {
    let (snapshot, _) = snapshot_large_file(&state, &snapshot_id, file_index)?;
    let finding = large_file_ai_finding(&snapshot, file_index)?;
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
    let key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中保存 API Key".to_string())?;
    let client = LlmClient::new(config, key).map_err(|error| error.to_string())?;
    client
        .assess_finding(&finding)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct LargeFileDeleteResponse {
    snapshot: ScanSnapshot,
    result: CleanupResult,
}

#[tauri::command]
async fn delete_large_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
    file_index: usize,
    confirmed: bool,
) -> Result<LargeFileDeleteResponse, String> {
    if !confirmed {
        return Err("请先确认要移到回收站的文件".into());
    }
    let (mut snapshot, file) = snapshot_large_file(&state, &snapshot_id, file_index)?;
    let metadata = file
        .path
        .symlink_metadata()
        .map_err(|error| format!("无法读取文件：{error}"))?;
    if !metadata.is_file() {
        return Err("文件已不存在或不再是普通文件，请重新扫描".into());
    }

    let env: HashMap<String, String> = std::env::vars().collect();
    if is_system_install_tree(&file.path, &env) {
        return Err("Windows/Program Files/ProgramData 中的文件不允许通过大文件清理删除".into());
    }
    let parent = file
        .path
        .parent()
        .ok_or_else(|| "无法确定文件所在目录".to_string())?
        .to_path_buf();
    let candidate = PlanCandidate {
        finding_id: format!("large_file:{snapshot_id}:{file_index}"),
        rule_id: "large_file.user_selected".into(),
        title_zh: "大文件（用户选择）".into(),
        risk: RiskLevel::Caution,
        recoverability: Recoverability::RecycleBin,
        requires_admin: false,
        action: ActionSpec::Trash,
        files: vec![file.path.clone()],
        allowed_roots: vec![parent],
    };

    let plan_env = env.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || build_plan(vec![candidate], &plan_env))
        .await
        .map_err(|error| format!("plan task failed: {error}"))?
        .map_err(|error| error.to_string())?;

    let exec_env = env;
    let audit_path = AuditLog::default_location();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let audit = AuditLog::new(audit_path);
        run_plan(
            &plan,
            &exec_env,
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

    if result.deleted == 0 {
        return Err(result
            .failed
            .first()
            .map(|(_, message)| message.clone())
            .unwrap_or_else(|| "文件未能移到回收站，可能已被占用".into()));
    }

    snapshot.large_files.retain(|entry| entry.path != file.path);
    if let Some(volume) = disksage_scan::enumerate_volumes()
        .into_iter()
        .find(|volume| volume.letter == snapshot.volume)
    {
        snapshot.free_bytes = volume.free_bytes;
    }
    state
        .snapshots
        .lock()
        .map_err(|_| "snapshot store lock poisoned".to_string())?
        .insert(snapshot.id.clone(), snapshot.clone());
    if let Err(error) = state.storage.save_scan(&snapshot) {
        eprintln!("failed to persist snapshot after large-file deletion: {error}");
    }

    notify(
        &app,
        "大文件已移到回收站",
        format!(
            "{} · {}",
            file.path.display(),
            format_bytes_short(file.allocated_bytes)
        ),
    );
    Ok(LargeFileDeleteResponse { snapshot, result })
}

#[tauri::command]
fn get_latest_scan(state: tauri::State<'_, AppState>) -> Result<Option<ScanSnapshot>, String> {
    let snapshot = state.storage.latest_scan()?;
    if let Some(snapshot) = &snapshot {
        state
            .snapshots
            .lock()
            .map_err(|_| "snapshot store lock poisoned".to_string())?
            .insert(snapshot.id.clone(), snapshot.clone());
    }
    Ok(snapshot)
}

#[tauri::command]
fn get_scan_history(
    state: tauri::State<'_, AppState>,
    limit: usize,
) -> Result<Vec<ScanHistoryEntry>, String> {
    state.storage.scan_history(limit)
}

#[tauri::command]
fn get_latest_duplicate_scan(
    state: tauri::State<'_, AppState>,
) -> Result<Option<DuplicateScanResult>, String> {
    let result = state.storage.latest_duplicate_scan()?;
    if let Some(result) = &result {
        state
            .duplicate_scans
            .lock()
            .map_err(|_| "duplicate store lock poisoned".to_string())?
            .insert(result.scan_id.clone(), result.clone());
    }
    Ok(result)
}

fn run_local_quick_scan() -> Result<QuickScanResult, String> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let mut pack = load_builtin_rules().map_err(|error| error.to_string())?;
    if let Some(directory) = user_rules_dir() {
        let user = load_user_rules(&directory).map_err(|error| error.to_string())?;
        pack.rules.extend(user.rules);
    }
    run_quick_scan(&pack, &env, true).map_err(|error| error.to_string())
}

fn store_quick_scan(state: &AppState, result: &QuickScanResult) -> Result<(), String> {
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
    Ok(())
}

fn stored_findings(state: &AppState) -> Result<Vec<Finding>, String> {
    let store = state
        .findings
        .lock()
        .map_err(|_| "finding store lock poisoned".to_string())?;
    Ok(store
        .values()
        .map(|stored| stored.finding.clone())
        .collect())
}

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::Safe => 0,
        RiskLevel::Caution => 1,
        RiskLevel::Danger => 2,
    }
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        risk_rank(a.risk)
            .cmp(&risk_rank(b.risk))
            .then_with(|| b.allocated_bytes.cmp(&a.allocated_bytes))
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });
}

#[tauri::command]
async fn quick_scan(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Finding>, String> {
    let result = tauri::async_runtime::spawn_blocking(run_local_quick_scan)
        .await
        .map_err(|error| format!("quick scan task failed: {error}"))??;

    store_quick_scan(&state, &result)?;

    let total_bytes = result
        .findings
        .iter()
        .map(|finding| finding.allocated_bytes)
        .fold(0u64, u64::saturating_add);
    notify(
        &app,
        "快速扫描完成",
        format!(
            "{} 个候选项，共 {}",
            result.findings.len(),
            format_bytes_short(total_bytes)
        ),
    );
    Ok(result.findings)
}

#[derive(Serialize)]
struct AdminScanResponse {
    findings: Vec<Finding>,
    admin_error: Option<String>,
    admin_count: usize,
}

/// Refresh the normal findings, then ask the elevated helper to measure the
/// admin-only rules. The UI gets a complete, freshly measured list; a UAC
/// cancellation still returns the normal findings together with an error.
#[tauri::command]
async fn quick_scan_admin(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AdminScanResponse, String> {
    let local = tauri::async_runtime::spawn_blocking(run_local_quick_scan)
        .await
        .map_err(|error| format!("quick scan task failed: {error}"))??;
    store_quick_scan(&state, &local)?;

    let mut admin_error = None;
    let mut admin_count = 0usize;
    match run_admin_scan_job().await {
        Ok(mut payload) => {
            for finding in &mut payload.findings {
                if let Some(targets) = payload.targets.remove(&finding.id) {
                    finding.targets = targets;
                }
            }
            admin_count = payload.findings.len();
            let mut store = state
                .findings
                .lock()
                .map_err(|_| "finding store lock poisoned".to_string())?;
            store.retain(|_, stored| !stored.finding.requires_admin);
            for finding in &payload.findings {
                store.insert(
                    finding.id.clone(),
                    StoredFinding {
                        finding: finding.clone(),
                        files: payload
                            .matches
                            .get(&finding.id)
                            .cloned()
                            .unwrap_or_default(),
                    },
                );
            }
        }
        Err(error) => admin_error = Some(error),
    }

    let mut findings = stored_findings(&state)?;
    sort_findings(&mut findings);
    notify(
        &app,
        if admin_error.is_some() {
            "管理员扫描未完成"
        } else {
            "管理员扫描完成"
        },
        match &admin_error {
            Some(error) => format!("已保留普通扫描结果：{error}"),
            None => format!(
                "{} 个候选项，其中 {} 个管理员项已实测",
                findings.len(),
                admin_count
            ),
        },
    );
    Ok(AdminScanResponse {
        findings,
        admin_error,
        admin_count,
    })
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
    let (candidates, provenance) = {
        let store = state
            .findings
            .lock()
            .map_err(|_| "finding store lock poisoned".to_string())?;
        let mut candidates = Vec::new();
        let mut provenance = HashMap::new();
        for finding_id in finding_ids {
            let Some(stored) = store.get(&finding_id) else {
                continue;
            };
            let finding = &stored.finding;
            provenance.insert(finding.id.clone(), finding.size_provenance);
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
        (candidates, provenance)
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
                size_provenance: provenance
                    .get(&item.finding_id)
                    .copied()
                    .unwrap_or(SizeProvenance::Estimated),
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    plan_id: String,
    permanent_delete: bool,
    on_progress: tauri::ipc::Channel<CleanupProgress>,
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

    if permanent_delete {
        for item in &normal_items {
            if !matches!(item.action, ActionSpec::Trash | ActionSpec::Delete)
                || item.recoverability != Recoverability::Regenerable
                || item.risk == RiskLevel::Danger
            {
                return Err(
                    "永久删除仅允许可重建且非危险的缓存项；请取消该选项或移除不适用项目。"
                        .to_string(),
                );
            }
        }
        for item in &mut normal_items {
            item.action = ActionSpec::Delete;
        }
    }

    let started = std::time::Instant::now();
    let total_items = normal_items
        .iter()
        .map(|item| item.file_count)
        .fold(0u64, u64::saturating_add)
        .saturating_add(admin_items.len() as u64)
        .saturating_add(guided.len() as u64);

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
        let progress = on_progress.clone();
        let normal_result = tauri::async_runtime::spawn_blocking(move || {
            let audit = AuditLog::new(AuditLog::default_location());
            run_plan_with_progress(
                &normal_plan,
                &normal_env,
                ExecutionOptions {
                    confirm: true,
                    dry_run: false,
                    elevation: false,
                },
                Some(&audit),
                Some(&mut |progress_value| {
                    let _ = progress.send(progress_value);
                }),
            )
        })
        .await
        .map_err(|error| format!("cleanup task failed: {error}"))?
        .map_err(|error| error.to_string())?;
        merge_cleanup_result(&mut result, normal_result);
    }

    if !admin_items.is_empty() {
        let _ = on_progress.send(CleanupProgress {
            phase: "admin".into(),
            total: admin_items.len() as u64,
            current: "正在请求管理员授权并重新扫描系统项…".into(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            ..CleanupProgress::default()
        });
        let rule_ids = admin_items
            .iter()
            .map(|item| item.rule_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        match run_admin_job(rule_ids).await {
            Ok(admin_result) => {
                let _ = on_progress.send(CleanupProgress {
                    phase: "admin".into(),
                    processed: admin_items.len() as u64,
                    total: admin_items.len() as u64,
                    deleted: admin_result.deleted,
                    failed: admin_result.failed.len() as u64,
                    bytes: admin_result.freed_bytes,
                    current: "管理员项已处理".into(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
                merge_cleanup_result(&mut result, admin_result);
            }
            Err(error) => {
                let _ = on_progress.send(CleanupProgress {
                    phase: "admin".into(),
                    processed: admin_items.len() as u64,
                    total: admin_items.len() as u64,
                    failed: 1,
                    current: error.clone(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..CleanupProgress::default()
                });
                result.notes.push(format!("管理员清理未执行：{error}"));
            }
        }
    }

    for guide in guided {
        let _ = on_progress.send(CleanupProgress {
            phase: "guided".into(),
            current: guide_label_zh(guide).to_string(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            ..CleanupProgress::default()
        });
        match open_guide(guide) {
            Ok(()) => result
                .notes
                .push(format!("已打开系统设置：{}", guide_label_zh(guide))),
            Err(error) => result
                .notes
                .push(format!("打开 {} 失败：{error}", guide_label_zh(guide))),
        }
    }

    let _ = on_progress.send(CleanupProgress {
        phase: "done".into(),
        processed: result
            .deleted
            .saturating_add(result.failed.len() as u64)
            .saturating_add(result.skipped_locked.len() as u64),
        total: total_items,
        deleted: result.deleted,
        failed: result.failed.len() as u64,
        bytes: result.freed_bytes,
        current: String::new(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    });

    notify(
        &app,
        "清理完成",
        format!(
            "处理 {}，删除 {} 个文件，失败 {}",
            format_bytes_short(result.freed_bytes),
            result.deleted,
            result.failed.len()
        ),
    );
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
    paths::jobs_dir()
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
    #[serde(skip_serializing_if = "Option::is_none")]
    volume: Option<String>,
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
        volume: None,
    };
    let text = serde_json::to_string_pretty(&job).map_err(|error| error.to_string())?;
    std::fs::write(&job_path, text).map_err(|error| error.to_string())?;

    let helper_for_launch = helper.clone();
    let job_for_launch = job_path.clone();
    let launch = tauri::async_runtime::spawn_blocking(move || {
        elevation::launch_elevated_helper(&helper_for_launch, &job_for_launch)
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

async fn run_admin_scan_job() -> Result<ScanPayload, String> {
    let helper = helper_executable()
        .ok_or_else(|| "未找到 disksage-helper.exe（请重新安装或构建 helper）".to_string())?;
    let jobs = jobs_directory();
    std::fs::create_dir_all(&jobs).map_err(|error| error.to_string())?;

    let job_id = Uuid::new_v4().to_string();
    let job_path = jobs.join(format!("{job_id}.json"));
    let result_path = job_path.with_extension("result.json");
    let payload_path = job_path.with_extension("scan.json");
    let _ = std::fs::remove_file(&result_path);
    let _ = std::fs::remove_file(&payload_path);

    let job = HelperJobFile {
        task: "quick_scan".into(),
        job_id: job_id.clone(),
        nonce: Uuid::new_v4().to_string(),
        expires_at: chrono::Utc::now().timestamp() + 600,
        rule_ids: Vec::new(),
        volume: None,
    };
    let text = serde_json::to_string_pretty(&job).map_err(|error| error.to_string())?;
    std::fs::write(&job_path, text).map_err(|error| error.to_string())?;

    let helper_for_launch = helper.clone();
    let job_for_launch = job_path.clone();
    let launch = tauri::async_runtime::spawn_blocking(move || {
        elevation::launch_elevated_helper(&helper_for_launch, &job_for_launch)
    })
    .await
    .map_err(|error| format!("helper launch task failed: {error}"))?;

    if !result_path.exists() {
        return Err(match launch {
            Ok(()) => "提权 helper 未产生管理员扫描结果".into(),
            Err(error) => error,
        });
    }

    let text = std::fs::read_to_string(&result_path).map_err(|error| error.to_string())?;
    let helper_result: CleanupResult =
        serde_json::from_str(&text).map_err(|error| format!("无效的 helper 结果：{error}"))?;
    if !helper_result.failed.is_empty() {
        return Err(helper_result
            .notes
            .last()
            .cloned()
            .unwrap_or_else(|| "管理员扫描失败".into()));
    }
    if !payload_path.exists() {
        return Err("helper 未写出管理员扫描结果".into());
    }
    let text = std::fs::read_to_string(&payload_path).map_err(|error| error.to_string())?;
    let payload: ScanPayload =
        serde_json::from_str(&text).map_err(|error| format!("无效的管理员扫描结果：{error}"))?;

    let _ = std::fs::remove_file(&job_path);
    let _ = std::fs::remove_file(&result_path);
    let _ = std::fs::remove_file(&payload_path);
    Ok(payload)
}

async fn run_mft_scan_job(volume: char) -> Result<ScanSnapshot, String> {
    let helper = helper_executable()
        .ok_or_else(|| "未找到 disksage-helper.exe（请重新安装或构建 helper）".to_string())?;
    let jobs = jobs_directory();
    std::fs::create_dir_all(&jobs).map_err(|error| error.to_string())?;

    let job_id = Uuid::new_v4().to_string();
    let job_path = jobs.join(format!("{job_id}.json"));
    let result_path = job_path.with_extension("result.json");
    let snapshot_path = job_path.with_extension("snapshot.json");
    let _ = std::fs::remove_file(&result_path);
    let _ = std::fs::remove_file(&snapshot_path);

    let job = HelperJobFile {
        task: "mft_scan".into(),
        job_id: job_id.clone(),
        nonce: Uuid::new_v4().to_string(),
        expires_at: chrono::Utc::now().timestamp() + 600,
        rule_ids: Vec::new(),
        volume: Some(volume.to_ascii_uppercase().to_string()),
    };
    let text = serde_json::to_string_pretty(&job).map_err(|error| error.to_string())?;
    std::fs::write(&job_path, text).map_err(|error| error.to_string())?;

    let helper_for_launch = helper.clone();
    let job_for_launch = job_path.clone();
    let launch = tauri::async_runtime::spawn_blocking(move || {
        elevation::launch_elevated_helper(&helper_for_launch, &job_for_launch)
    })
    .await
    .map_err(|error| format!("helper launch task failed: {error}"))?;

    if !result_path.exists() {
        return Err(match launch {
            Ok(()) => "提权 helper 未产生 MFT 扫描结果".into(),
            Err(error) => error,
        });
    }

    let text = std::fs::read_to_string(&result_path).map_err(|error| error.to_string())?;
    let result: CleanupResult =
        serde_json::from_str(&text).map_err(|error| format!("无效的 helper 结果：{error}"))?;
    if !result.failed.is_empty() {
        return Err(result
            .notes
            .last()
            .cloned()
            .unwrap_or_else(|| "MFT 提权扫描失败".into()));
    }
    if !snapshot_path.exists() {
        return Err("helper 未写出 MFT 快照".into());
    }
    let text = std::fs::read_to_string(&snapshot_path).map_err(|error| error.to_string())?;
    let snapshot: ScanSnapshot =
        serde_json::from_str(&text).map_err(|error| format!("无效的 MFT 快照：{error}"))?;
    let _ = std::fs::remove_file(&job_path);
    let _ = std::fs::remove_file(&result_path);
    let _ = std::fs::remove_file(&snapshot_path);
    Ok(snapshot)
}

#[tauri::command]
async fn scan_volume_mft(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    volume: String,
) -> Result<ScanSnapshot, String> {
    let letter = volume.trim().trim_end_matches(':').to_ascii_uppercase();
    if letter.len() != 1
        || !letter
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return Err("invalid volume letter".to_string());
    }
    let letter = letter.chars().next().unwrap_or('C');
    let info = disksage_scan::enumerate_volumes()
        .into_iter()
        .find(|info| info.letter == letter)
        .ok_or_else(|| format!("未找到 {letter}: 盘"))?;
    if !info.filesystem.eq_ignore_ascii_case("NTFS") {
        return Err(format!(
            "{}: 盘的文件系统是 {}，MFT 秒扫仅支持 NTFS；请使用普通扫描。",
            letter, info.filesystem
        ));
    }

    let snapshot = run_mft_scan_job(letter).await?;
    let summary = prune_snapshot(&snapshot);
    state
        .snapshots
        .lock()
        .map_err(|_| "snapshot store lock poisoned".to_string())?
        .insert(summary.id.clone(), summary.clone());
    if let Err(error) = state.storage.save_scan(&summary) {
        eprintln!("failed to persist MFT scan: {error}");
    }
    notify(
        &app,
        "MFT 扫描完成",
        format!(
            "{}: {} 个文件 / {} / {:.1}s",
            summary.volume,
            summary.file_count,
            format_bytes_short(summary.allocated_bytes),
            summary.duration_ms as f64 / 1000.0
        ),
    );
    Ok(summary)
}

fn duplicate_roots(scope: &str) -> Result<Vec<PathBuf>, String> {
    let candidates: Vec<Option<PathBuf>> = match scope {
        "user" => vec![
            dirs::document_dir(),
            dirs::download_dir(),
            dirs::picture_dir(),
            dirs::video_dir(),
            dirs::desktop_dir(),
        ],
        "downloads" => vec![dirs::download_dir()],
        "documents" => vec![dirs::document_dir()],
        "pictures" => vec![dirs::picture_dir()],
        "videos" => vec![dirs::video_dir()],
        "desktop" => vec![dirs::desktop_dir()],
        _ => return Err("invalid duplicate scan scope".into()),
    };
    let mut roots = Vec::new();
    for root in candidates.into_iter().flatten() {
        if root.is_dir() && !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
    if roots.is_empty() {
        return Err("所选目录不存在或无法访问".into());
    }
    Ok(roots)
}

#[tauri::command]
async fn find_duplicate_files(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    scope: String,
    min_size: u64,
    on_progress: tauri::ipc::Channel<DuplicateScanProgress>,
) -> Result<DuplicateScanResult, String> {
    let roots = duplicate_roots(&scope)?;
    let min_size = min_size.clamp(4 * 1024, 10 * 1024 * 1024 * 1024);
    let mut request = DuplicateRequest::new(roots);
    request.min_size = min_size;

    let cancel = CancelToken::new();
    *state
        .duplicate_cancel
        .lock()
        .map_err(|_| "duplicate cancel lock poisoned".to_string())? = Some(cancel.clone());

    let result = tauri::async_runtime::spawn_blocking(move || {
        find_duplicates(
            request,
            move |progress| {
                let _ = on_progress.send(progress);
            },
            cancel,
        )
    })
    .await
    .map_err(|error| format!("duplicate scan task failed: {error}"));

    if let Ok(mut guard) = state.duplicate_cancel.lock() {
        *guard = None;
    }
    let result = result?.map_err(|error| error.to_string())?;

    {
        let mut store = state
            .duplicate_scans
            .lock()
            .map_err(|_| "duplicate store lock poisoned".to_string())?;
        store.clear();
        store.insert(result.scan_id.clone(), result.clone());
    }
    if let Err(error) = state.storage.save_duplicate_scan(&result) {
        eprintln!("failed to persist duplicate scan: {error}");
    }
    notify(
        &app,
        "重复文件扫描完成",
        format!(
            "{} 组，可释放 {}，扫描 {} 个文件",
            result.groups.len(),
            format_bytes_short(result.total_wasted_bytes),
            result.files_scanned
        ),
    );
    Ok(result)
}

#[tauri::command]
fn cancel_duplicate_scan(state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(token) = state
        .duplicate_cancel
        .lock()
        .map_err(|_| "duplicate cancel lock poisoned".to_string())?
        .as_ref()
    {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
async fn delete_duplicate_files(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    scan_id: String,
    group_id: String,
    keep_file_id: String,
    delete_file_ids: Vec<String>,
) -> Result<CleanupResult, String> {
    if delete_file_ids.is_empty() {
        return Err("请选择要删除的重复文件".into());
    }
    if delete_file_ids.len() > 100 {
        return Err("单次最多删除 100 个文件".into());
    }
    let unique = delete_file_ids
        .iter()
        .collect::<std::collections::HashSet<_>>();
    if unique.len() != delete_file_ids.len() || delete_file_ids.contains(&keep_file_id) {
        return Err("删除列表无效".into());
    }

    let scan = duplicate_scan_from_state(&state, &scan_id)?;
    let group = scan
        .groups
        .iter()
        .find(|group| group.id == group_id)
        .ok_or_else(|| "重复文件组已失效，请重新扫描".to_string())?;
    if !group.files.iter().any(|file| file.id == keep_file_id) {
        return Err("保留文件不在该组中".into());
    }
    let mut selected_paths = Vec::new();
    for file_id in &delete_file_ids {
        let file = group
            .files
            .iter()
            .find(|file| file.id == *file_id)
            .ok_or_else(|| "删除文件不在该组中".to_string())?;
        let metadata = std::fs::metadata(&file.path)
            .map_err(|error| format!("无法读取 {}：{error}", file.path.display()))?;
        if metadata.len() != group.size {
            return Err(format!(
                "{} 在扫描后已变化，请重新扫描后再删除",
                file.path.display()
            ));
        }
        selected_paths.push(file.path.clone());
    }
    let roots = scan.roots.clone();

    let env: HashMap<String, String> = std::env::vars().collect();
    let candidate = PlanCandidate {
        finding_id: group_id.clone(),
        rule_id: "duplicates.user_selected".into(),
        title_zh: "重复文件（用户选择）".into(),
        risk: RiskLevel::Caution,
        recoverability: Recoverability::RecycleBin,
        requires_admin: false,
        action: ActionSpec::Trash,
        files: selected_paths,
        allowed_roots: roots,
    };
    let plan_input = vec![candidate];
    let plan_env = env.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let plan = build_plan(plan_input, &plan_env).map_err(|error| error.to_string())?;
        let audit = AuditLog::new(AuditLog::default_location());
        run_plan(
            &plan,
            &plan_env,
            ExecutionOptions {
                confirm: true,
                dry_run: false,
                elevation: false,
            },
            Some(&audit),
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("duplicate delete task failed: {error}"))??;

    let updated = {
        let mut store = state
            .duplicate_scans
            .lock()
            .map_err(|_| "duplicate store lock poisoned".to_string())?;
        if let Some(scan) = store.get_mut(&scan_id) {
            scan.groups.retain(|group| group.id != group_id);
            scan.total_wasted_bytes = scan
                .groups
                .iter()
                .map(|group| group.wasted_bytes)
                .fold(0, u64::saturating_add);
            Some(scan.clone())
        } else {
            None
        }
    };
    if let Some(scan) = updated
        && let Err(error) = state.storage.save_duplicate_scan(&scan)
    {
        eprintln!("failed to persist updated duplicate scan: {error}");
    }
    notify(
        &app,
        "重复文件已处理",
        format!(
            "删除 {} 个文件，失败 {}",
            result.deleted,
            result.failed.len()
        ),
    );
    Ok(result)
}

#[tauri::command]
fn open_duplicate_file(
    state: tauri::State<'_, AppState>,
    scan_id: String,
    file_id: String,
) -> Result<(), String> {
    let scan = duplicate_scan_from_state(&state, &scan_id)?;
    let path = scan
        .groups
        .iter()
        .flat_map(|group| group.files.iter())
        .find(|file| file.id == file_id)
        .map(|file| file.path.clone())
        .ok_or_else(|| "文件已不在扫描结果中".to_string())?;
    let windows = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    std::process::Command::new(windows.join("explorer.exe"))
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn duplicate_scan_from_state(
    state: &AppState,
    scan_id: &str,
) -> Result<DuplicateScanResult, String> {
    if let Some(scan) = state
        .duplicate_scans
        .lock()
        .map_err(|_| "duplicate store lock poisoned".to_string())?
        .get(scan_id)
        .cloned()
    {
        return Ok(scan);
    }
    let scan = state
        .storage
        .load_duplicate_scan(scan_id)?
        .ok_or_else(|| "重复扫描结果已失效，请重新扫描".to_string())?;
    state
        .duplicate_scans
        .lock()
        .map_err(|_| "duplicate store lock poisoned".to_string())?
        .insert(scan_id.to_string(), scan.clone());
    Ok(scan)
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
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
    let has_key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .is_some();
    Ok(public_llm_config(&config, has_key))
}

#[tauri::command]
fn llm_set_config(config: LlmConfig) -> Result<LlmConfigPublic, String> {
    let path = paths::llm_config_path();
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
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
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
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
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
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
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
    let config = LlmConfig::load(&paths::llm_config_path()).map_err(|error| error.to_string())?;
    let key = get_key(&config.provider_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中保存 API Key".to_string())?;
    let client = LlmClient::new(config, key).map_err(|error| error.to_string())?;
    client
        .ask_followup(&finding, &assessment, &question)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_auto_clean_status() -> Result<auto_clean::AutoCleanStatus, String> {
    auto_clean::status()
}

#[tauri::command]
fn set_auto_clean_config(
    config: auto_clean::AutoCleanConfig,
) -> Result<auto_clean::AutoCleanStatus, String> {
    auto_clean::set_config(config)
}

#[tauri::command]
async fn run_auto_clean_now(app: tauri::AppHandle) -> Result<CleanupResult, String> {
    let result = tauri::async_runtime::spawn_blocking(|| auto_clean::run_auto_clean(true))
        .await
        .map_err(|error| error.to_string())??;
    notify(
        &app,
        "自动清理完成",
        format!(
            "处理 {}，删除 {} 个文件，失败 {}",
            format_bytes_short(result.freed_bytes),
            result.deleted,
            result.failed.len()
        ),
    );
    Ok(result)
}

fn format_bytes_short(bytes: u64) -> String {
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

fn notify(app: &tauri::AppHandle, title: impl Into<String>, body: impl Into<String>) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        eprintln!("failed to show notification: {error}");
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

static TRAY_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "tray_show", "显示主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray_quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("application icon is missing")?;

    let tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("CC DiskSage")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_show" => show_main_window(app),
            "tray_quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(tray);

    if let Some(window) = app.get_webview_window("main") {
        let window_for_close = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_for_close.hide();
                if !TRAY_HINT_SHOWN.swap(true, Ordering::Relaxed) {
                    notify(
                        window_for_close.app_handle(),
                        "CC DiskSage",
                        "已隐藏到系统托盘。点击托盘图标可恢复，右键可退出。",
                    );
                }
            }
        });
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_root = paths::prepare_data_root();

    if std::env::args().any(|argument| argument == "--auto-clean") {
        let exit_code = match auto_clean::run_auto_clean(false) {
            Ok(_) => 0,
            Err(error) => {
                auto_clean::mark_headless_failure(&error);
                1
            }
        };
        std::process::exit(exit_code);
    }

    let storage = match Storage::open_default() {
        Ok(storage) => storage,
        Err(error) => {
            eprintln!("failed to open local database, using in-memory storage: {error}");
            Storage::in_memory().expect("in-memory storage must initialize")
        }
    };

    let mut context = tauri::generate_context!();
    context.config_mut().app.app_directories_override = Some(
        tauri::utils::config::AppDirectoriesOverride::Root(data_root),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .manage(AppState::new(storage))
        .invoke_handler(tauri::generate_handler![
            get_volumes,
            scan_volume,
            cancel_scan,
            get_latest_scan,
            get_scan_history,
            get_treemap,
            quick_scan,
            quick_scan_admin,
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
            llm_ask_followup,
            get_auto_clean_status,
            set_auto_clean_config,
            run_auto_clean_now,
            scan_volume_mft,
            find_duplicate_files,
            cancel_duplicate_scan,
            get_latest_duplicate_scan,
            delete_duplicate_files,
            open_duplicate_file,
            open_large_file,
            delete_large_file,
            llm_preview_large_file,
            llm_assess_large_file
        ])
        .run(context)
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    #[ignore = "moves fixture files to the real recycle bin; run manually"]
    fn duplicate_delete_fixture_end_to_end() {
        use std::fs;

        let root = std::env::temp_dir().join(format!("ccds-dup-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let keep = root.join("keep.bin");
        let delete = root.join("delete.bin");
        let unique = root.join("unique.bin");
        let content = vec![7u8; 4096];
        fs::write(&keep, &content).unwrap();
        fs::write(&delete, &content).unwrap();
        fs::write(&unique, vec![8u8; 4096]).unwrap();

        let mut request = DuplicateRequest::new(vec![root.clone()]);
        request.min_size = 1;
        let scan = find_duplicates(request, |_| {}, CancelToken::new()).unwrap();
        let group = scan
            .groups
            .iter()
            .find(|group| group.files.len() == 2)
            .expect("duplicate group should exist");
        let delete_file = group
            .files
            .iter()
            .find(|file| file.path != keep)
            .expect("delete fixture");
        assert_eq!(delete_file.path, delete);

        let candidate = PlanCandidate {
            finding_id: group.id.clone(),
            rule_id: "duplicates.user_selected".into(),
            title_zh: "重复文件（测试）".into(),
            risk: RiskLevel::Caution,
            recoverability: Recoverability::RecycleBin,
            requires_admin: false,
            action: ActionSpec::Trash,
            files: vec![delete.clone()],
            allowed_roots: vec![root.clone()],
        };
        let env: HashMap<String, String> = std::env::vars().collect();
        let plan = build_plan(vec![candidate], &env).unwrap();
        let audit = AuditLog::new(root.join("audit.jsonl"));
        let result = run_plan(
            &plan,
            &env,
            ExecutionOptions {
                confirm: true,
                dry_run: false,
                elevation: false,
            },
            Some(&audit),
        )
        .unwrap();

        assert!(result.failed.is_empty());
        assert_eq!(result.deleted, 1);
        assert!(keep.exists());
        assert!(!delete.exists());
        assert!(unique.exists());
        let audit_text = fs::read_to_string(root.join("audit.jsonl")).unwrap();
        assert!(audit_text.contains("duplicates.user_selected"));
        let _ = fs::remove_dir_all(&root);
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

    #[test]
    fn large_file_ai_payload_hides_file_name_by_default() {
        let temp = std::env::temp_dir().join(format!("ccds-large-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap();
        let path = temp.join("secret-video.iso");
        fs::write(&path, b"fixture").unwrap();
        let snapshot = ScanSnapshot {
            id: "snap-ai".into(),
            volume: 'C',
            large_files: vec![LargeFile {
                path: path.clone(),
                allocated_bytes: 7,
                logical_bytes: 7,
                modified_at: Some(1_700_000_000),
                extension: "iso".into(),
            }],
            ..ScanSnapshot::default()
        };

        let finding = large_file_ai_finding(&snapshot, 0).unwrap();
        assert_eq!(finding.title_zh, "大文件（用户选择）");
        assert!(finding.targets[0].template.ends_with("<file>"));
        assert_eq!(finding.sample_files.len(), 1);

        let config = LlmConfig::default();
        let redact = RedactContext::from_env(true);
        let payload = build_finding_payload(&finding, &config, &redact);
        let json = serde_json::to_string(&payload_json(&payload)).unwrap();
        assert!(!json.contains("secret-video"));
        assert!(json.contains("<file>"));

        let with_name = LlmConfig {
            send_sample_names: true,
            sample_name_limit: 10,
            ..LlmConfig::default()
        };
        let payload = build_finding_payload(&finding, &with_name, &redact);
        let json = serde_json::to_string(&payload_json(&payload)).unwrap();
        assert!(json.contains("secret-video.iso"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn large_file_deletion_rejects_system_install_trees() {
        let mut env = HashMap::new();
        env.insert("WINDIR".into(), r"C:\Windows".into());
        env.insert("ProgramFiles".into(), r"C:\Program Files".into());
        env.insert("ProgramFiles(x86)".into(), r"C:\Program Files (x86)".into());
        env.insert("ProgramData".into(), r"C:\ProgramData".into());

        assert!(is_system_install_tree(
            std::path::Path::new(r"C:\Windows\System32\large.dll"),
            &env
        ));
        assert!(is_system_install_tree(
            std::path::Path::new(r"C:\Program Files\Game\large.bin"),
            &env
        ));
        assert!(is_system_install_tree(
            std::path::Path::new(r"C:\ProgramData\Vendor\large.bin"),
            &env
        ));
        assert!(!is_system_install_tree(
            std::path::Path::new(r"C:\Users\me\Downloads\large.iso"),
            &env
        ));
        assert!(!is_system_install_tree(
            std::path::Path::new(r"D:\VMs\disk.vhdx"),
            &env
        ));
    }
}
