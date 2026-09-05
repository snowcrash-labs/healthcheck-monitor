//! Reference-only target selection stays within explicitly configured targets.
use super::{
    resolve::{Job, Selection},
    types::Config,
};
use crate::{error::Error, model::Check};
use std::collections::BTreeSet;
impl Config {
    pub(super) fn reference_jobs(
        &self,
        selection: &Selection,
        jobs: &mut Vec<Job>,
        revision: &str,
        metadata_only: bool,
    ) -> Result<(), Error> {
        if !metadata_only {
            let dependencies: Vec<_> = jobs
                .iter()
                .filter(|job| job.check == Check::Releases)
                .flat_map(|job| &job.target.artifact_targets)
                .filter(|target| {
                    !jobs
                        .iter()
                        .any(|job| job.target.name == **target && job.check == Check::Releases)
                })
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            if !dependencies.is_empty() {
                let mut dependencies = self.resolve_inner(
                    &Selection {
                        profile: selection.profile.clone(),
                        targets: dependencies,
                        checks: vec![Check::Releases],
                        resources: selection.resources.clone(),
                        overrides: selection.overrides.clone(),
                    },
                    true,
                )?;
                for job in &mut dependencies.jobs {
                    job.revision = revision.to_owned();
                    job.assess_health = false;
                }
                jobs.extend(dependencies.jobs);
            }
        }
        if !metadata_only
            && jobs.iter().any(|job| {
                job.check == Check::Releases
                    && job.target.provider != crate::model::Provider::Github
            })
        {
            let sources: Vec<_> = self
                .targets
                .iter()
                .filter(|target| {
                    target.provider == crate::model::Provider::Github
                        && !jobs.iter().any(|job| {
                            job.target.name == target.name
                                && matches!(
                                    job.check,
                                    Check::Github | Check::Releases | Check::Inventory
                                )
                        })
                })
                .map(|target| target.name.clone())
                .collect();
            if !sources.is_empty() {
                let mut sources = self.resolve(&Selection {
                    profile: selection.profile.clone(),
                    targets: sources,
                    checks: vec![Check::Github],
                    resources: vec![],
                    overrides: selection.overrides.clone(),
                })?;
                for job in &mut sources.jobs {
                    job.revision = revision.to_owned();
                    job.assess_health = false;
                }
                jobs.extend(sources.jobs);
            }
        }
        Ok(())
    }
}
