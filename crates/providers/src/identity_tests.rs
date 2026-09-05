//! Identity constraints fail closed without retaining token contents.
use base64::Engine;
#[test]
fn azure_identity_assertions_use_the_principal_not_the_cli_application_id() {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"oid":"principal","upn":"operator@example.test","appid":"shared-cli"}"#);
    let token = format!("header.{payload}.signature");
    assert!(super::auth_identity::azure(&token, "principal").is_ok());
    assert!(super::auth_identity::azure(&token, "OPERATOR@example.test").is_ok());
    assert!(super::auth_identity::azure(&token, "shared-cli").is_err());
    assert!(super::auth_identity::azure("opaque-token", "principal").is_err());
}
