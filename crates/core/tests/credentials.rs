//! Credential configuration cannot silently ignore provider-specific options.
use monitor_core::config::types::Config;
fn config(profile: &str) -> String {
    format!("version=1\n{profile}\n[[targets]]\nname='target'\nprovider='gcp'\nscope='project'")
}
#[test]
fn unsupported_options_are_rejected() {
    assert!(
        Config::parse(&config(
            "[credentials.google]\nprovider='gcp'\nrole_arn='arn:aws:iam::123456789012:role/read'"
        ))
        .is_err()
    );
    assert!(
        Config::parse(&config(
            "[credentials.google]\nprovider='gcp'\nprofile='ignored-cli-profile'"
        ))
        .is_err()
    );
    assert!(
        Config::parse(&config(
            "[credentials.azure]\nprovider='azure'\nprofile='typo'"
        ))
        .is_err()
    );
}
#[test]
fn credential_files_and_discovery_profiles_are_explicit() {
    assert!(Config::parse(&config("[credentials.google]\nprovider='gcp'\ncredential_file='/tmp/credentials.json'\n[[discovery]]\nprovider='gcp'\nscope='organizations/123'\ncredential='google'")).is_ok());
    assert!(Config::parse(&config("[credentials.azure]\nprovider='azure'\n[[discovery]]\nprovider='gcp'\nscope='organizations/123'\ncredential='azure'")).is_err());
}
#[test]
fn github_credential_files_have_unambiguous_selection() {
    assert!(Config::parse(&config("[credentials.github]\nprovider='github'\ncredential_file='/run/credentials/monitor/github-token'")).is_ok());
    assert!(Config::parse(&config("[credentials.github]\nprovider='github'\ncredential_file='/run/credentials/monitor/github-token'\ntoken_env='GH_TOKEN'")).is_err());
}
#[test]
fn nats_credentials_cannot_be_embedded_in_urls() {
    for url in [
        "tls://server:4222?token=private",
        "tls://server:4222/#private",
        "tls://server:4222/private",
        "tls:server",
        "nats://user:password@server:4222",
    ] {
        let text = format!(
            "version=1\n[[targets]]\nname='nats'\nprovider='nats'\nscope='nats'\nnats_url='{url}'"
        );
        assert!(monitor_core::config::types::Config::parse(&text).is_err());
    }
}
