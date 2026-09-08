//! Optional native Cost Explorer bootstrap; exported ledgers remain the preferred durable source.
use crate::auth::Auth;
use aws_sdk_costexplorer::types::{
    DateInterval, Granularity, GroupDefinition, GroupDefinitionType,
};
use monitor_core::{config::settings::Settings, model::Provider as CoreProvider};
use monitor_costs::{
    config::Source,
    model::{Amount, Charge, Provider},
    query::Period,
};
use monitor_integrations::{
    http_pool::Pools,
    process::Processes,
    transport::{Error, Http},
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct Reader {
    client: aws_sdk_costexplorer::Client,
}
pub struct Page {
    pub rows: Vec<Charge>,
    pub next: Option<String>,
}
impl Reader {
    pub async fn new(source: &Source, pools: Arc<Pools>) -> Result<Self, Error> {
        let options = source.aws_query.as_ref().ok_or(Error::Forbidden)?;
        let settings = Settings {
            response_bytes: 8 * 1024 * 1024,
            ..Default::default()
        };
        let http = Http::shared(pools, &settings)?;
        let auth = Auth::new(
            CoreProvider::Aws,
            source.credential.as_ref(),
            Some(&options.region),
            Some(&source.billing_scope),
            &http,
            &settings,
            &Processes::new(1),
        )
        .await?;
        let Auth::Aws(clients) = auth else {
            return Err(Error::Authentication);
        };
        Ok(Self {
            client: clients
                .service(
                    "cost-explorer",
                    &options.region,
                    &settings,
                    aws_sdk_costexplorer::Client::new,
                )
                .await?,
        })
    }
    pub async fn page(
        &self,
        period: Period,
        cursor: Option<String>,
        stop: &CancellationToken,
    ) -> Result<Page, Error> {
        if cursor.as_ref().is_some_and(|s| s.len() > 8192) {
            return Err(Error::Limit);
        }
        let period = DateInterval::builder()
            .start(period.from.to_string())
            .end(period.to.to_string())
            .build()
            .map_err(|_| Error::Malformed)?;
        let dimension = |key| {
            GroupDefinition::builder()
                .key(key)
                .r#type(GroupDefinitionType::Dimension)
                .build()
        };
        let request = self
            .client
            .get_cost_and_usage()
            .time_period(period)
            .granularity(Granularity::Daily)
            .metrics("UnblendedCost")
            .group_by(dimension("SERVICE"))
            .group_by(dimension("LINKED_ACCOUNT"))
            .set_next_page_token(cursor);
        let response = tokio::select! { _=stop.cancelled()=>return Err(Error::Cancelled), response=request.send()=>response.map_err(|_| Error::Unavailable)? };
        decode(response)
    }
}
fn decode(
    response: aws_sdk_costexplorer::operation::get_cost_and_usage::GetCostAndUsageOutput,
) -> Result<Page, Error> {
    let days = response.results_by_time.ok_or(Error::Missing)?;
    let mut rows = Vec::new();
    for day in days {
        let date = day
            .time_period
            .as_ref()
            .ok_or(Error::Malformed)?
            .start()
            .parse()
            .map_err(|_| Error::Malformed)?;
        for group in day.groups.ok_or(Error::Missing)? {
            if rows.len() >= 5000 {
                return Err(Error::Limit);
            }
            let keys = group.keys();
            if keys.len() != 2 {
                return Err(Error::Malformed);
            }
            let metric = group
                .metrics()
                .and_then(|m| m.get("UnblendedCost"))
                .ok_or(Error::Missing)?;
            let amount = Amount::provider(metric.amount().ok_or(Error::Missing)?)
                .map_err(|_| Error::Malformed)?;
            let charge = Charge {
                target: None,
                day: date,
                invoice_month: None,
                provider: Provider::Aws,
                scope: Some(keys[1].clone()),
                region: None,
                product: keys[0].clone(),
                resource: None,
                category: None,
                currency: metric.unit().ok_or(Error::Missing)?.into(),
                billed: amount,
                effective: None,
            };
            charge.validate().map_err(|_| Error::Malformed)?;
            rows.push(charge);
        }
    }
    Ok(Page {
        rows,
        next: response.next_page_token,
    })
}
