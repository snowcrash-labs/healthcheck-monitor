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
#[test]
fn azure_vm_power_and_dead_letter_counts_are_operational_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("azure")?;
    let vm = Endpoint::get(
        "vm-instance-view/vm",
        "https://management.azure.com/vm/instanceView",
        "",
    );
    let observations = project(
        &job,
        &vm,
        &json!({"statuses":[{"code":"ProvisioningState/succeeded"},{"code":"PowerState/deallocated","message":"private-value"}]}),
    );
    assert!(matches!(
        observations[0].data,
        Data::Service {
            state: ServiceState::Stopped,
            ..
        }
    ));
    assert!(!serde_json::to_string(&observations)?.contains("private-value"));
    let queue = Endpoint::get(
        "service-bus-queues/bus",
        "https://management.azure.com/bus/queues",
        "/value",
    );
    let observations = project(
        &job,
        &queue,
        &json!({"name":"work","properties":{"countDetails":{"activeMessageCount":2,"deadLetterMessageCount":3,"transferDeadLetterMessageCount":0}}}),
    );
    assert!(observations.iter().any(|obs| matches!(
        obs.data,
        Data::Metric {
            value: 3.0,
            warning: Some(1.0),
            ..
        }
    )));
    Ok(())
}
#[test]
fn active_failed_revisions_are_not_mistaken_for_intentional_scale_to_zero()
-> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::get(
        "container-revisions/app",
        "https://management.azure.com/app/revisions",
        "/value",
    );
    let failed = project(
        &job("azure")?,
        &endpoint,
        &json!({"name":"revision","properties":{"active":true,"replicas":0,"healthState":"Unhealthy"}}),
    );
    assert_eq!(failed[0].expected, Expected::Active);
    let inactive = project(
        &job("azure")?,
        &endpoint,
        &json!({"name":"revision","properties":{"active":false,"replicas":0}}),
    );
    assert_eq!(inactive[0].expected, Expected::ScaleToZero);
    Ok(())
}
