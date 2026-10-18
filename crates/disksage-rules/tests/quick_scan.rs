use std::collections::HashMap;
use std::path::{Path, PathBuf};

use disksage_rules::{RulePack, load_builtin_rules, parse_rule_pack, quick_scan};

fn env_for(temp: &Path, windows: Option<&Path>) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("TEMP".to_string(), temp.display().to_string());
    env.insert("TMP".to_string(), temp.display().to_string());
    if let Some(windows) = windows {
        env.insert("WINDIR".to_string(), windows.display().to_string());
        env.insert("SYSTEMDRIVE".to_string(), "C:".to_string());
    }
    env
}

fn rule_pack(root: &str, exclude: &str) -> RulePack {
    let text = format!(
        r#"
schema_version = 1

[[rules]]
id = "test.rule"
version = 1
verified_at = "2026-10-18"
verified_on = "test"
category = "test"
title_zh = "测试规则"
title_en = "Test rule"
risk = "safe"
requires_admin = false
default_selected = true
recoverability = "regenerable"
min_age_days = 0
detect = {{ path_exists_any = ["{root}"] }}
targets = [
  {{ root = "{root}", include_globs = ["**/*"], exclude_globs = [{exclude}], max_depth = 32 }}
]
explain = {{ what_zh = "测试", what_en = "Test", impact_zh = "无影响", impact_en = "No impact", exclude_notes_zh = "无", exclude_notes_en = "None", source = "test fixture" }}
action = {{ type = "trash" }}
"#
    );
    parse_rule_pack("test", &text).unwrap()
}

#[test]
fn builtin_packs_parse_and_validate() {
    let pack = load_builtin_rules().unwrap();
    assert!(pack.rules.len() >= 10);
    assert!(pack.rules.iter().all(|rule| !rule.id.is_empty()));
    assert!(
        pack.rules
            .iter()
            .filter(|rule| rule.default_selected)
            .all(|rule| matches!(rule.risk, disksage_core::RiskLevel::Safe))
    );
}

#[test]
fn scans_fixture_and_excludes_patterns() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(cache.join("a.tmp"), vec![0u8; 10]).unwrap();
    std::fs::write(cache.join("b.log"), vec![0u8; 5]).unwrap();
    std::fs::write(temp.path().join("keep.db"), vec![0u8; 99]).unwrap();

    let pack = rule_pack("%TEMP%", r#""**/*.db""#);
    let env = env_for(temp.path(), None);
    let result = quick_scan(&pack, &env, false).unwrap();

    assert_eq!(result.findings.len(), 1);
    let finding = &result.findings[0];
    assert_eq!(finding.file_count, 2);
    assert_eq!(finding.allocated_bytes, 15);
    assert_eq!(finding.rule_id, "test.rule");
    assert!(finding.default_selected);
    assert_eq!(finding.evidence.len(), 8);
    assert_eq!(finding.evidence[0].id, "E1");
    assert_eq!(finding.evidence[3].bytes, Some(15));
    assert_eq!(result.matches[&finding.id].len(), 2);
}

#[test]
fn skips_system_red_line_descendants() {
    let temp = tempfile::tempdir().unwrap();
    let windows = temp.path().join("Windows");
    let win_sxs = windows.join("WinSxS");
    let win_temp = windows.join("Temp");
    std::fs::create_dir_all(&win_sxs).unwrap();
    std::fs::create_dir_all(&win_temp).unwrap();
    std::fs::write(win_sxs.join("evil.bin"), vec![0u8; 100]).unwrap();
    std::fs::write(win_temp.join("ok.tmp"), vec![0u8; 3]).unwrap();

    let pack = rule_pack("%TEMP%", "");
    let env = env_for(temp.path(), Some(&windows));
    let result = quick_scan(&pack, &env, false).unwrap();

    let finding = &result.findings[0];
    assert_eq!(finding.file_count, 1);
    let matched = &result.matches[&finding.id];
    assert_eq!(matched, &vec![win_temp.join("ok.tmp")]);
}

#[test]
fn rejects_caution_rules_selected_by_default() {
    let text = r#"
schema_version = 1
[[rules]]
id = "test.bad"
version = 1
category = "test"
title_zh = "bad"
title_en = "bad"
risk = "caution"
requires_admin = false
default_selected = true
recoverability = "regenerable"
targets = [{ root = "%TEMP%", max_depth = 4 }]
explain = { what_zh = "x", what_en = "x", impact_zh = "x", impact_en = "x", source = "x" }
action = { type = "trash" }
"#;
    assert!(parse_rule_pack("bad", text).is_err());
}

#[test]
fn browser_cache_rule_never_matches_login_data() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("Default");
    let cache = profile.join("Cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("a.bin"), vec![0u8; 7]).unwrap();
    std::fs::write(profile.join("Login Data"), vec![0u8; 50]).unwrap();
    std::fs::write(profile.join("History"), vec![0u8; 50]).unwrap();

    let text = r#"
schema_version = 1
[[rules]]
id = "test.browser"
version = 1
category = "browser"
title_zh = "browser"
title_en = "browser"
risk = "safe"
requires_admin = false
default_selected = true
recoverability = "regenerable"
detect = { path_exists_any = ["%TEMP%"] }
targets = [{ root = "%TEMP%", include_globs = ["*/Cache/**"], exclude_globs = ["**/Login Data*", "**/History*"], max_depth = 8 }]
explain = { what_zh = "x", what_en = "x", impact_zh = "x", impact_en = "x", source = "x" }
action = { type = "trash" }
"#;
    let pack = parse_rule_pack("browser", text).unwrap();
    let env = env_for(temp.path(), None);
    let result = quick_scan(&pack, &env, false).unwrap();
    let finding = &result.findings[0];
    assert_eq!(finding.file_count, 1);
    assert_eq!(result.matches[&finding.id], vec![cache.join("a.bin")]);
}

#[test]
fn environment_missing_targets_are_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let pack = rule_pack("%TEMP%", "");
    let mut env = HashMap::new();
    env.insert("WINDIR".to_string(), "C:\\Windows".to_string());
    let result = quick_scan(&pack, &env, false).unwrap();
    assert!(result.findings.is_empty());
    let _ = temp;
    let _: PathBuf = PathBuf::from(".");
}

#[test]
fn admin_rule_without_files_is_reported_as_estimated() {
    let temp = tempfile::tempdir().unwrap();
    let text = r#"
schema_version = 1
[[rules]]
id = "test.admin"
version = 1
category = "test"
title_zh = "管理员测试"
title_en = "Admin test"
risk = "caution"
requires_admin = true
default_selected = false
recoverability = "regenerable"
detect = { path_exists_any = ["%TEMP%"] }
targets = [{ root = "%TEMP%", include_globs = ["**/*.never-match"], exclude_globs = [], max_depth = 4 }]
explain = { what_zh = "测试", what_en = "Test", impact_zh = "无", impact_en = "None", source = "fixture" }
action = { type = "trash" }
"#;
    let pack = parse_rule_pack("admin", text).unwrap();
    let env = env_for(temp.path(), None);

    assert!(quick_scan(&pack, &env, false).unwrap().findings.is_empty());

    let result = quick_scan(&pack, &env, true).unwrap();
    assert_eq!(result.findings.len(), 1);
    let finding = &result.findings[0];
    assert!(finding.requires_admin);
    assert_eq!(finding.file_count, 0);
    assert!(matches!(
        finding.size_provenance,
        disksage_core::SizeProvenance::Estimated
    ));
    assert!(finding.consequence.contains("管理员"));
    assert!(result.matches.get(&finding.id).unwrap().is_empty());
}
