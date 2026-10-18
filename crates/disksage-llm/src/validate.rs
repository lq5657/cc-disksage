use std::collections::HashSet;

use chrono::Utc;
use disksage_core::{Confidence, LlmAssessment, ReasoningStep, Verdict};
use serde::Deserialize;

use crate::error::LlmError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssessmentJson {
    schema_version: u32,
    finding_id: String,
    verdict: Verdict,
    confidence: Confidence,
    summary: String,
    reasoning: Vec<ReasoningJson>,
    cited_evidence_ids: Vec<String>,
    consequence: String,
    caveats: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasoningJson {
    text: String,
    evidence_ids: Vec<String>,
}

/// Validate a provider response against the strict assessment schema.
///
/// The schema has no numeric fields on purpose. Text fields may not contain
/// digits either: every number shown to the user must come from the scanner.
pub fn validate_assessment(
    raw: &str,
    expected_finding_id: &str,
    allowed_evidence_ids: &[String],
    model: &str,
    payload_hash: &str,
) -> Result<LlmAssessment, LlmError> {
    let parsed: AssessmentJson = serde_json::from_str(raw)?;

    if parsed.schema_version != 1 {
        return Err(LlmError::Validation("schema_version must be 1".to_string()));
    }
    if parsed.finding_id != expected_finding_id {
        return Err(LlmError::Validation(
            "finding_id does not match the requested finding".to_string(),
        ));
    }

    let summary = parsed.summary.trim().to_string();
    let consequence = parsed.consequence.trim().to_string();
    if summary.is_empty() || consequence.is_empty() {
        return Err(LlmError::Validation(
            "summary and consequence must not be empty".to_string(),
        ));
    }
    if summary.chars().count() > 600 || consequence.chars().count() > 600 {
        return Err(LlmError::Validation(
            "summary or consequence is too long".to_string(),
        ));
    }
    ensure_no_numbers("summary", &summary)?;
    ensure_no_numbers("consequence", &consequence)?;

    if parsed.reasoning.is_empty() || parsed.reasoning.len() > 8 {
        return Err(LlmError::Validation(
            "reasoning must contain between 1 and 8 steps".to_string(),
        ));
    }
    if parsed.caveats.len() > 8 {
        return Err(LlmError::Validation("too many caveats".to_string()));
    }
    if parsed.cited_evidence_ids.is_empty() || parsed.cited_evidence_ids.len() > 50 {
        return Err(LlmError::Validation(
            "cited_evidence_ids must contain between 1 and 50 ids".to_string(),
        ));
    }

    let allowed = allowed_evidence_ids.iter().cloned().collect::<HashSet<_>>();
    let mut cited = HashSet::new();
    for id in &parsed.cited_evidence_ids {
        if !allowed.contains(id) {
            return Err(LlmError::Validation(format!("unknown evidence id: {id}")));
        }
        cited.insert(id.clone());
    }
    if cited.len() != parsed.cited_evidence_ids.len() {
        return Err(LlmError::Validation(
            "cited_evidence_ids contains duplicates".to_string(),
        ));
    }

    let mut referenced = HashSet::new();
    let mut reasoning = Vec::new();
    for step in parsed.reasoning {
        let text = step.text.trim().to_string();
        if text.is_empty() || step.evidence_ids.is_empty() {
            return Err(LlmError::Validation(
                "each reasoning step needs text and evidence ids".to_string(),
            ));
        }
        if text.chars().count() > 400 {
            return Err(LlmError::Validation(
                "reasoning step is too long".to_string(),
            ));
        }
        ensure_no_numbers("reasoning", &text)?;
        for id in &step.evidence_ids {
            if !allowed.contains(id) {
                return Err(LlmError::Validation(format!(
                    "unknown evidence id in reasoning: {id}"
                )));
            }
            referenced.insert(id.clone());
        }
        reasoning.push(ReasoningStep {
            text,
            evidence_ids: step.evidence_ids,
        });
    }

    if referenced != cited {
        return Err(LlmError::Validation(
            "cited_evidence_ids must match the union of reasoning evidence ids".to_string(),
        ));
    }

    let mut caveats = Vec::new();
    for caveat in parsed.caveats {
        let caveat = caveat.trim().to_string();
        if caveat.is_empty() {
            continue;
        }
        if caveat.chars().count() > 300 {
            return Err(LlmError::Validation("caveat is too long".to_string()));
        }
        ensure_no_numbers("caveat", &caveat)?;
        caveats.push(caveat);
    }

    Ok(LlmAssessment {
        schema_version: parsed.schema_version,
        finding_id: parsed.finding_id,
        verdict: parsed.verdict,
        confidence: parsed.confidence,
        summary,
        reasoning,
        cited_evidence_ids: parsed.cited_evidence_ids,
        consequence,
        caveats,
        model: model.to_string(),
        generated_at: Utc::now().timestamp(),
        payload_hash: payload_hash.to_string(),
    })
}

fn ensure_no_numbers(field: &str, text: &str) -> Result<(), LlmError> {
    if text.chars().any(|character| character.is_numeric()) {
        return Err(LlmError::Validation(format!(
            "{field} must not contain numbers; numbers are injected by the scanner"
        )));
    }
    Ok(())
}
