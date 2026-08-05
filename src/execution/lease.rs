use super::model::CleanupEvidence;
use anyhow::Result;
use std::panic::{catch_unwind, AssertUnwindSafe};

type CleanupAction = Box<dyn FnOnce() -> Result<bool> + Send>;

pub(crate) struct ExecutionLease {
    owner: String,
    resource: String,
    cleanup: Option<CleanupAction>,
}

impl ExecutionLease {
    #[allow(
        dead_code,
        reason = "Phase 2 proves lease semantics before Phase 3 and Phase 4 acquire runtime resources"
    )]
    pub(crate) fn new(
        owner: impl Into<String>,
        resource: impl Into<String>,
        cleanup: impl FnOnce() -> Result<bool> + Send + 'static,
    ) -> Self {
        Self {
            owner: owner.into(),
            resource: resource.into(),
            cleanup: Some(Box::new(cleanup)),
        }
    }

    fn release(mut self) -> CleanupEvidence {
        let result = self.cleanup.take().map(|cleanup| {
            catch_unwind(AssertUnwindSafe(cleanup))
                .map_err(|_| anyhow::anyhow!("cleanup action panicked"))
                .and_then(|result| result)
        });
        match result {
            Some(Ok(verified)) => CleanupEvidence {
                owner: self.owner,
                resource: self.resource,
                released: true,
                verified,
                message: (!verified).then(|| "cleanup verification failed".to_string()),
            },
            Some(Err(error)) => CleanupEvidence {
                owner: self.owner,
                resource: self.resource,
                released: false,
                verified: false,
                message: Some(error.to_string()),
            },
            None => CleanupEvidence {
                owner: self.owner,
                resource: self.resource,
                released: false,
                verified: false,
                message: Some("cleanup action was already consumed".to_string()),
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct LeaseRegistry {
    leases: Vec<ExecutionLease>,
}

impl LeaseRegistry {
    #[allow(
        dead_code,
        reason = "Phase 2 proves lease semantics before Phase 3 and Phase 4 acquire runtime resources"
    )]
    pub(crate) fn register_lease(&mut self, lease: ExecutionLease) {
        self.leases.push(lease);
    }

    pub(crate) fn release_all(&mut self) -> Vec<CleanupEvidence> {
        self.leases
            .drain(..)
            .rev()
            .map(ExecutionLease::release)
            .collect()
    }
}

impl Drop for LeaseRegistry {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecutionLease, LeaseRegistry};
    use std::sync::{Arc, Mutex};

    #[test]
    fn leases_release_in_reverse_order_even_after_cleanup_failure() {
        let released = Arc::new(Mutex::new(Vec::new()));
        let mut leases = LeaseRegistry::default();
        for (name, succeeds) in [("first", true), ("second", false), ("third", true)] {
            let released = Arc::clone(&released);
            leases.register_lease(ExecutionLease::new(name, "fixture", move || {
                released.lock().expect("release log").push(name);
                if succeeds {
                    Ok(true)
                } else {
                    anyhow::bail!("fixture cleanup failure")
                }
            }));
        }
        let evidence = leases.release_all();
        assert_eq!(
            *released.lock().expect("release log"),
            ["third", "second", "first"]
        );
        assert_eq!(evidence.len(), 3);
        assert!(!evidence[1].released);
        assert!(evidence[2].verified);
    }
}
