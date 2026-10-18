use std::fs;
use std::path::Path;

use disksage_core::RiskLevel;

use crate::error::RuleError;
use crate::schema::{RulePack, SUPPORTED_SCHEMA_VERSION, validate_pack};

const BUILTIN_PACKS: &[(&str, &str)] = &[
    (
        "system.toml",
        include_str!("../../../rules/builtin/system.toml"),
    ),
    (
        "browsers.toml",
        include_str!("../../../rules/builtin/browsers.toml"),
    ),
    ("dev.toml", include_str!("../../../rules/builtin/dev.toml")),
    (
        "apps.toml",
        include_str!("../../../rules/builtin/apps.toml"),
    ),
    (
        "system_admin.toml",
        include_str!("../../../rules/builtin/system_admin.toml"),
    ),
    (
        "system_maintenance.toml",
        include_str!("../../../rules/builtin/system_maintenance.toml"),
    ),
];

pub fn parse_rule_pack(pack_name: &str, text: &str) -> Result<RulePack, RuleError> {
    let pack: RulePack = toml::from_str(text).map_err(|error| RuleError::Parse {
        pack: pack_name.to_string(),
        message: error.to_string(),
    })?;
    validate_pack(&pack)?;
    Ok(pack)
}

pub fn load_builtin_rules() -> Result<RulePack, RuleError> {
    let mut rules = Vec::new();
    for (pack_name, text) in BUILTIN_PACKS {
        let pack = parse_rule_pack(pack_name, text)?;
        rules.extend(pack.rules);
    }
    let pack = RulePack {
        schema_version: SUPPORTED_SCHEMA_VERSION,
        rules,
    };
    validate_pack(&pack)?;
    Ok(pack)
}

/// Load optional user rules. User rules are additive and restricted to
/// non-privileged trash/delete/quarantine actions.
pub fn load_user_rules(directory: &Path) -> Result<RulePack, RuleError> {
    if !directory.exists() {
        return Ok(RulePack {
            schema_version: SUPPORTED_SCHEMA_VERSION,
            rules: Vec::new(),
        });
    }

    let mut paths = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut rules = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path)?;
        let name = path.display().to_string();
        let pack = parse_rule_pack(&name, &text)?;
        for rule in pack.rules {
            if !rule.id.starts_with("user.") {
                return Err(RuleError::Validation(format!(
                    "user rule {} must use the user. prefix",
                    rule.id
                )));
            }
            if rule.requires_admin {
                return Err(RuleError::Validation(format!(
                    "user rule {} may not require administrator",
                    rule.id
                )));
            }
            if rule.default_selected {
                return Err(RuleError::Validation(format!(
                    "user rule {} may not be selected by default",
                    rule.id
                )));
            }
            if rule.risk == RiskLevel::Danger {
                return Err(RuleError::Validation(format!(
                    "user rule {} may not be marked danger",
                    rule.id
                )));
            }
            if !rule.action.is_user_rule_safe() {
                return Err(RuleError::Validation(format!(
                    "user rule {} may only use trash/delete/quarantine",
                    rule.id
                )));
            }
            rules.push(rule);
        }
    }

    let pack = RulePack {
        schema_version: SUPPORTED_SCHEMA_VERSION,
        rules,
    };
    validate_pack(&pack)?;
    Ok(pack)
}
