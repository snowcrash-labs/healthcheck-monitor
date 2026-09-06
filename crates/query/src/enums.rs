//! Closed query vocabulary shared by JSON and MCP schemas.
macro_rules! vocabulary {
    ($name:ident, [$($variant:ident),+]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => stringify!($variant)),+ }
            }
        }
    };
}
vocabulary!(Provider, [Gcp, Aws, Azure, Kubernetes, Github, Edge, Nats]);
vocabulary!(
    Check,
    [
        Preflight, Discovery, Inventory, Kubernetes, Edge, Managed, Queues, Releases, Github,
        Metrics, Logs, Alerts, Slo, Flows
    ]
);
vocabulary!(Severity, [Info, Warning, Error]);
vocabulary!(
    Health,
    [Healthy, Degraded, Unhealthy, Unknown, ExpectedInactive]
);
vocabulary!(Category, [Finding, Diagnostic, Check, Release]);
vocabulary!(FindingState, [Active, Recovered, Removed]);
vocabulary!(Outcome, [Passing, Failing, Pending, Incomplete]);
