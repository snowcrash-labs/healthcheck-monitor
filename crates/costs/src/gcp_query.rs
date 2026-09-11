//! Fixed parameterized BigQuery statement; only validated table identifiers are interpolated.
use crate::{
    config::{Gcp, identifier},
    error::Error,
    query::Period,
};
use serde_json::{Value, json};

pub fn configuration(
    source: &Gcp,
    billing_scope: &str,
    period: Period,
    bytes: u64,
    rows: usize,
    dry_run: bool,
) -> Result<Value, Error> {
    if ![
        &source.project,
        &source.dataset,
        &source.table,
        &source.location,
    ]
    .iter()
    .all(|v| identifier(v))
        || billing_scope.is_empty()
        || billing_scope.len() > 128
        || rows > 500_000
    {
        return Err(Error::Configuration);
    }
    let resource = if source.detailed {
        "COALESCE(resource.global_name, resource.name, '')"
    } else {
        "''"
    };
    // Billed is the list-price charge; effective subtracts every credit (promotions, discounts,
    // committed use). Keeping both lets the dashboard show spend while a promotion covers it.
    let sql = format!(
        "SELECT CAST(DATE(usage_start_time) AS STRING) AS charge_day, invoice.month AS invoice_month, COALESCE(project.id, '') AS scope, COALESCE(location.region, '') AS region, COALESCE(service.description, service.id, 'Unknown') AS product, {resource} AS resource, cost_type AS category, currency, CAST(SUM(CAST(cost AS NUMERIC)) AS STRING) AS billed, CAST(SUM(CAST(cost AS NUMERIC) + IFNULL((SELECT SUM(CAST(c.amount AS NUMERIC)) FROM UNNEST(credits) c), 0)) AS STRING) AS effective FROM `{}.{}.{}` WHERE billing_account_id = @billing_scope AND usage_start_time >= TIMESTAMP(@from) AND usage_start_time < TIMESTAMP(@to) AND (_PARTITIONTIME IS NULL OR (_PARTITIONTIME >= TIMESTAMP(@from) AND _PARTITIONTIME < TIMESTAMP(@export_to))) GROUP BY charge_day, invoice_month, scope, region, product, resource, category, currency ORDER BY charge_day, invoice_month, scope, region, product, resource, category, currency LIMIT {}",
        source.project,
        source.dataset,
        source.table,
        rows + 1
    );
    let export_to = chrono::Utc::now()
        .date_naive()
        .succ_opt()
        .ok_or(Error::Query)?;
    let parameter = |name: &str, kind: &str, value: String| json!({"name":name,"parameterType":{"type":kind},"parameterValue":{"value":value}});
    Ok(
        json!({"dryRun":dry_run,"query":{"query":sql,"useLegacySql":false,"maximumBytesBilled":bytes.to_string(),"priority":"BATCH","parameterMode":"NAMED","queryParameters":[
            parameter("billing_scope", "STRING", billing_scope.into()), parameter("from","DATE",period.from.to_string()),
            parameter("to","DATE",period.to.to_string()), parameter("export_to","DATE",export_to.to_string())
        ]}}),
    )
}
