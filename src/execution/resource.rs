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

    pub(crate) fn sample_resources(&self) -> ResourceEvidence {
        ResourceEvidence {
            elapsed_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ..ResourceEvidence::default()
        }
    }
}
