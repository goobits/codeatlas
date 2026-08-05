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
            Some(result) => self.complete(result),
            None => CleanupEvidence {
                owner: self.owner,
                resource: self.resource,
                released: false,
                verified: false,
                message: Some("cleanup action was already consumed".to_string()),
            },
        }
    }

    fn complete(self, result: Result<bool>) -> CleanupEvidence {
        match result {
            Ok(verified) => CleanupEvidence {
                owner: self.owner,
                resource: self.resource,
                released: true,
                verified,
                message: (!verified).then(|| "cleanup verification failed".to_string()),
            },
            Err(error) => CleanupEvidence {
                owner: self.owner,
                resource: self.resource,
                released: false,
                verified: false,
                message: Some(error.to_string()),
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct LeaseRegistry {
    leases: Vec<ExecutionLease>,
    completed: Vec<CleanupEvidence>,
}

impl LeaseRegistry {
    pub(crate) fn register_lease(&mut self, lease: ExecutionLease) {
        self.leases.push(lease);
    }

    pub(crate) fn complete_latest_verified(&mut self) -> Result<CleanupEvidence> {
        let lease = self
            .leases
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No execution lease is available to complete"))?;
        let evidence = lease.complete(Ok(true));
        self.completed.push(evidence.clone());
        Ok(evidence)
    }

    pub(crate) fn release_latest(&mut self) -> Result<CleanupEvidence> {
        let lease = self
            .leases
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No execution lease is available to release"))?;
        let evidence = lease.release();
        self.completed.push(evidence.clone());
        Ok(evidence)
    }

    pub(crate) fn release_all(&mut self) -> Vec<CleanupEvidence> {
        self.completed
            .extend(self.leases.drain(..).rev().map(ExecutionLease::release));
        std::mem::take(&mut self.completed)
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

    #[test]
    fn externally_completed_latest_lease_is_not_cleaned_twice() {
        let cleanup_ran = Arc::new(Mutex::new(false));
        let observed = Arc::clone(&cleanup_ran);
        let mut leases = LeaseRegistry::default();
        leases.register_lease(ExecutionLease::new("fixture", "container", move || {
            *observed.lock().expect("cleanup observer") = true;
            Ok(true)
        }));

        let completed = leases
            .complete_latest_verified()
            .expect("complete latest lease");
        assert!(completed.released);
        assert!(completed.verified);
        let evidence = leases.release_all();
        assert_eq!(evidence, [completed]);
        assert!(!*cleanup_ran.lock().expect("cleanup observer"));
    }
}
