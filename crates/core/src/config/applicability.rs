//! Select domains supported by an explicitly configured target.
use super::types::Target;
use crate::model::Check;
/// Select meaningful checks while preserving explicit unsupported provider outcomes.
pub fn applicable(target: &Target, check: Check) -> bool {
    use crate::model::Provider;
    match check {
        Check::Slo if target.provider == Provider::Azure => !target.slo_goals.is_empty(),
        Check::Flows => target.flows_required || !target.flows.is_empty(),
        Check::Preflight => true,
        Check::Kubernetes => target.context.is_some() || target.provider == Provider::Kubernetes,
        Check::Edge => {
            !target.endpoints.is_empty()
                || matches!(
                    target.provider,
                    Provider::Gcp
                        | Provider::Aws
                        | Provider::Azure
                        | Provider::Kubernetes
                        | Provider::Edge
                )
        }
        Check::Github => target.provider == Provider::Github || !target.repositories.is_empty(),
        _ => match target.provider {
            Provider::Edge => false,
            Provider::Github => {
                matches!(check, Check::Discovery | Check::Inventory | Check::Releases)
            }
            Provider::Kubernetes => matches!(
                check,
                Check::Inventory | Check::Queues | Check::Releases | Check::Managed
            ),
            Provider::Nats => check == Check::Queues,
            _ => true,
        },
    }
}
