use std::path::PathBuf;

use disksage_core::{
    ActionSpec, Evidence, EvidenceKind, EvidenceSource, Finding, MatchedPath, Recoverability,
    RiskLevel, SampleFile, SizeProvenance,
};
use disksage_llm::{
    LlmClient, LlmConfig, RedactContext, build_finding_payload, payload_json, validate_assessment,
};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_finding() -> Finding {
    let profile = std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\alice".into());
    Finding {
        id: "f1".into(),
        rule_id: "test.cache".into(),
        rule_version: 1,
        category: "test".into(),
        title_zh: "测试缓存".into(),
        title_en: "Test cache".into(),
        risk: RiskLevel::Safe,
        recoverability: Recoverability::Regenerable,
        requires_admin: false,
        default_selected: true,
        targets: vec![MatchedPath {
            template: r"%TEMP%\cache".into(),
            resolved: PathBuf::from(format!(r"{profile}\AppData\Local\Temp\cache")),
            bytes: 100,
            file_count: 2,
        }],
        allocated_bytes: 100,
        logical_bytes: 100,
        file_count: 2,
        size_provenance: SizeProvenance::Scanner,
        oldest_mtime: Some(1_700_000_000),
        newest_mtime: Some(1_700_000_100),
        ext_histogram: vec![(".tmp".into(), 2)],
        sample_files: vec![SampleFile {
            name: "secret-report.docx".into(),
            bytes: 50,
            modified_at: None,
        }],
        evidence: vec![
            Evidence {
                id: "E1".into(),
                source: EvidenceSource::Rule,
                kind: EvidenceKind::RuleProvenance,
                claim: "命中规则 test.cache".into(),
                path: None,
                bytes: None,
                timestamp: None,
                rule_id: Some("test.cache".into()),
                confidence: 1.0,
            },
            Evidence {
                id: "E2".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::Size,
                claim: "扫描器实测 100 B".into(),
                path: None,
                bytes: Some(100),
                timestamp: None,
                rule_id: None,
                confidence: 1.0,
            },
            Evidence {
                id: "E3".into(),
                source: EvidenceSource::Scanner,
                kind: EvidenceKind::Path,
                claim: format!("目标路径 {profile}\\AppData\\Local\\Temp\\cache"),
                path: Some(PathBuf::from(format!(
                    r"{profile}\AppData\Local\Temp\cache"
                ))),
                bytes: None,
                timestamp: None,
                rule_id: None,
                confidence: 1.0,
            },
        ],
        consequence: "删除后应用会自动重建".into(),
        action: ActionSpec::Trash,
        llm: None,
    }
}

fn valid_assessment() -> String {
    serde_json::to_string(&json!({
        "schema_version": 1,
        "finding_id": "f1",
        "verdict": "cleanable",
        "confidence": "high",
        "summary": "这是可重建的测试缓存",
        "reasoning": [
            {"text": "规则与扫描器都表明它是缓存", "evidence_ids": ["E1", "E2"]}
        ],
        "cited_evidence_ids": ["E1", "E2"],
        "consequence": "删除后应用会自动重建",
        "caveats": ["应用正在使用时可能跳过部分文件"]
    }))
    .unwrap()
}

fn test_config(base_url: String) -> LlmConfig {
    LlmConfig {
        provider_id: "test".into(),
        base_url,
        model: "mock-model".into(),
        temperature: 0.0,
        max_tokens: 512,
        timeout_secs: 10,
        redact_paths: true,
        send_sample_names: false,
        sample_name_limit: 0,
    }
}

fn allowed() -> Vec<String> {
    vec!["E1".into(), "E2".into(), "E3".into()]
}

#[test]
fn accepts_valid_assessment() {
    let assessment =
        validate_assessment(&valid_assessment(), "f1", &allowed(), "mock", "hash").unwrap();
    assert_eq!(assessment.verdict, disksage_core::Verdict::Cleanable);
    assert_eq!(assessment.confidence, disksage_core::Confidence::High);
    assert_eq!(assessment.cited_evidence_ids, vec!["E1", "E2"]);
}

#[test]
fn rejects_unknown_evidence_ids() {
    let text = valid_assessment().replace("\"E2\"", "\"E99\"");
    let error = validate_assessment(&text, "f1", &allowed(), "mock", "hash").unwrap_err();
    assert!(error.to_string().contains("unknown evidence id"));
}

#[test]
fn rejects_numbers_in_prose() {
    let text = valid_assessment().replace("这是可重建的测试缓存", "可清理 1.2 GB");
    let error = validate_assessment(&text, "f1", &allowed(), "mock", "hash").unwrap_err();
    assert!(error.to_string().contains("must not contain numbers"));
}

#[test]
fn rejects_unknown_fields() {
    let mut value: serde_json::Value = serde_json::from_str(&valid_assessment()).unwrap();
    value["extra"] = json!(true);
    let error =
        validate_assessment(&value.to_string(), "f1", &allowed(), "mock", "hash").unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn payload_redacts_profile_and_hides_sample_names_by_default() {
    let finding = sample_finding();
    let config = test_config("https://example.com/v1".into());
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    let payload = build_finding_payload(&finding, &config, &RedactContext::from_env(true));
    let text = payload_json(&payload).to_string();

    assert!(!text.contains("secret-report.docx"));
    if !profile.is_empty() {
        assert!(!text.contains(&profile.replace('\\', "\\\\")));
        assert!(text.contains("%USERPROFILE%"));
    }
}

#[test]
fn payload_includes_sample_names_when_opted_in() {
    let finding = sample_finding();
    let mut config = test_config("https://example.com/v1".into());
    config.send_sample_names = true;
    config.sample_name_limit = 1;
    let payload = build_finding_payload(&finding, &config, &RedactContext::from_env(true));
    assert_eq!(payload.finding.sample_files, vec!["secret-report.docx"]);
}

#[tokio::test]
async fn assesses_finding_with_openai_compatible_mock() {
    let server = MockServer::start().await;
    let body = json!({
        "choices": [{"message": {"content": valid_assessment()}}]
    });
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let client = LlmClient::new(
        test_config(format!("{}/v1", server.uri())),
        "test-key".into(),
    )
    .unwrap();
    let assessment = client.assess_finding(&sample_finding()).await.unwrap();
    assert_eq!(assessment.finding_id, "f1");
    assert_eq!(assessment.verdict, disksage_core::Verdict::Cleanable);
}

#[tokio::test]
async fn rejects_mock_response_with_fabricated_numbers() {
    let server = MockServer::start().await;
    let invalid = valid_assessment().replace("这是可重建的测试缓存", "可清理 1.2 GB");
    let body = json!({"choices": [{"message": {"content": invalid}}]});
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let client = LlmClient::new(
        test_config(format!("{}/v1", server.uri())),
        "test-key".into(),
    )
    .unwrap();
    assert!(client.assess_finding(&sample_finding()).await.is_err());
}
