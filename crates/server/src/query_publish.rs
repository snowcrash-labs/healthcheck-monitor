//! Feed query history from published observations without starting additional collection.
use crate::{query_recorder::Recorder, view::View};
use monitor_core::{
    config::resolve::Effective,
    model::{Snapshot, Transition, TransitionKind},
};
use monitor_history::{error::Error, query_records::QueryRecord};

pub fn records<'a>(
    recorder: &'a mut Recorder,
    snapshot: &'a Snapshot,
    effective: &'a Effective,
    view: &'a View,
    previous: Option<&'a View>,
    transitions: &'a [Transition],
) -> impl Iterator<Item = Result<QueryRecord, Error>> + 'a {
    let checks = effective
        .jobs
        .iter()
        .filter_map(move |job| {
            let result = snapshot.results.get(&job.key)?;
            if previous
                .and_then(|p| p.result_stamps.get(&job.key))
                .is_some_and(|stamp| stamp.matches(result))
            {
                return None;
            }
            let target = view.targets.iter().find(|t| t.name == job.target.name)?;
            Some(crate::query_projection::result(
                result,
                job,
                target,
                &snapshot.health,
            ))
        })
        .flatten();
    let findings = view
        .findings
        .iter()
        .filter_map(|f| crate::query_projection::finding(f, &view.targets));
    enum Input {
        Observation(Box<monitor_query::record::Record>),
        Closure(Transition),
    }
    let input = checks
        .chain(findings)
        .map(|record| Input::Observation(Box::new(record)))
        .chain(
            transitions
                .iter()
                .filter(|t| matches!(t.kind, TransitionKind::Recovered | TransitionKind::Removed))
                .cloned()
                .map(Input::Closure),
        );
    input.flat_map(move |input| match input {
        Input::Observation(record) => match recorder.capture(*record, snapshot.captured_at) {
            Ok(rows) => rows.into_iter().map(Ok).collect::<Vec<_>>(),
            Err(error) => vec![Err(error)],
        },
        Input::Closure(t) => recorder
            .close(&t.finding, t.at, t.kind == TransitionKind::Removed)
            .map(Ok)
            .into_iter()
            .collect(),
    })
}
