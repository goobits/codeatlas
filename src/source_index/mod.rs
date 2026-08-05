mod environment;
mod facts;
mod graph;
mod metrics;
mod snapshot;
mod store;

#[cfg(test)]
mod tests;

use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::SourceGraph;
use anyhow::Result;
use metrics::{SourceIndexMeasurement, SourceIndexMetrics};
use serde::{Deserialize, Serialize};
use snapshot::{FileFingerprint, SourceSnapshot};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SOURCE_INDEX_FORMAT_VERSION: u32 = 1;
pub(crate) const SOURCE_INDEX_ALGORITHM_VERSION: u32 = 7;

pub(crate) struct SourceIndex {
    root: Option<PathBuf>,
    max_bytes: u64,
    snapshot: Option<SourceSnapshot>,
    metrics: Mutex<SourceIndexMetrics>,
    untracked_input: AtomicBool,
    untracked_inputs: Mutex<BTreeSet<String>>,
}

#[derive(Serialize, Deserialize)]
struct CacheEnvelope<T> {
    format_version: u32,
    algorithm_version: u32,
    key: String,
    value: T,
}

impl SourceIndex {
    pub(crate) fn open(projects: &[ResolvedAnalysisProject]) -> Result<Self> {
        let environment = environment::resolve(projects)?;
        Self::from_environment(environment, projects)
    }

    fn from_environment(
        environment: environment::SourceIndexEnvironment,
        projects: &[ResolvedAnalysisProject],
    ) -> Result<Self> {
        let snapshot = environment
            .root
            .as_ref()
            .map(|_| snapshot::create(projects))
            .transpose()?;
        Ok(Self {
            root: environment.root,
            max_bytes: environment.max_bytes,
            snapshot,
            metrics: Mutex::new(SourceIndexMetrics::default()),
            untracked_input: AtomicBool::new(false),
            untracked_inputs: Mutex::new(BTreeSet::new()),
        })
    }

    pub(crate) fn finish(&self, status: &str, elapsed: Duration) {
        let cache_bytes = self
            .root
            .as_ref()
            .and_then(|root| store::prune(root, self.max_bytes).ok())
            .unwrap_or_default();
        let metrics = self.metrics.lock().expect("source index metrics");
        let untracked_inputs = self
            .untracked_inputs
            .lock()
            .expect("source index untracked inputs")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let (input_files, input_bytes) = self
            .snapshot
            .as_ref()
            .map(|snapshot| (snapshot.file_count, snapshot.byte_count))
            .unwrap_or_default();
        metrics::emit(SourceIndexMeasurement {
            status,
            input_files,
            input_bytes,
            metrics: &metrics,
            cache_bytes,
            cache_limit_bytes: self.max_bytes,
            elapsed,
            untracked_inputs: &untracked_inputs,
        });
    }

    fn file_fingerprint(&self, path: &Path, relative_path: &str) -> FileFingerprint {
        if let Some(fingerprint) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.files.get(path))
        {
            return fingerprint.clone();
        }
        self.untracked_input.store(true, Ordering::Relaxed);
        self.untracked_inputs
            .lock()
            .expect("source index untracked inputs")
            .insert(relative_path.to_string());
        snapshot::fingerprint(path)
    }

    fn write<T: Serialize>(&self, path: &Path, value: &T) {
        self.record_write(store::write_json(path, value));
    }

    fn record_write(&self, result: Result<u64>) {
        if let Ok(bytes) = result {
            if bytes == 0 {
                return;
            }
            let mut metrics = self.metrics.lock().expect("source index metrics");
            metrics.writes += 1;
            metrics.written_bytes = metrics.written_bytes.saturating_add(bytes);
        }
    }

    #[cfg(test)]
    fn open_at(
        root: PathBuf,
        max_bytes: u64,
        projects: &[ResolvedAnalysisProject],
    ) -> Result<Self> {
        Self::from_environment(environment::for_tests(root, max_bytes), projects)
    }
}

pub(crate) fn build_graph<F>(projects: &[ResolvedAnalysisProject], build: F) -> Result<SourceGraph>
where
    F: FnOnce(&SourceIndex) -> Result<SourceGraph>,
{
    let started = Instant::now();
    let index = SourceIndex::open(projects)?;
    if let Some(graph) = index.load_graph() {
        if graph.validate().is_ok() {
            index.finish("hit", started.elapsed());
            return Ok(graph);
        }
    }
    let graph = build(&index)?;
    index.store_graph(&graph);
    index.finish(
        if index.root.is_some() {
            "miss"
        } else {
            "disabled"
        },
        started.elapsed(),
    );
    Ok(graph)
}
