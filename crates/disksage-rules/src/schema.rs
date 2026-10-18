use std::collections::HashSet;

use disksage_core::{ActionSpec, EnvMap, GuideId, Recoverability, RiskLevel};
use serde::Deserialize;

use crate::error::RuleError;

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;
pub const MAX_RULES: usize = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulePack {
    pub schema_version: u32,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub version: u32,
    #[serde(default)]
    pub verified_at: Option<String>,
    #[serde(default)]
    pub verified_on: Option<String>,
    pub category: String,
    pub title_zh: String,
    pub title_en: String,
    pub risk: RiskLevel,
    pub requires_admin: bool,
    pub default_selected: bool,
    pub recoverability: Recoverability,
    #[serde(default)]
    pub min_age_days: u32,
    #[serde(default)]
    pub detect: Detect,
    pub targets: Vec<Target>,
    pub explain: Explain,
    pub action: RuleAction,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detect {
    #[serde(default)]
    pub path_exists_any: Vec<String>,
    #[serde(default)]
    pub path_exists_all: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Environment variables are allowed. Wildcards are rejected so that the
    /// deletion root is always a literal, verified directory.
    pub root: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub follow_reparse_points: bool,
}

fn default_max_depth() -> usize {
    32
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Explain {
    pub what_zh: String,
    pub what_en: String,
    pub impact_zh: String,
    pub impact_en: String,
    #[serde(default)]
    pub exclude_notes_zh: String,
    #[serde(default)]
    pub exclude_notes_en: String,
    pub source: String,
}

/// Fixed action enum. Rules can reference these variants but can never carry
/// arbitrary commands or arguments.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleAction {
    Trash,
    Delete,
    Quarantine,
    EmptyRecycleBin,
    ServiceCleanup {
        services: Vec<String>,
        paths: Vec<String>,
    },
    DismAnalyze,
    DismStartComponentCleanup,
    Guided {
        guide: GuideId,
    },
    HibernateOff,
    CreateRestorePoint,
}

impl RuleAction {
    pub fn to_action_spec(&self, env: &EnvMap) -> Result<ActionSpec, RuleError> {
        Ok(match self {
            Self::Trash => ActionSpec::Trash,
            Self::Delete => ActionSpec::Delete,
            Self::Quarantine => ActionSpec::Quarantine,
            Self::EmptyRecycleBin => ActionSpec::EmptyRecycleBin,
            Self::ServiceCleanup { services, paths } => ActionSpec::ServiceCleanup {
                services: services.clone(),
                paths: paths
                    .iter()
                    .map(|path| disksage_core::path_safety::expand_env(path, env))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            Self::DismAnalyze => ActionSpec::DismAnalyze,
            Self::DismStartComponentCleanup => ActionSpec::DismStartComponentCleanup,
            Self::Guided { guide } => ActionSpec::Guided(*guide),
            Self::HibernateOff => ActionSpec::HibernateOff,
            Self::CreateRestorePoint => ActionSpec::CreateRestorePoint,
        })
    }

    /// User-authored rules may only use non-privileged file actions.
    pub fn is_user_rule_safe(&self) -> bool {
        matches!(self, Self::Trash | Self::Delete | Self::Quarantine)
    }
}

pub fn validate_pack(pack: &RulePack) -> Result<(), RuleError> {
    if pack.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(RuleError::Validation(format!(
            "unsupported schema_version {}, expected {}",
            pack.schema_version, SUPPORTED_SCHEMA_VERSION
        )));
    }
    if pack.rules.len() > MAX_RULES {
        return Err(RuleError::Validation(format!(
            "too many rules: {} (max {MAX_RULES})",
            pack.rules.len()
        )));
    }

    let mut ids = HashSet::new();
    for rule in &pack.rules {
        validate_rule(rule)?;
        if !ids.insert(rule.id.clone()) {
            return Err(RuleError::Validation(format!(
                "duplicate rule id: {}",
                rule.id
            )));
        }
    }
    Ok(())
}

fn validate_rule(rule: &Rule) -> Result<(), RuleError> {
    if rule.id.is_empty()
        || !rule.id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        return Err(RuleError::Validation(format!(
            "invalid rule id (use [a-z0-9._-]): {}",
            rule.id
        )));
    }
    if rule.version == 0 {
        return Err(RuleError::Validation(format!(
            "rule {} must have version >= 1",
            rule.id
        )));
    }
    if rule.default_selected && rule.risk != RiskLevel::Safe {
        return Err(RuleError::Validation(format!(
            "rule {} is not safe but is selected by default",
            rule.id
        )));
    }
    if rule.targets.is_empty() {
        return Err(RuleError::Validation(format!(
            "rule {} has no targets",
            rule.id
        )));
    }
    if rule.targets.len() > 64 {
        return Err(RuleError::Validation(format!(
            "rule {} has too many targets",
            rule.id
        )));
    }
    if rule.explain.source.trim().is_empty() {
        return Err(RuleError::Validation(format!(
            "rule {} must document a path source",
            rule.id
        )));
    }
    if rule.min_age_days > 3650 {
        return Err(RuleError::Validation(format!(
            "rule {} has an implausible min_age_days",
            rule.id
        )));
    }

    for target in &rule.targets {
        if target.root.trim().is_empty() {
            return Err(RuleError::Validation(format!(
                "rule {} has an empty target root",
                rule.id
            )));
        }
        if target.root.contains('*') || target.root.contains('?') {
            return Err(RuleError::Validation(format!(
                "rule {} target root must be a literal path: {}",
                rule.id, target.root
            )));
        }
        if target.follow_reparse_points {
            return Err(RuleError::Validation(format!(
                "rule {} may not follow reparse points",
                rule.id
            )));
        }
        if target.max_depth == 0 || target.max_depth > 128 {
            return Err(RuleError::Validation(format!(
                "rule {} has an invalid max_depth",
                rule.id
            )));
        }
    }

    Ok(())
}
