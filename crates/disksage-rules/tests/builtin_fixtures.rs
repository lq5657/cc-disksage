use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use disksage_core::EnvMap;
use disksage_rules::{RulePack, load_builtin_rules, quick_scan};

/// Rules added in the deep-clean batches. Each one must ship with a
/// `rules/tests/<rule_id>/positive` and `rules/tests/<rule_id>/redline` tree.
const NEW_RULES: &[&str] = &[
    "browser.edge_component_cache",
    "browser.edge_gpu_cache",
    "browser.chrome_component_cache",
    "browser.chrome_gpu_cache",
    "system.servicing_logs",
    "system.prefetch",
    "system.panther_logs",
    "system.live_kernel_reports",
    "system.windows_system_temp",
    "apps.office_cache",
    "system.upgrade_leftovers",
];

const ENV_VARS: &[&str] = &[
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
    "PROGRAMDATA",
    "WINDIR",
    "SYSTEMDRIVE",
];

struct FixtureOutcome {
    findings: usize,
    matches: usize,
    paths: Vec<PathBuf>,
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../rules/tests")
        .canonicalize()
        .expect("cannot resolve rules/tests")
}

fn prepare_env(root: &Path) -> EnvMap {
    let mut env = HashMap::new();
    for variable in ENV_VARS {
        let directory = root.join(variable);
        fs::create_dir_all(&directory).unwrap();
        env.insert((*variable).to_string(), directory.display().to_string());
    }
    env
}

fn copy_fixture(source: &Path, root: &Path) {
    assert!(
        source.is_dir(),
        "fixture directory missing: {}",
        source.display()
    );
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            ENV_VARS.contains(&name.as_str()),
            "unexpected fixture root {} in {}",
            name,
            source.display()
        );
        copy_tree(&entry.path(), &root.join(&name));
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
            // min_age_days must not hide positive fixtures.
            let modified = SystemTime::now() - Duration::from_secs(365 * 24 * 60 * 60);
            let file = fs::OpenOptions::new().write(true).open(&target).unwrap();
            file.set_times(fs::FileTimes::new().set_modified(modified))
                .unwrap();
        }
    }
}

fn run_fixture(rule_id: &str, kind: &str) -> FixtureOutcome {
    let pack = load_builtin_rules().unwrap();
    let rule = pack
        .rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .unwrap_or_else(|| panic!("built-in rule not found: {rule_id}"))
        .clone();
    let single = RulePack {
        schema_version: pack.schema_version,
        rules: vec![rule],
    };

    let temp = tempfile::tempdir().unwrap();
    let env = prepare_env(temp.path());
    copy_fixture(&fixtures_root().join(rule_id).join(kind), temp.path());

    let result = quick_scan(&single, &env, true).unwrap();
    let paths = result
        .matches
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    FixtureOutcome {
        findings: result.findings.len(),
        matches: paths.len(),
        paths,
    }
}

#[test]
fn new_deep_clean_rules_have_positive_and_redline_fixtures() {
    let pack = load_builtin_rules().unwrap();

    for rule_id in NEW_RULES {
        assert!(
            pack.rules.iter().any(|rule| rule.id == *rule_id),
            "missing built-in rule: {rule_id}"
        );

        let positive = run_fixture(rule_id, "positive");
        assert!(
            positive.findings > 0,
            "{rule_id}: positive fixture produced no finding"
        );
        assert!(
            positive.matches > 0,
            "{rule_id}: positive fixture matched no files"
        );

        let redline = run_fixture(rule_id, "redline");
        assert_eq!(
            redline.matches, 0,
            "{rule_id}: redline fixture matched files: {:?}",
            redline.paths
        );
    }
}
