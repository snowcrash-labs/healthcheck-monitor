//! GitHub history windows and smaller pull-request pages keep routine diagnostics bounded.
use crate::transport::{Error, Http};
use monitor_core::config::resolve::Job;
pub fn page_size(job: &Job, id: &str) -> usize {
    job.settings
        .page_size
        .min(if id.starts_with("pulls/") { 25 } else { 100 })
}
pub fn list(
    http: &Http,
    job: &Job,
    token: &str,
    id: &str,
    path: &str,
    page: usize,
) -> Result<reqwest::Request, Error> {
    let mut request = http
        .client()
        .get(format!("https://api.github.com/{path}"))
        .query(&[("per_page", page_size(job, id)), ("page", page)])
        .bearer_auth(token)
        .header("X-GitHub-Api-Version", "2022-11-28");
    if id.starts_with("workflows/") {
        request = request.query(&[(
            "created",
            format!(
                ">={}",
                (chrono::Utc::now()
                    - chrono::Duration::seconds(job.settings.runtime_window.0 as i64))
                .to_rfc3339()
            ),
        )]);
    }
    request.build().map_err(|_| Error::Malformed)
}
pub fn pipeline(value: &serde_json::Value) -> Option<&str> {
    crate::projection::text(value, &["/path", "/name"])
        .map(|path| path.split('@').next().unwrap_or(path))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_windows_and_pr_pages_are_explicit_and_refs_do_not_change_workflow_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let job = monitor_core::config::types::Config::parse(
            "version=1\n[[targets]]\nname='source'\nprovider='github'\nscope='example'",
        )?
        .resolve(&Default::default())?
        .jobs
        .into_iter()
        .next()
        .ok_or("missing job")?;
        let http = Http::new(&job.settings)?;
        let request = list(
            &http,
            &job,
            "synthetic",
            "workflows/repo",
            "repos/example/repo/actions/runs",
            1,
        )?;
        assert!(
            request
                .url()
                .query_pairs()
                .any(|(key, value)| key == "created" && value.starts_with(">="))
        );
        assert_eq!(page_size(&job, "pulls/repo"), 25);
        assert_eq!(
            pipeline(&serde_json::json!({"path":".github/workflows/build.yml@main"})),
            Some(".github/workflows/build.yml")
        );
        Ok(())
    }
}
