//! Operational service details must survive projection while payloads remain excluded.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_providers::{common::Endpoint, details::followups, resource_projection::project};
use serde_json::json;
fn job(provider: &str) -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(&format!("version=1\n[[targets]]\nname='test'\nprovider='{provider}'\nscope='subscription'\nregions=['us-east-1']"))?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
#[test]
fn azure_follows_the_full_resource_id_when_a_short_name_is_present()
-> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::get(
        "service-bus",
        "https://management.azure.com/subscriptions/subscription/providers/Microsoft.ServiceBus/namespaces?api-version=2024-01-01",
        "/value",
    );
    let id = "/subscriptions/subscription/resourceGroups/group/providers/Microsoft.ServiceBus/namespaces/bus";
    let details = followups(&job("azure")?, &endpoint, &json!({"id":id,"name":"bus"}));
    assert!(
        details
            .iter()
            .any(|endpoint| endpoint.url.contains(&format!("{id}/queues?")))
    );
    Ok(())
}
#[test]
fn aws_certificates_use_expiry_and_lambda_details_exclude_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("aws")?;
    let certificate = Endpoint::get(
        "acm-detail/us-east-1/certificate",
        "https://acm.us-east-1.amazonaws.com/",
        "/Certificate",
    );
    let observations = project(
        &job,
        &certificate,
        &json!({"CertificateArn":"certificate","Status":"ISSUED","NotAfter":1800000000}),
    );
    assert!(matches!(
        observations[0].data,
        Data::Certificate {
            issued: Some(true),
            expires_at: Some(_)
        }
    ));
    let function = Endpoint::get(
        "lambda-detail/us-east-1/function",
        "https://lambda.us-east-1.amazonaws.com/2015-03-31/functions/function/configuration",
        "",
    );
    let observations = project(
        &job,
        &function,
        &json!({"FunctionName":"function","State":"Failed","Environment":{"Variables":{"SECRET":"forbidden-secret"}},"StateReason":"customer request forbidden-payload"}),
    );
    assert!(matches!(
        observations[0].data,
        Data::Service {
            state: ServiceState::Failed,
            ..
        }
    ));
    assert!(!serde_json::to_string(&observations)?.contains("forbidden-"));
    Ok(())
}
#[test]
fn aws_alarm_states_include_composite_and_log_alarms_without_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    use aws_sdk_cloudwatch::{
        operation::describe_alarms::DescribeAlarmsOutput,
        types::{CompositeAlarm, LogAlarm, MetricAlarm, StateValue},
    };
    let response = DescribeAlarmsOutput::builder()
        .metric_alarms(
            MetricAlarm::builder()
                .alarm_name("metric")
                .state_value(StateValue::Ok)
                .state_reason("private-metric-reason")
                .build(),
        )
        .composite_alarms(
            CompositeAlarm::builder()
                .alarm_name("composite")
                .state_value(StateValue::Alarm)
                .state_reason("private-composite-reason")
                .build(),
        )
        .log_alarms(
            LogAlarm::builder()
                .alarm_name("logs")
                .state_value(StateValue::InsufficientData)
                .state_reason("private-log-reason")
                .build(),
        )
        .build();
    let observations = monitor_providers::aws_alarms::project(&job("aws")?, "alarms", &response);
    assert_eq!(observations.len(), 3);
    assert!(matches!(
        observations[0].data,
        Data::Condition {
            healthy: Some(true),
            ..
        }
    ));
    assert!(matches!(
        observations[1].data,
        Data::Condition {
            healthy: Some(false),
            ..
        }
    ));
    assert!(matches!(
        observations[2].data,
        Data::Condition { healthy: None, .. }
    ));
    assert!(!serde_json::to_string(&observations)?.contains("private-"));
    Ok(())
}
