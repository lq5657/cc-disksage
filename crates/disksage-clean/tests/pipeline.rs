use std::collections::HashMap;

use disksage_clean::{
    AuditLog, ExecutionOptions, PlanCandidate, create_plan, execute_plan,
    execute_plan_with_progress,
};
use disksage_core::{ActionSpec, Recoverability, RiskLevel};
use disksage_rules::{parse_rule_pack, quick_scan};

fn env_for(root: &std::path::Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("TEMP".into(), root.display().to_string());
    env.insert("TMP".into(), root.display().to_string());
    env.insert("SYSTEMDRIVE".into(), "C:".into());
    env
}

#[test]
fn quick_scan_plan_execute_pipeline() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let top_tmp = root.join("a.tmp");
    let keep = root.join("keep.txt");
    let nested = root.join("sub");
    std::fs::create_dir(&nested).unwrap();
    let nested_tmp = nested.join("b.tmp");
    std::fs::write(&top_tmp, vec![0u8; 10]).unwrap();
    std::fs::write(&keep, vec![0u8; 20]).unwrap();
    std::fs::write(&nested_tmp, vec![0u8; 5]).unwrap();

    let text = r#"
schema_version = 1
[[rules]]
id = "user.test_fixture"
version = 1
category = "test"
title_zh = "测试缓存"
title_en = "Test cache"
risk = "safe"
requires_admin = false
default_selected = true
recoverability = "regenerable"
min_age_days = 0
detect = { path_exists_any = ["%TEMP%"] }
targets = [{ root = "%TEMP%", include_globs = ["**/*.tmp"], exclude_globs = ["**/keep*"], max_depth = 8 }]
explain = { what_zh = "测试", what_en = "Test", impact_zh = "无", impact_en = "None", source = "fixture" }
action = { type = "delete" }
"#;
    let pack = parse_rule_pack("fixture", text).unwrap();
    let env = env_for(root);
    let scan = quick_scan(&pack, &env, false).unwrap();
    assert_eq!(scan.findings.len(), 1);
    let finding = &scan.findings[0];
    assert_eq!(finding.file_count, 2);
    let files = scan.matches.get(&finding.id).unwrap().clone();
    assert_eq!(files.len(), 2);

    let candidate = PlanCandidate {
        finding_id: finding.id.clone(),
        rule_id: finding.rule_id.clone(),
        title_zh: finding.title_zh.clone(),
        risk: RiskLevel::Safe,
        recoverability: Recoverability::Regenerable,
        requires_admin: false,
        action: ActionSpec::Delete,
        files,
        allowed_roots: finding
            .targets
            .iter()
            .map(|target| target.resolved.clone())
            .collect(),
    };
    let plan = create_plan(vec![candidate], &env).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.total_allocated_bytes, 15);

    let audit = AuditLog::new(root.join("audit.jsonl"));
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

    assert_eq!(result.deleted, 2);
    assert_eq!(result.freed_bytes, 15);
    assert!(!top_tmp.exists());
    assert!(!nested_tmp.exists());
    assert!(keep.exists());
    assert!(audit.path().exists());
}

#[test]
#[ignore = "moves a file to the Windows Recycle Bin; run explicitly on Windows"]
fn trash_action_moves_file_to_recycle_bin() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("trash-me.tmp");
    std::fs::write(&file, b"trash").unwrap();
    let env = env_for(temp.path());

    let candidate = PlanCandidate {
        finding_id: "finding-trash".into(),
        rule_id: "user.test_trash".into(),
        title_zh: "回收站测试".into(),
        risk: RiskLevel::Safe,
        recoverability: Recoverability::Regenerable,
        requires_admin: false,
        action: ActionSpec::Trash,
        files: vec![file.clone()],
        allowed_roots: vec![temp.path().to_path_buf()],
    };
    let plan = create_plan(vec![candidate], &env).unwrap();
    let result = execute_plan(
        &plan,
        &env,
        ExecutionOptions {
            confirm: true,
            dry_run: false,
            elevation: false,
        },
        None,
    )
    .unwrap();

    assert_eq!(result.deleted, 1);
    assert!(!file.exists());
}

#[test]
#[ignore = "moves 600 fixture files to the Windows Recycle Bin; run explicitly on Windows"]
fn trash_batch_moves_many_files_to_recycle_bin() {
    let temp = tempfile::tempdir().unwrap();
    let files = (0..600)
        .map(|index| {
            let path = temp.path().join(format!("batch-{index:03}.tmp"));
            std::fs::write(&path, b"batch").unwrap();
            path
        })
        .collect::<Vec<_>>();
    let env = env_for(temp.path());
    let candidate = PlanCandidate {
        finding_id: "finding-batch".into(),
        rule_id: "user.test_trash_batch".into(),
        title_zh: "回收站批量测试".into(),
        risk: RiskLevel::Safe,
        recoverability: Recoverability::Regenerable,
        requires_admin: false,
        action: ActionSpec::Trash,
        files: files.clone(),
        allowed_roots: vec![temp.path().to_path_buf()],
    };
    let plan = create_plan(vec![candidate], &env).unwrap();

    let mut updates = Vec::new();
    let mut record = |progress| updates.push(progress);
    let started = std::time::Instant::now();
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
    let elapsed = started.elapsed();

    assert_eq!(result.deleted, 600, "failed: {:?}", result.failed);
    assert!(files.iter().all(|path| !path.exists()));
    let last = updates.last().unwrap();
    assert_eq!(last.phase, "done");
    assert_eq!(last.processed, 600);
    println!("batch trash 600 files took {elapsed:?}");
}
