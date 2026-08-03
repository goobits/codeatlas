use super::{
    store, CacheEnvelope, SourceIndex, SOURCE_INDEX_ALGORITHM_VERSION, SOURCE_INDEX_FORMAT_VERSION,
};
use crate::domain::source_graph::SourceGraph;
use std::sync::atomic::Ordering;

impl SourceIndex {
    pub(crate) fn load_graph(&self) -> Option<SourceGraph> {
        let root = self.root.as_ref()?;
        let snapshot = self.snapshot.as_ref()?;
        let path = root.join("graphs").join(format!("{}.json", snapshot.key));
        let envelope = store::read_json::<CacheEnvelope<SourceGraph>>(&path);
        let graph = envelope.and_then(|envelope| {
            (envelope.format_version == SOURCE_INDEX_FORMAT_VERSION
                && envelope.algorithm_version == SOURCE_INDEX_ALGORITHM_VERSION
                && envelope.key == snapshot.key)
                .then_some(envelope.value)
        });
        if graph.is_none() {
            let _ = std::fs::remove_file(&path);
        }
        let mut metrics = self.metrics.lock().expect("source index metrics");
        if graph.is_some() {
            metrics.graph_hits += 1;
        } else {
            metrics.graph_misses += 1;
        }
        graph
    }

    pub(crate) fn store_graph(&self, graph: &SourceGraph) {
        if self.untracked_input.load(Ordering::Relaxed) {
            return;
        }
        let (Some(root), Some(snapshot)) = (&self.root, &self.snapshot) else {
            return;
        };
        let path = root.join("graphs").join(format!("{}.json", snapshot.key));
        let envelope = CacheEnvelope {
            format_version: SOURCE_INDEX_FORMAT_VERSION,
            algorithm_version: SOURCE_INDEX_ALGORITHM_VERSION,
            key: snapshot.key.clone(),
            value: graph,
        };
        self.write(&path, &envelope);
    }
}
