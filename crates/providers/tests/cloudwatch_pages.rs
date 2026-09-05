//! Native metric pages retain explicit partial and bounded outcomes.
use aws_sdk_cloudwatch::{
    primitives::DateTime,
    types::{MetricDataResult, StatusCode},
};
use monitor_core::model::Coverage;
use monitor_providers::aws_metric_batch::Series;
#[test]
fn partial_pages_can_complete_without_losing_points() {
    let mut series = Series::default();
    let first = MetricDataResult::builder()
        .id("m0")
        .status_code(StatusCode::PartialData)
        .timestamps(DateTime::from_secs(1))
        .values(5.0)
        .build();
    let second = MetricDataResult::builder()
        .id("m0")
        .status_code(StatusCode::Complete)
        .timestamps(DateTime::from_secs(2))
        .values(6.0)
        .build();
    series.record(&first, 10);
    series.record(&second, 10);
    assert_eq!(series.coverage, Coverage::Complete);
    assert_eq!(series.points.len(), 2);
}
#[test]
fn limits_and_malformed_pairs_cannot_become_complete() {
    let mut series = Series::default();
    let row = MetricDataResult::builder()
        .status_code(StatusCode::Complete)
        .timestamps(DateTime::from_secs(1))
        .timestamps(DateTime::from_secs(2))
        .values(1.0)
        .values(2.0)
        .build();
    series.record(&row, 1);
    assert_eq!(series.coverage, Coverage::Truncated);
    let row = MetricDataResult::builder()
        .status_code(StatusCode::Complete)
        .timestamps(DateTime::from_secs(1))
        .build();
    let mut malformed = Series::default();
    malformed.record(&row, 10);
    assert_eq!(malformed.coverage, Coverage::Malformed);
}
#[test]
fn an_internal_error_cannot_be_erased_by_a_later_complete_page() {
    let mut series = Series::default();
    series.record(
        &MetricDataResult::builder()
            .status_code(StatusCode::InternalError)
            .build(),
        10,
    );
    series.record(
        &MetricDataResult::builder()
            .status_code(StatusCode::Complete)
            .timestamps(DateTime::from_secs(1))
            .values(5.0)
            .build(),
        10,
    );
    assert_eq!(series.coverage, Coverage::Unavailable);
    assert_eq!(series.points.len(), 1);
}
