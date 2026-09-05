//! Shared admission limits can shrink without forgetting work still in flight.
use crate::error::Error;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
pub struct Budget {
    limit: AtomicUsize,
    used: AtomicUsize,
    changed: Notify,
    admission: tokio::sync::Mutex<()>,
}
pub struct Permit {
    budget: Arc<Budget>,
    amount: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: AtomicUsize::new(limit),
            used: AtomicUsize::new(0),
            changed: Notify::new(),
            admission: tokio::sync::Mutex::new(()),
        }
    }
    /// Existing reservations retain their charge; new admissions wait until they fit.
    pub fn resize(&self, limit: usize) {
        self.limit.store(limit, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
    pub fn available_permits(&self) -> usize {
        self.limit
            .load(Ordering::SeqCst)
            .saturating_sub(self.used.load(Ordering::SeqCst))
    }
    pub fn try_acquire_many_owned(self: Arc<Self>, amount: u32) -> Result<Permit, Error> {
        let amount = amount as usize;
        let mut used = self.used.load(Ordering::SeqCst);
        loop {
            let total = used.checked_add(amount).ok_or(Error::Capacity)?;
            if total > self.limit.load(Ordering::SeqCst) {
                return Err(Error::Capacity);
            }
            match self
                .used
                .compare_exchange_weak(used, total, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => {
                    if total > self.limit.load(Ordering::SeqCst) {
                        self.used.fetch_sub(amount, Ordering::SeqCst);
                        self.changed.notify_waiters();
                        return Err(Error::Capacity);
                    }
                    return Ok(Permit {
                        budget: self,
                        amount,
                    });
                }
                Err(current) => used = current,
            }
        }
    }
    pub async fn acquire(self: &Arc<Self>) -> Result<Permit, Error> {
        // Tokio's FIFO mutex orders helper admission without holding a permit while waiting.
        let _turn = self.admission.lock().await;
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Ok(permit) = self.clone().try_acquire_many_owned(1) {
                return Ok(permit);
            }
            changed.await;
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.amount, Ordering::SeqCst);
        self.budget.changed.notify_waiters();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lowering_limit_keeps_existing_reservations_charged() -> Result<(), Error> {
        let budget = Arc::new(Budget::new(100));
        let first = budget.clone().try_acquire_many_owned(60)?;
        budget.resize(40);
        assert_eq!(budget.available_permits(), 0);
        assert!(budget.clone().try_acquire_many_owned(1).is_err());
        drop(first);
        assert_eq!(budget.available_permits(), 40);
        let _next = budget.clone().try_acquire_many_owned(40)?;
        assert!(budget.clone().try_acquire_many_owned(1).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn cancelled_waiter_does_not_lose_capacity_and_growth_wakes_waiters()
    -> Result<(), Box<dyn std::error::Error>> {
        let budget = Arc::new(Budget::new(1));
        let held = budget.acquire().await?;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), budget.acquire())
                .await
                .is_err()
        );
        let waiting = budget.clone();
        let task = tokio::spawn(async move { waiting.acquire().await });
        budget.resize(2);
        let next = task.await??;
        assert_eq!(budget.available_permits(), 0);
        drop(held);
        drop(next);
        assert_eq!(budget.available_permits(), 2);
        Ok(())
    }
}
