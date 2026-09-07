//! Native PostgreSQL enums match the closed monitoring vocabulary.
macro_rules! mapped {
    ($name:ident, $sql:literal, $core:path, [$($variant:ident),+]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, diesel_derive_enum::DbEnum)]
        #[serde(rename_all = "snake_case")]
        #[ExistingTypePath = $sql]
        pub enum $name { $($variant),+ }
        impl From<$core> for $name {
            fn from(value: $core) -> Self { use $core as Core; match value { $(Core::$variant => Self::$variant),+ } }
        }
        impl From<$name> for $core {
            fn from(value: $name) -> Self { match value { $($name::$variant => Self::$variant),+ } }
        }
    };
}
mapped!(
    Kind,
    "crate::schema::sql_types::EventKind",
    monitor_core::model::TransitionKind,
    [New, Worsened, Recovered, Stale, Removed, Reappeared]
);
mapped!(
    QueryCategory,
    "crate::query_schema::sql_types::QueryCategory",
    monitor_query::enums::Category,
    [Finding, Diagnostic, Check, Release]
);
mapped!(
    QueryProvider,
    "crate::query_schema::sql_types::QueryProvider",
    monitor_query::enums::Provider,
    [Gcp, Aws, Azure, Kubernetes, Github, Edge, Nats, Unknown]
);

impl From<monitor_query::enums::Check> for Check {
    fn from(value: monitor_query::enums::Check) -> Self {
        use monitor_query::enums::Check as Q;
        match value {
            Q::Preflight => Self::Preflight,
            Q::Discovery => Self::Discovery,
            Q::Inventory => Self::Inventory,
            Q::Kubernetes => Self::Kubernetes,
            Q::Edge => Self::Edge,
            Q::Managed => Self::Managed,
            Q::Queues => Self::Queues,
            Q::Releases => Self::Releases,
            Q::Github => Self::Github,
            Q::Metrics => Self::Metrics,
            Q::Logs => Self::Logs,
            Q::Alerts => Self::Alerts,
            Q::Slo => Self::Slo,
            Q::Flows => Self::Flows,
        }
    }
}
impl From<monitor_query::enums::Severity> for Severity {
    fn from(value: monitor_query::enums::Severity) -> Self {
        match value {
            monitor_query::enums::Severity::Info => Self::Info,
            monitor_query::enums::Severity::Warning => Self::Warning,
            monitor_query::enums::Severity::Error => Self::Error,
        }
    }
}
mapped!(
    Severity,
    "crate::schema::sql_types::Severity",
    monitor_core::model::Severity,
    [Info, Warning, Error]
);
mapped!(
    Expected,
    "crate::schema::sql_types::Expected",
    monitor_core::model::Expected,
    [Active, Dormant, ScaleToZero, Suspended]
);
mapped!(
    Confidence,
    "crate::schema::sql_types::Confidence",
    monitor_core::model::Confidence,
    [Direct, Correlated, Insufficient]
);
mapped!(
    Check,
    "crate::schema::sql_types::CheckKind",
    monitor_core::model::Check,
    [
        Preflight, Discovery, Inventory, Kubernetes, Edge, Managed, Queues, Releases, Github,
        Metrics, Logs, Alerts, Slo, Flows
    ]
);
