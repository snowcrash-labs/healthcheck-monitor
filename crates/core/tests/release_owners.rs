//! Runtime image provenance follows owner UIDs within the selected cluster target.
pub mod support;
use monitor_core::{model::*, releases};
use support::release::{fixture, observation, result, trace};
#[test]
fn controller_images_follow_pod_and_replica_set_ownership() -> Result<(), Box<dyn std::error::Error>>
{
    for target in ["dev", "other"] {
        let (job, mut snapshot, mut raw, now) = fixture()?;
        let kube = snapshot.results.get_mut("dev/Kubernetes").ok_or("kube")?;
        let mut pod = kube.observations[0].clone();
        pod.resource = format!("{target}/pods/ns/app/image/app");
        let mut controller = pod.clone();
        controller.resource = "dev/deployments/ns/app/image/app".into();
        controller.operation = "deployments".into();
        if let Data::Image {
            observed_digest, ..
        } = &mut controller.data
        {
            *observed_digest = None;
        }
        let mut observations = vec![
            controller,
            observation(
                "dev/deployments/ns/app/owner",
                "deployments",
                Data::Owner {
                    uid: "deployment".into(),
                    owner_uid: None,
                },
                now,
            ),
        ];
        observations.extend([
            pod,
            observation(
                &format!("{target}/pods/ns/app/owner"),
                "pods",
                Data::Owner {
                    uid: "pod".into(),
                    owner_uid: Some("replicaset".into()),
                },
                now,
            ),
            observation(
                &format!("{target}/replicasets/ns/app/owner"),
                "replicasets",
                Data::Owner {
                    uid: "replicaset".into(),
                    owner_uid: Some("deployment".into()),
                },
                now,
            ),
        ]);
        snapshot.results.insert(
            "dev/Kubernetes".into(),
            result("dev", Check::Kubernetes, observations, now),
        );
        releases::enrich(&snapshot, &job, &mut raw, now);
        let count = trace(&raw).and_then(|obs| match &obs.data {
            Data::Provenance {
                observed_digests, ..
            } => Some(observed_digests.len()),
            _ => None,
        });
        assert_eq!(count, Some(usize::from(target == "dev")));
    }
    Ok(())
}
