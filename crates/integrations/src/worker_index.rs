//! Owner links are indexed once per queue collection, including replaced pods and ReplicaSets.
use monitor_core::model::{Data, Observation};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) struct Workers {
    workloads: BTreeMap<String, (u32, u32, bool)>,
}
impl Workers {
    pub fn new(observations: &[Observation]) -> Self {
        let mut owners = BTreeMap::new();
        let mut parents = BTreeMap::new();
        for observation in observations {
            if let Data::Owner { uid, owner_uid } = &observation.data {
                owners.insert(observation.resource.as_str(), uid.as_str());
                if let Some(parent) = owner_uid {
                    parents.insert(uid.as_str(), parent.as_str());
                }
            }
        }
        let mut crashes = BTreeSet::new();
        for observation in observations {
            if let Data::Pod {
                uid,
                crash_loop: true,
                ..
            } = &observation.data
            {
                let mut uid = uid.as_str();
                crashes.insert(uid);
                // Match the bounded Deployment -> ReplicaSet -> Pod ancestry policy.
                for _ in 0..3 {
                    let Some(parent) = parents.get(uid) else {
                        break;
                    };
                    crashes.insert(*parent);
                    uid = parent;
                }
            }
        }
        let mut workloads = BTreeMap::new();
        for observation in observations {
            if let Data::Workload {
                desired,
                ready,
                node: false,
                ..
            } = &observation.data
            {
                let mut parts = observation.resource.rsplitn(3, '/');
                let (Some(name), Some(namespace)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let owner = format!("{}/owner", observation.resource);
                let crash = owners
                    .get(owner.as_str())
                    .is_some_and(|uid| crashes.contains(uid));
                workloads
                    .entry(format!("{namespace}/{name}"))
                    .or_insert((*desired, *ready, crash));
            }
        }
        Self { workloads }
    }
    pub fn get(&self, namespace: &str, name: &str) -> Option<(u32, u32, bool)> {
        self.workloads.get(&format!("{namespace}/{name}")).copied()
    }
}
