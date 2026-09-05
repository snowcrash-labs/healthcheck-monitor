//! Service-specific statistics and global CloudFront metric routing.
use monitor_core::{
    config::{
        resolve::Job,
        types::{Aggregation, MetricQuery},
    },
    model::Check,
};
/// Capacity minima establish sustained pressure; unknown gauges retain their latest period.
pub fn aggregation(namespace: &str, name: &str) -> Aggregation {
    if crate::metric_catalog::percent_metric(name) {
        return Aggregation::Minimum;
    }
    if namespace == "AWS/CloudFront" {
        return if matches!(name, "Requests" | "BytesDownloaded" | "BytesUploaded") {
            Aggregation::Sum
        } else {
            Aggregation::Average
        };
    }
    if matches!(
        name,
        "RequestCount"
            | "Invocations"
            | "Errors"
            | "Throttles"
            | "NumberOfMessagesSent"
            | "NumberOfMessagesReceived"
            | "NumberOfMessagesDeleted"
            | "NumberOfNotificationsDelivered"
            | "NumberOfNotificationsFailed"
            | "FailedInvocations"
            | "HTTPCode_ELB_5XX_Count"
            | "HTTPCode_Target_5XX_Count"
    ) {
        return Aggregation::Sum;
    }
    Aggregation::Latest
}
pub fn regions(job: &Job) -> Vec<String> {
    let mut regions = job.target.regions.clone();
    if job.check != Check::Queues
        && (job.target.metrics.is_empty()
            || job
                .target
                .metrics
                .iter()
                .any(|query| query.namespace == "AWS/CloudFront"))
        && !regions.iter().any(|region| region == "us-east-1")
    {
        regions.push("us-east-1".into());
    }
    regions
}
pub fn namespace_in_region(job: &Job, namespace: &str, region: &str) -> bool {
    if namespace == "AWS/CloudFront" {
        region == "us-east-1" && job.check != Check::Queues
    } else {
        job.target
            .regions
            .iter()
            .any(|configured| configured == region)
    }
}
pub fn configured(job: &Job, region: &str) -> Vec<MetricQuery> {
    job.target
        .metrics
        .iter()
        .filter(|query| namespace_in_region(job, &query.namespace, region))
        .cloned()
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::config::{resolve::Selection, types::Config};
    #[test]
    fn capacities_gauges_and_event_counts_use_different_statistics() {
        assert!(matches!(
            aggregation("AWS/EC2", "CPUUtilization"),
            Aggregation::Minimum
        ));
        assert!(matches!(
            aggregation("AWS/SQS", "ApproximateNumberOfMessagesVisible"),
            Aggregation::Latest
        ));
        assert!(matches!(
            aggregation("AWS/Lambda", "Errors"),
            Aggregation::Sum
        ));
        assert!(matches!(
            aggregation("AWS/CloudFront", "5xxErrorRate"),
            Aggregation::Average
        ));
    }
    #[test]
    fn cloudfront_adds_only_its_documented_global_metric_region()
    -> Result<(), Box<dyn std::error::Error>> {
        let job = Config::parse("version=1\n[[targets]]\nname='aws'\nprovider='aws'\nscope='123456789012'\nregions=['eu-west-1']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Metrics).ok_or("missing metrics")?;
        assert_eq!(regions(&job), vec!["eu-west-1", "us-east-1"]);
        assert!(namespace_in_region(&job, "AWS/CloudFront", "us-east-1"));
        assert!(!namespace_in_region(&job, "AWS/CloudFront", "eu-west-1"));
        assert!(!namespace_in_region(&job, "AWS/RDS", "us-east-1"));
        Ok(())
    }
}
