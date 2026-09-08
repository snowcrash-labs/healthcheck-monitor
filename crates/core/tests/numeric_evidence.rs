//! Stored metric values must remain readable when billing dependencies change JSON features.
use monitor_core::model::Data;

#[test]
#[ignore = "requires HEALTHCHECK_SNAPSHOT_FIXTURE pointing to a private saved snapshot"]
fn saved_snapshot_remains_readable() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("HEALTHCHECK_SNAPSHOT_FIXTURE")?;
    let snapshot = monitor_core::storage::read(std::path::Path::new(&path), 128 * 1024 * 1024)?;
    assert!(!snapshot.results.is_empty());
    Ok(())
}

#[test]
fn metric_json_round_trip_preserves_fractional_and_exponent_values()
-> Result<(), Box<dyn std::error::Error>> {
    for value in [0.16677720329728665, 0.75, 1.0, 1e-12, 1e20] {
        let metric = Data::Metric {
            name: "utilization".into(),
            value,
            capacity: Some(1.0),
            warning: None,
            error: None,
            window_seconds: 60,
        };
        let bytes = serde_json::to_vec(&metric)?;
        let decoded: Data = serde_json::from_slice(&bytes)?;
        let Data::Metric { value: actual, .. } = decoded else {
            return Err("wrong observation variant".into());
        };
        assert_eq!(actual, value);
    }
    Ok(())
}
