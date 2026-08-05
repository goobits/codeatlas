use super::model::ResourceEvidence;
use std::time::Instant;

pub(crate) struct ResourceSampler {
    started: Instant,
}

impl ResourceSampler {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    pub(crate) fn sample_resources(&self, mut observed: ResourceEvidence) -> ResourceEvidence {
        observed.elapsed_ms = observed
            .elapsed_ms
            .max(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX));
        observed
    }
}
