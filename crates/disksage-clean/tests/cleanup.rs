use std::collections::HashMap;
use std::path::{Path, PathBuf};

use disksage_clean::{
    AuditLog, CleanError, ExecutionOptions, PlanCandidate, create_plan, execute_plan,
    execute_plan_with_progress,
};
use disksage_core::{ActionSpec, Recoverability, RiskLevel};

fn env_for(temp: &Path, windows: Option<&Path>) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("TEMP".into(), temp.display().to_string());
    env.insert("TMP".into(), temp.display().to_string());
    env.insert("SYSTEMDRIVE".into(), "C:".into());
    if let Some(windows) = windows {
        env.insert("WINDIR".into(), windows.display().to_string());
    }
    env
}

fn candidate(
    files: Vec<PathBuf>,
    allowed_roots: Vec<PathBuf>,
    action: ActionSpec,
) -> PlanCandidate {
    PlanCandidate {
        finding_id: "finding-1".into(),
        rule_id: "test.rule".into(),
        title_zh: "测试".into(),
        risk: RiskLevel::Safe,
        recoverability: Recoverability::Regenerable,
        requires_admin: false,
        action,
        files,
        allowed_roots,
    }
}

#[test]
fn plan_rejects_missing_files() {
    let temp = tempfile::tempdir().unwrap();
    let env = env_for(temp.path(), None);
    let missing = temp.path().join("missing.tmp");
    let error = create_plan(
        vec![candidate(
            vec![missing],
            vec![temp.path().to_path_buf()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap_err();
    assert!(matches!(error, CleanError::EmptyPlan));
}

#[test]
fn plan_confines_files_to_allowed_roots() {
    let temp = tempfile::tempdir().unwrap();
    let allowed = temp.path().join("allowed");
    let other = temp.path().join("other");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let good = allowed.join("good.tmp");
    let bad = other.join("bad.tmp");
    std::fs::write(&good, b"good").unwrap();
    std::fs::write(&bad, b"bad").unwrap();

    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(
            vec![good.clone(), bad],
            vec![allowed.clone()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap();

    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].file_count, 1);
    assert_eq!(plan.items[0].targets, vec![good]);
}

#[test]
fn plan_rejects_system_red_lines() {
    let temp = tempfile::tempdir().unwrap();
    let windows = temp.path().join("Windows");
    let win_sxs = windows.join("WinSxS");
    std::fs::create_dir_all(&win_sxs).unwrap();
    let evil = win_sxs.join("evil.bin");
    std::fs::write(&evil, b"evil").unwrap();

    let env = env_for(temp.path(), Some(&windows));
    let error = create_plan(
        vec![candidate(
            vec![evil],
            vec![windows.clone()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap_err();
    assert!(matches!(error, CleanError::EmptyPlan));
}

#[test]
fn execution_requires_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("a.tmp");
    std::fs::write(&file, b"data").unwrap();
    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(
            vec![file.clone()],
            vec![temp.path().to_path_buf()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap();

    let error = execute_plan(
        &plan,
        &env,
        ExecutionOptions {
            confirm: false,
            dry_run: false,
            elevation: false,
        },
        None,
    )
    .unwrap_err();
    assert!(matches!(error, CleanError::ConfirmationRequired));
    assert!(file.exists());
}

#[test]
fn dry_run_does_not_delete() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("a.tmp");
    std::fs::write(&file, b"12345").unwrap();
    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(
            vec![file.clone()],
            vec![temp.path().to_path_buf()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap();

    let result = execute_plan(
        &plan,
        &env,
        ExecutionOptions {
            confirm: true,
            dry_run: true,
            elevation: false,
        },
        None,
    )
    .unwrap();
    assert_eq!(result.deleted, 1);
    assert_eq!(result.freed_bytes, 5);
    assert!(file.exists());
}

#[test]
fn reports_progress_for_delete_action() {
    let temp = tempfile::tempdir().unwrap();
    let files = ["a.tmp", "b.tmp", "c.tmp"]
        .iter()
        .map(|name| {
            let path = temp.path().join(name);
            std::fs::write(&path, b"12345").unwrap();
            path
        })
        .collect::<Vec<_>>();
    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(
            files,
            vec![temp.path().to_path_buf()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap();

    let mut updates = Vec::new();
    let mut record = |progress| updates.push(progress);
    let result = execute_plan_with_progress(
        &plan,
        &env,
        ExecutionOptions {
            confirm: true,
            dry_run: false,
            elevation: false,
        },
        None,
        Some(&mut record),
    )
    .unwrap();

    assert_eq!(result.deleted, 3);
    let last = updates.last().expect("progress callback should run");
    assert_eq!(last.phase, "done");
    assert_eq!(last.total, 3);
    assert_eq!(last.processed, 3);
    assert_eq!(last.deleted, 3);
    assert_eq!(last.failed, 0);
    assert_eq!(last.bytes, 15);
}

#[test]
fn delete_action_removes_file_and_writes_audit() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("a.tmp");
    std::fs::write(&file, b"12345").unwrap();
    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(
            vec![file.clone()],
            vec![temp.path().to_path_buf()],
            ActionSpec::Delete,
        )],
        &env,
    )
    .unwrap();

    let audit = AuditLog::new(temp.path().join("logs").join("audit.jsonl"));
    let result = execute_plan(
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

    assert_eq!(result.deleted, 1);
    assert_eq!(result.freed_bytes, 5);
    assert!(!file.exists());
    let audit_text = std::fs::read_to_string(audit.path()).unwrap();
    assert!(audit_text.contains(&plan.id));
    assert!(audit_text.contains("success"));
}

#[test]
fn admin_action_without_files_is_kept_for_helper() {
    let temp = tempfile::tempdir().unwrap();
    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(vec![], vec![], ActionSpec::DismAnalyze)],
        &env,
    )
    .unwrap_err();
    assert!(matches!(plan, CleanError::EmptyPlan));

    let mut admin = candidate(vec![], vec![], ActionSpec::DismAnalyze);
    admin.requires_admin = true;
    admin.rule_id = "system.dism_analyze".into();
    let plan = create_plan(vec![admin], &env).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert!(plan.items[0].requires_admin);
    assert_eq!(plan.items[0].file_count, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.code == "admin_helper")
    );
    assert!(plan.requires_admin);
}

#[test]
fn action_only_recycle_bin_plan_is_kept_and_dry_run_safe() {
    let temp = tempfile::tempdir().unwrap();
    let env = env_for(temp.path(), None);
    let plan = create_plan(
        vec![candidate(vec![], vec![], ActionSpec::EmptyRecycleBin)],
        &env,
    )
    .unwrap();

    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].file_count, 0);
    assert!(!plan.requires_admin);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.code == "system_action")
    );

    let result = execute_plan(
        &plan,
        &env,
        ExecutionOptions {
            confirm: true,
            dry_run: true,
            elevation: false,
        },
        None,
    )
    .unwrap();
    assert!(result.notes.iter().any(|note| note.contains("dry-run")));
    assert_eq!(result.deleted, 0);
}
