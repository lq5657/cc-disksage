use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use disksage_core::path_safety::{
    expand_env, is_forbidden_delete_target, is_strict_descendant, is_system_red_line,
};
use disksage_core::{ActionSpec, EnvMap, GuideId};
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::error::RuleError;
use crate::schema::{Rule, Target};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

const SKIPPED_DIR_NAMES: &[&str] = &[
    "$Recycle.Bin",
    "System Volume Information",
    "WindowsApps",
    "Recovery",
];

#[derive(Debug, Clone)]
pub struct CompiledTarget {
    pub template: String,
    pub root: PathBuf,
    pub include: GlobSet,
    pub match_all: bool,
    pub exclude: GlobSet,
    pub max_depth: usize,
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub rule: Rule,
    pub targets: Vec<CompiledTarget>,
    pub action: ActionSpec,
}

impl CompiledRule {
    /// Walk all targets and return the exact files that match this rule.
    /// The result is deduplicated and sorted so plans are deterministic.
    pub fn matched_files(&self, env: &EnvMap) -> Vec<(PathBuf, u64)> {
        let cutoff = if self.rule.min_age_days == 0 {
            None
        } else {
            SystemTime::now().checked_sub(Duration::from_secs(
                u64::from(self.rule.min_age_days) * 86_400,
            ))
        };

        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for target in &self.targets {
            for (path, bytes) in walk_target(target, env, cutoff) {
                if seen.insert(path.clone()) {
                    files.push((path, bytes));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }
}

pub fn compile_rules(
    pack: &crate::schema::RulePack,
    env: &EnvMap,
    include_admin: bool,
) -> Result<Vec<CompiledRule>, RuleError> {
    let mut compiled = Vec::new();

    for rule in &pack.rules {
        if rule.requires_admin && !include_admin {
            continue;
        }
        if !detect_matches(&rule.detect, env) {
            continue;
        }

        let mut targets = Vec::new();
        for target in &rule.targets {
            if let Some(compiled_target) = compile_target(target, env)? {
                targets.push(compiled_target);
            }
        }
        if targets.is_empty() {
            continue;
        }

        let action = rule.action.to_action_spec(env)?;
        compiled.push(CompiledRule {
            rule: rule.clone(),
            targets,
            action,
        });
    }

    Ok(compiled)
}

fn detect_matches(detect: &crate::schema::Detect, env: &EnvMap) -> bool {
    let any = detect.path_exists_any.is_empty()
        || detect
            .path_exists_any
            .iter()
            .any(|path| expand_env(path, env).is_ok_and(|path| path.exists()));
    let all = detect.path_exists_all.is_empty()
        || detect
            .path_exists_all
            .iter()
            .all(|path| expand_env(path, env).is_ok_and(|path| path.exists()));
    any && all
}

fn compile_target(target: &Target, env: &EnvMap) -> Result<Option<CompiledTarget>, RuleError> {
    let root = match expand_env(&target.root, env) {
        Ok(root) => root,
        Err(disksage_core::CoreError::EnvMissing(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    if !root.is_dir() {
        return Ok(None);
    }
    if is_forbidden_delete_target(&root, env) {
        return Err(RuleError::Validation(format!(
            "target root is a protected deletion target: {}",
            root.display()
        )));
    }

    let include = build_glob_set(&target.include_globs)?;
    let exclude = build_glob_set(&target.exclude_globs)?;
    Ok(Some(CompiledTarget {
        template: target.root.clone(),
        root,
        include,
        match_all: target.include_globs.is_empty(),
        exclude,
        max_depth: target.max_depth,
    }))
}

fn build_glob_set(patterns: &[String]) -> Result<GlobSet, RuleError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}

fn is_reparse(path: &Path) -> bool {
    let Ok(metadata) = path.symlink_metadata() else {
        return true;
    };
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn should_skip_dir(path: &Path) -> bool {
    if is_reparse(path) {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            SKIPPED_DIR_NAMES
                .iter()
                .any(|skipped| skipped.eq_ignore_ascii_case(name))
        })
}

fn to_slash(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

fn walk_target(
    target: &CompiledTarget,
    env: &EnvMap,
    age_cutoff: Option<SystemTime>,
) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();

    let walker = WalkDir::new(&target.root)
        .max_depth(target.max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !entry.file_type().is_dir() || !should_skip_dir(entry.path()));

    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_reparse(path) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&target.root) else {
            continue;
        };
        let relative_slash = to_slash(relative);
        if !target.match_all && !target.include.is_match(&relative_slash) {
            continue;
        }
        if target.exclude.is_match(&relative_slash) {
            continue;
        }
        if is_system_red_line(path, env) || is_forbidden_delete_target(path, env) {
            continue;
        }
        if !is_strict_descendant(path, &target.root) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if let Some(cutoff) = age_cutoff {
            match metadata.modified() {
                Ok(modified) if modified > cutoff => continue,
                Err(_) => continue,
                _ => {}
            }
        }
        files.push((path.to_path_buf(), metadata.len()));
    }

    files
}

/// Validate that a path is still inside the declared root and not protected.
pub fn path_is_allowed(path: &Path, target: &CompiledTarget, env: &EnvMap) -> bool {
    is_strict_descendant(path, &target.root)
        && !is_system_red_line(path, env)
        && !is_forbidden_delete_target(path, env)
}

/// Map a schema action to a display-safe guide identifier.
pub fn guide_label(guide: GuideId) -> &'static str {
    match guide {
        GuideId::WindowsStorageSettings => "settings:storage",
        GuideId::WindowsUpdateSettings => "settings:windows-update",
        GuideId::SystemProtection => "settings:system-protection",
        GuideId::PagefileSettings => "settings:pagefile",
        GuideId::CleanMgr => "tool:cleanmgr",
    }
}
