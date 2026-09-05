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
    };
}
mapped!(
    Kind,
    "crate::schema::sql_types::EventKind",
    monitor_core::model::TransitionKind,
    [New, Worsened, Recovered, Stale, Removed, Reappeared]
);
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
