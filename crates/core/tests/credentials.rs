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
