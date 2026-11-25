//! A signal to stop a running job.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A cancel signal that can be cloned and handed to another thread.
///
/// Every clone shares one flag. Call [`CancelToken::cancel`] from anywhere and every clone sees it
/// from then on. A token cannot be reset, make a new one for the next job.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that has not been cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the job to stop. Safe to call more than once.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether [`CancelToken::cancel`] was called on this token or any clone of it.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Whether `other` is this token or a clone of it.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// `Err(Error::Cancelled)` when cancelled, so a backend can write `cancel.check()?` between
    /// its steps.
    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_the_flag() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        assert!(b.check().is_ok());
        a.cancel();
        assert!(b.is_cancelled());
        assert!(matches!(b.check(), Err(crate::Error::Cancelled)));
    }
}
