//! Operation admission is shared across checks and does not reserve unrelated scope capacity.
use monitor_core::config::{resolve::Effective, types::Config};
use monitor_integrations::admission::{Limits, acquire};
use std::time::Duration;
fn effective() -> Result<Effective, Box<dyn std::error::Error>> {
    Ok(Config::parse("version=1\n[settings]\nconcurrency=4\nscope_concurrency=2\n[[targets]]\nname='a'\nprovider='edge'\nscope='a'\n[[targets]]\nname='b'\nprovider='edge'\nscope='b'")?.resolve(&Default::default())?)
}
#[tokio::test(start_paused = true)]
async fn busy_scope_does_not_occupy_global_slots_and_cancellation_releases_waiters()
-> Result<(), Box<dyn std::error::Error>> {
    let effective = effective()?;
    let limits = Limits::new(&effective);
    let a = limits
        .context(
            effective
                .jobs
                .iter()
                .find(|job| job.target.name == "a")
                .ok_or("a")?,
        )
        .await?;
    let b = limits
        .context(
            effective
                .jobs
                .iter()
                .find(|job| job.target.name == "b")
                .ok_or("b")?,
        )
        .await?;
    let first = a.run(acquire()).await?;
    let second = a.run(acquire()).await?;
    let blocked = a.run(async { tokio::time::timeout(Duration::from_secs(1), acquire()).await });
    let independent = b.run(async {
        let first = acquire().await?;
        let second = acquire().await?;
        Ok::<_, monitor_integrations::transport::Error>((first, second))
    });
    let (blocked, independent) = tokio::join!(blocked, independent);
    assert!(blocked.is_err());
    drop(independent?);
    drop(first);
    drop(second);
    assert!(
        tokio::time::timeout(Duration::from_millis(1), a.run(acquire()))
            .await
            .is_ok()
    );
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn reload_preserves_in_flight_charges_when_limits_shrink()
-> Result<(), Box<dyn std::error::Error>> {
    let mut effective = effective()?;
    let limits = Limits::new(&effective);
    let job = effective.jobs.first().ok_or("job")?;
    let old = limits.context(job).await?;
    let first = old.run(acquire()).await?;
    let second = old.run(acquire()).await?;
    for job in &mut effective.jobs {
        job.settings.concurrency = 2;
        job.settings.scope_concurrency = 1;
    }
    limits.reload(&effective).await;
    let current = limits.context(effective.jobs.first().ok_or("job")?).await?;
    drop(first);
    assert!(
        tokio::time::timeout(Duration::from_millis(1), current.run(acquire()))
            .await
            .is_err()
    );
    drop(second);
    assert!(
        tokio::time::timeout(Duration::from_millis(1), current.run(acquire()))
            .await
            .is_ok()
    );
    Ok(())
}
