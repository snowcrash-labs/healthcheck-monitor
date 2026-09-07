//! Validate and project the compact query ledger before journal admission.
use crate::{
    enums,
    error::Error,
    query_schema::query_record,
    types::{Digest, Id, Name, Resource},
};
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use monitor_query::record::Record;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct QueryRecord {
    pub key: Digest,
    pub record: Record,
}
impl QueryRecord {
    pub fn new(key: Digest, record: Record) -> Result<Self, Error> {
        validate(&record)?;
        Ok(Self { key, record })
    }
}
/// Serialized diagnostic payloads have a lower admission bound than PostgreSQL's expanded JSON.
pub fn validate(record: &Record) -> Result<(), Error> {
    Name::try_new(record.scope.target.clone()).map_err(|_| Error::Record)?;
    Resource::try_new(record.scope.scope.clone()).map_err(|_| Error::Record)?;
    Resource::try_new(record.identity.clone()).map_err(|_| Error::Record)?;
    for value in [
        &record.resource,
        &record.location.region,
        &record.location.cluster,
        &record.location.namespace,
        &record.location.service,
    ]
    .into_iter()
    .flatten()
    {
        Resource::try_new(value.clone()).map_err(|_| Error::Record)?;
        if value.len() > 2048 {
            return Err(Error::Record);
        }
    }
    if record
        .location
        .hostname
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.len() > 253)
    {
        return Err(Error::Record);
    }
    if record.last_observed_at < record.observed_at {
        return Err(Error::Record);
    }
    let payload = serde_json::to_value(record).map_err(|_| Error::Record)?;
    fn bounded(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => s.len() <= 4096 && !s.chars().any(char::is_control),
            serde_json::Value::Array(a) => a.len() <= 256 && a.iter().all(bounded),
            serde_json::Value::Object(o) => o.values().all(bounded),
            _ => true,
        }
    }
    if !bounded(&payload) || serde_json::to_vec(record).map_err(|_| Error::Record)?.len() > 16384 {
        return Err(Error::Record);
    }
    Ok(())
}
#[derive(Insertable)]
#[diesel(table_name = query_record)]
pub(crate) struct NewRecord {
    query_record_key: Digest,
    query_record_identity: Resource,
    query_record_category: enums::QueryCategory,
    query_record_provider: enums::QueryProvider,
    query_record_target: Name,
    query_record_scope: Resource,
    query_record_check: Option<enums::Check>,
    query_record_resource: Option<Resource>,
    query_record_region: Option<Resource>,
    query_record_cluster: Option<Resource>,
    query_record_namespace: Option<Resource>,
    query_record_service: Option<Resource>,
    query_record_hostname: Option<String>,
    query_record_severity: Option<enums::Severity>,
    query_record_state: Option<String>,
    query_record_from: DateTime<Utc>,
    query_record_to: DateTime<Utc>,
    query_record_closed_at: Option<DateTime<Utc>>,
    query_record_payload: serde_json::Value,
}
impl TryFrom<&QueryRecord> for NewRecord {
    type Error = Error;
    fn try_from(row: &QueryRecord) -> Result<Self, Error> {
        let r = &row.record;
        let resource = |value: &Option<String>| {
            value
                .clone()
                .map(Resource::try_new)
                .transpose()
                .map_err(|_| Error::Record)
        };
        Ok(Self {
            query_record_key: row.key.clone(),
            query_record_identity: Resource::try_new(r.identity.clone())
                .map_err(|_| Error::Record)?,
            query_record_category: r.category().into(),
            query_record_provider: r.scope.provider.into(),
            query_record_target: Name::try_new(r.scope.target.clone())
                .map_err(|_| Error::Record)?,
            query_record_scope: Resource::try_new(r.scope.scope.clone())
                .map_err(|_| Error::Record)?,
            query_record_check: r.check.map(Into::into),
            query_record_resource: resource(&r.resource)?,
            query_record_region: resource(&r.location.region)?,
            query_record_cluster: resource(&r.location.cluster)?,
            query_record_namespace: resource(&r.location.namespace)?,
            query_record_service: resource(&r.location.service)?,
            query_record_hostname: r.location.hostname.clone(),
            query_record_severity: r.severity().map(Into::into),
            query_record_state: r.state().map(|s| s.as_str().to_ascii_lowercase()),
            query_record_from: r.observed_at,
            query_record_to: r.last_observed_at,
            query_record_closed_at: r.closed_at,
            query_record_payload: serde_json::to_value(r).map_err(|_| Error::Record)?,
        })
    }
}
#[derive(Queryable, Selectable)]
#[diesel(table_name = query_record, check_for_backend(diesel::pg::Pg))]
pub(crate) struct Stored {
    pub query_record_id: Id,
    pub query_record_from: DateTime<Utc>,
    pub query_record_to: DateTime<Utc>,
    pub query_record_closed_at: Option<DateTime<Utc>>,
    pub query_record_payload: serde_json::Value,
}
impl Stored {
    pub fn decode(self) -> Result<Record, Error> {
        let mut record: Record =
            serde_json::from_value(self.query_record_payload).map_err(|_| Error::Record)?;
        record.id = self.query_record_id.as_ref().to_string();
        record.observed_at = self.query_record_from;
        record.last_observed_at = self.query_record_to;
        record.closed_at = self.query_record_closed_at;
        validate(&record)?;
        Ok(record)
    }
}
