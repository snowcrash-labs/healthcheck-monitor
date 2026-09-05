//! Default prerequisite sampling follows checks that require repeated observations.
use super::resolve::Job;
use crate::{error::Error, model::Check};
pub fn sampling(jobs: &mut [Job]) -> Result<(), Error> {
    let sources: Vec<_> = jobs
        .iter()
        .filter(|job| {
            job.check == Check::Queues || job.check == Check::Flows && !job.target.flows.is_empty()
        })
        .map(|job| (job.target.name.clone(), job.check, job.settings.clone()))
        .collect();
    for job in jobs.iter_mut() {
        for (target, check, settings) in &sources {
            if target != &job.target.name {
                continue;
            }
            let dependent = job.check == Check::Kubernetes
                || *check == Check::Flows && matches!(job.check, Check::Metrics | Check::Queues);
            if dependent && !job.sampling_explicit {
                job.settings.samples = job.settings.samples.max(settings.samples);
                job.settings.sample_interval =
                    job.settings.sample_interval.min(settings.sample_interval);
            }
            if *check == Check::Flows && job.check == Check::Metrics && !job.interval_explicit {
                job.settings.interval = job.settings.interval.min(settings.interval);
            }
        }
    }
    for (target, check, settings) in sources {
        if check == Check::Flows
            && jobs.iter().any(|job| {
                job.target.name == target
                    && job.check == Check::Metrics
                    && job.settings.interval.0 > settings.freshness()
            })
        {
            return Err(Error::Config(
                "flow freshness must cover its metric collection interval".into(),
            ));
        }
    }
    Ok(())
}
