use disksage_llm::{delete_key, get_key, set_key};

#[test]
#[ignore = "writes a temporary credential to Windows Credential Manager"]
fn credential_manager_roundtrip() {
    let provider = "cc-disksage-test-provider";
    let _ = delete_key(provider);

    set_key(provider, "test-secret-value").expect("set key");
    assert_eq!(
        get_key(provider).expect("get key").as_deref(),
        Some("test-secret-value")
    );

    delete_key(provider).expect("delete key");
    assert!(get_key(provider).expect("get after delete").is_none());
}
