//! Assertion identity checks reject cross-cloud, cross-subject, stale, and malformed tokens.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use monitor_core::config::types::GoogleFederation;
use serde_json::json;
#[test]
fn assertion_claims_bind_subject_audience_and_authorized_party()
-> Result<(), Box<dyn std::error::Error>> {
    let expected = GoogleFederation {
        subject: "111111111111111111111".into(),
        audience: "https://health.example/aws/123456789012".into(),
        client_id: None,
    };
    let value = json!({"iss":"https://accounts.google.com","sub":expected.subject,"aud":expected.audience,"azp":expected.subject,"exp":4600});
    let token = |value: &serde_json::Value| -> Result<String, serde_json::Error> {
        Ok(format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(value)?)
        ))
    };
    crate::google_assertion::claims(&token(&value)?, &expected, true, 1000)?;
    for (key, replacement) in [
        ("sub", json!("222222222222222222222")),
        ("aud", json!("api://AzureADTokenExchange")),
        ("azp", json!("222222222222222222222")),
        ("iss", json!("https://other.example")),
        ("exp", json!(1000)),
    ] {
        let mut bad = value.clone();
        bad[key] = replacement;
        assert!(crate::google_assertion::claims(&token(&bad)?, &expected, true, 1000).is_err());
    }
    assert!(crate::google_assertion::claims("malformed", &expected, true, 1000).is_err());
    assert!(crate::google_assertion::claims(&"x".repeat(16385), &expected, true, 1000).is_err());
    Ok(())
}
