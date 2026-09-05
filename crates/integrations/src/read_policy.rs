//! Closed read-only request policy, including documented query POST endpoints.
use reqwest::Request;
/// Deny secret retrieval, message consumption, object bodies, and arbitrary POST calls.
pub fn allowed(request: &Request) -> bool {
    let path = request.url().path().to_ascii_lowercase();
    if request.url().scheme() != "https"
        || path.contains(":access")
        || path.contains(":decrypt")
        || path.contains("/exec")
        || path.contains("/attach")
        || path.contains("/proxy")
        || path.contains(":pull")
        || path.contains(":acknowledge")
        || path.contains("/listkeys")
        || path.contains("/listsecrets")
    {
        return false;
    }
    match *request.method() {
        reqwest::Method::GET | reqwest::Method::HEAD => {
            let query = request.url().query().unwrap_or("").to_ascii_lowercase();
            !query.contains("alt=media")
                && !path.contains("/objects/")
                && (!path.contains("/secrets/") || path.ends_with("/versions"))
        }
        reqwest::Method::POST => {
            if request.url().host_str().is_some_and(|host| {
                host.starts_with("application-signals.") && host.ends_with(".api.aws")
            }) && matches!(path.as_str(), "/slos" | "/budget-report")
            {
                return true;
            }
            if request
                .url()
                .host_str()
                .is_some_and(|host| host.ends_with(".amazonaws.com"))
                && path
                    .strip_prefix("/service/graniteserviceversion20100801/operation/")
                    .is_some_and(|operation| {
                        matches!(
                            operation,
                            "getmetricdata" | "listmetrics" | "describealarms"
                        )
                    })
            {
                return true;
            }
            if request.url().host_str() == Some("api.loganalytics.azure.com")
                && path.starts_with("/v1/workspaces/")
                && path.ends_with("/query")
            {
                return true;
            }
            if request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                == Some("application/x-www-form-urlencoded")
            {
                let body = request.body().and_then(|b| b.as_bytes()).unwrap_or(&[]);
                let action = url::form_urlencoded::parse(body)
                    .find(|(k, _)| k == "Action")
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
                return [
                    "GetMetricData",
                    "ListMetrics",
                    "DescribeAlarms",
                    "DescribeInstances",
                    "DescribeInstanceStatus",
                    "DescribeVolumes",
                    "DescribeAutoScalingGroups",
                    "DescribeLoadBalancers",
                    "DescribeTargetGroups",
                    "DescribeTargetHealth",
                    "DescribeDBInstances",
                    "DescribeDBClusters",
                    "DescribeCacheClusters",
                    "DescribeReplicationGroups",
                    "ListTopics",
                    "GetTopicAttributes",
                    "DescribeRegions",
                    "DescribeAccountAttributes",
                ]
                .contains(&action.as_str());
            }
            if path == "/v2/entries:list"
                || path.ends_with("/providers/microsoft.resourcegraph/resources")
                || path.ends_with("/gethealth")
            {
                return true;
            }
            let target = request
                .headers()
                .get("x-amz-target")
                .and_then(|s| s.to_str().ok())
                .unwrap_or("");
            let action = target.rsplit('.').next().unwrap_or("");
            const READS: &[&str] = &[
                "ListMetrics",
                "ListCertificates",
                "DescribeCertificate",
                "ListSecretVersionIds",
                "DescribeTable",
                "ListTables",
                "DescribeClusters",
                "DescribeServices",
                "ListClusters",
                "ListServices",
                "ListFunctions",
                "DescribeLogGroups",
                "FilterLogEvents",
                "ListQueues",
                "GetQueueAttributes",
                "ListRules",
                "ListEventBuses",
                "ListEventSourceMappings",
                "DescribeAlarms",
                "GetMetricData",
                "ListAccounts",
                "DescribeOrganization",
                "DescribeRepositories",
                "ListImages",
                "DescribeImages",
                "ListBuilds",
                "BatchGetBuilds",
                "ListPipelines",
                "GetPipelineState",
                "ListBackupVaults",
                "ListRecoveryPointsByBackupVault",
                "ListKeys",
                "DescribeKey",
                "ListSecrets",
                "ListServiceQuotas",
                "ListServices",
                "DescribeEvents",
                "DescribeAffectedEntities",
                "DescribeSubscriptionFilters",
            ];
            READS.contains(&action)
        }
        _ => false,
    }
}
