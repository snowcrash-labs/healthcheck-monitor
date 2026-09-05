//! Canonical image references keep registry digests and source revisions distinct.
pub fn canonical(reference: &str) -> Option<String> {
    let reference = reference
        .strip_prefix("docker-pullable://")
        .unwrap_or(reference);
    let name = reference.split('@').next()?;
    let slash = name.rfind('/').map_or(0, |index| index + 1);
    let name = if let Some(colon) = name[slash..].find(':') {
        &name[..slash + colon]
    } else {
        name
    };
    if name.is_empty()
        || name.len() > 1024
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._/:".contains(&byte))
    {
        return None;
    }
    let first = name.split('/').next()?;
    Some(if !name.contains('/') {
        format!("docker.io/library/{name}")
    } else if first.contains(['.', ':']) || first == "localhost" {
        name.to_owned()
    } else {
        format!("docker.io/{name}")
    })
}
pub fn tag(reference: &str) -> Option<&str> {
    if reference.contains('@') {
        return None;
    }
    reference
        .rsplit('/')
        .next()?
        .split_once(':')
        .map(|(_, tag)| tag)
}
pub fn digest(reference: &str) -> Option<&str> {
    let digest = reference
        .rsplit_once('@')
        .map_or(reference, |(_, digest)| digest);
    let (algorithm, hex) = digest.split_once(':')?;
    ((algorithm == "sha256" && hex.len() == 64 || algorithm == "sha512" && hex.len() == 128)
        && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(digest)
}
pub fn revision_matches(short: &str, full: &str) -> bool {
    (7..=40).contains(&short.len())
        && full.len() == 40
        && short
            .bytes()
            .chain(full.bytes())
            .all(|byte| byte.is_ascii_hexdigit())
        && full
            .get(..short.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(short))
}
