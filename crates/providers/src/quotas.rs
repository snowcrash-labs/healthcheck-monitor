//! Provider quota denominators feed native batched usage queries.
use crate::{auth::Auth,common::Endpoint};
use monitor_core::{config::{resolve::Job,types::MetricQuery},model::*};
use monitor_integrations::{projection::{text,number,observation,operation},transport::Error};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;
pub fn project(job:&Job,endpoint:&Endpoint,row:&serde_json::Value)->Vec<Observation>{
    let Some(limit)=number(row,&["/Value"]).filter(|v|*v>0.0)else{return vec![]};
    let code=text(row,&["/QuotaCode"]).unwrap_or("unknown");
    let region=endpoint.aws.as_ref().map(|(_,region,_)|region.clone()).unwrap_or_default();
    let usage=text(row,&["/UsageMetric/MetricNamespace"]).zip(text(row,&["/UsageMetric/MetricName"])).map(|(namespace,name)|{
        let dimensions=row.pointer("/UsageMetric/MetricDimensions").and_then(|v|v.as_object()).map(|m|m.iter().filter_map(|(k,v)|v.as_str().map(|v|(k.clone(),v.to_string()))).collect()).unwrap_or_default();
        MetricQuery{aggregation:Default::default(),name:format!("quota-usage/{code}"),namespace:namespace.into(),metric:name.into(),resource:code.into(),dimensions,capacity:Some(limit),warning:None,error:None}
    });
    vec![observation(job,&endpoint.id,code,Data::Quota{region,code:code.into(),limit,usage})]
}
pub async fn evaluate(auth:&Auth,job:&Job,result:&mut CheckResult,cancel:&CancellationToken){
    let mut regions:BTreeMap<String,Vec<MetricQuery>>=BTreeMap::new();
    for observation in &result.observations {
        if let Data::Quota{region,usage:Some(query),..}=&observation.data { regions.entry(region.clone()).or_default().push(query.clone()); }
    }
    for(region,queries)in regions{
        let mut metrics=job.clone();metrics.target.regions=vec![region];metrics.target.metrics=queries;metrics.check=Check::Metrics;
        let collected=crate::aws_metrics::aws(auth,&metrics,cancel).await;
        result.operations.extend(collected.operations);result.observations.extend(collected.observations);
    }
    if result.observations.iter().any(|observation|matches!(observation.data,Data::Quota{usage:None,..})){
        result.operations.push(operation("quota-usage-unavailable",Err(&Error::Missing),0,true));
    }
}

