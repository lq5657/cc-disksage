use chrono::DateTime;
use disksage_core::{
    EvidenceKind, EvidenceSource, Finding, Recoverability, RiskLevel, SizeProvenance,
};
use serde::Serialize;

use crate::config::LlmConfig;

#[derive(Debug, Clone)]
pub struct RedactContext {
    enabled: bool,
    user_profile: Option<String>,
    username: Option<String>,
    computer_name: Option<String>,
}

impl RedactContext {
    pub fn from_env(enabled: bool) -> Self {
        Self {
            enabled,
            user_profile: std::env::var("USERPROFILE").ok(),
            username: std::env::var("USERNAME").ok(),
            computer_name: std::env::var("COMPUTERNAME").ok(),
        }
    }

    pub fn redact(&self, text: &str) -> String {
        let mut value = text
            .chars()
            .map(|character| {
                if character.is_control() && !matches!(character, '\n' | '\t') {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();

        if !self.enabled {
            return value;
        }

        if let Some(profile) = &self.user_profile
            && !profile.is_empty()
        {
            value = value.replace(profile, "%USERPROFILE%");
        }
        if let Some(computer) = &self.computer_name
            && !computer.is_empty()
        {
            value = value.replace(computer, "%COMPUTERNAME%");
        }
        if let Some(username) = &self.username
            && !username.is_empty()
        {
            value = value.replace(username, "%USERNAME%");
        }
        value
    }
}

#[derive(Debug, Serialize)]
pub struct FindingPayload {
    pub task: &'static str,
    pub finding: FindingFactSheet,
    pub output_schema: &'static str,
}

#[derive(Debug, Serialize)]
pub struct FindingFactSheet {
    pub finding_id: String,
    pub rule_id: String,
    pub rule_version: u32,
    pub title_zh: String,
    pub risk_hint: RiskLevel,
    pub size_provenance: SizeProvenance,
    pub path_template: String,
    pub allocated_bytes: u64,
    pub logical_bytes: u64,
    pub file_count: u64,
    pub oldest_date: Option<String>,
    pub newest_date: Option<String>,
    pub extensions: Vec<ExtensionFact>,
    pub evidence: Vec<EvidenceFact>,
    pub sample_files: Vec<String>,
    pub consequence: String,
    pub recoverability: Recoverability,
}

#[derive(Debug, Serialize)]
pub struct ExtensionFact {
    pub extension: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct EvidenceFact {
    pub id: String,
    pub source: EvidenceSource,
    pub kind: EvidenceKind,
    pub claim: String,
}

pub fn build_finding_payload(
    finding: &Finding,
    config: &LlmConfig,
    redact: &RedactContext,
) -> FindingPayload {
    let path_template = finding
        .targets
        .first()
        .map(|target| target.template.clone())
        .or_else(|| {
            finding
                .evidence
                .iter()
                .find(|evidence| evidence.kind == EvidenceKind::Path)
                .and_then(|evidence| evidence.path.as_ref())
                .map(|path| path.display().to_string())
        })
        .unwrap_or_default();

    let sample_limit = if config.send_sample_names {
        config.sample_name_limit.min(10)
    } else {
        0
    };

    FindingPayload {
        task: "assess_finding",
        finding: FindingFactSheet {
            finding_id: finding.id.clone(),
            rule_id: finding.rule_id.clone(),
            rule_version: finding.rule_version,
            title_zh: redact.redact(&finding.title_zh),
            risk_hint: finding.risk,
            size_provenance: finding.size_provenance,
            path_template: redact.redact(&path_template),
            allocated_bytes: finding.allocated_bytes,
            logical_bytes: finding.logical_bytes,
            file_count: finding.file_count,
            oldest_date: finding.oldest_mtime.and_then(format_timestamp),
            newest_date: finding.newest_mtime.and_then(format_timestamp),
            extensions: finding
                .ext_histogram
                .iter()
                .take(10)
                .map(|(extension, count)| ExtensionFact {
                    extension: extension.clone(),
                    count: *count,
                })
                .collect(),
            evidence: finding
                .evidence
                .iter()
                .map(|evidence| EvidenceFact {
                    id: evidence.id.clone(),
                    source: evidence.source,
                    kind: evidence.kind,
                    claim: redact.redact(&evidence.claim),
                })
                .collect(),
            sample_files: finding
                .sample_files
                .iter()
                .take(sample_limit)
                .map(|file| {
                    let name = redact.redact(&file.name);
                    if name.chars().count() > 120 {
                        format!("{}…", name.chars().take(120).collect::<String>())
                    } else {
                        name
                    }
                })
                .collect(),
            consequence: redact.redact(&finding.consequence),
            recoverability: finding.recoverability,
        },
        output_schema: "assessment-v1",
    }
}

pub fn allowed_evidence_ids(payload: &FindingPayload) -> Vec<String> {
    payload
        .finding
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect()
}

pub fn payload_json(payload: &FindingPayload) -> serde_json::Value {
    serde_json::to_value(payload).expect("finding payload is serializable")
}

pub fn payload_hash(payload: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

fn format_timestamp(timestamp: i64) -> Option<String> {
    DateTime::from_timestamp(timestamp, 0).map(|date| date.format("%Y-%m-%d").to_string())
}
