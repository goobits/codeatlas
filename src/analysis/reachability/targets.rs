use super::{adjacencies, public_surface_roots, Reachability};
use codeatlas_domain::source_graph::{ContextScope, GraphDiagnostic, NodeId, SourceGraph};
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet, VecDeque};

impl Reachability {
    pub(crate) fn analyze_targets(
        graph: &SourceGraph,
        targets: &BTreeSet<NodeId>,
    ) -> Result<Self, Vec<GraphDiagnostic>> {
        graph.validate()?;
        if targets.is_empty() {
            return Ok(Self::default());
        }
        if targets.len() > graph.contexts.len().max(1) {
            return Ok(Self::analyze_validated(graph));
        }

        let (runtime_adjacency, public_surface_adjacency) = adjacencies(graph);
        let reverse_adjacency = reverse_adjacency(&runtime_adjacency);
        let mut result = Self::default();
        for target in targets {
            if !graph.nodes.contains_key(target) {
                continue;
            }
            let distances = reverse_distances(target, &reverse_adjacency);
            for context in graph.contexts.values() {
                let roots = context.roots.iter().collect::<Vec<_>>();
                let selected = roots
                    .iter()
                    .enumerate()
                    .filter_map(|(root_index, root)| {
                        let distance = match context.scope {
                            ContextScope::Runtime => distances.get(*root).copied(),
                            ContextScope::PublicSurface => {
                                public_surface_roots(root, &public_surface_adjacency)
                                    .iter()
                                    .filter_map(|root| distances.get(root))
                                    .copied()
                                    .min()
                            }
                        }?;
                        Some((distance, root_index))
                    })
                    .min();
                let Some((_, root_index)) = selected else {
                    continue;
                };
                result.record(target, context, roots[root_index]);
            }
        }
        Ok(result)
    }
}

fn reverse_adjacency(
    adjacency: &BTreeMap<NodeId, BTreeSet<NodeId>>,
) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
    let mut reverse = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for (source, targets) in adjacency {
        for target in targets {
            reverse
                .entry(target.clone())
                .or_default()
                .insert(source.clone());
        }
    }
    reverse
}

fn reverse_distances(
    target: &NodeId,
    adjacency: &BTreeMap<NodeId, BTreeSet<NodeId>>,
) -> BTreeMap<NodeId, usize> {
    let mut distances = BTreeMap::new();
    let mut queue = VecDeque::from([(target.clone(), 0)]);
    while let Some((node, distance)) = queue.pop_front() {
        let Entry::Vacant(entry) = distances.entry(node.clone()) else {
            continue;
        };
        entry.insert(distance);
        if let Some(sources) = adjacency.get(&node) {
            queue.extend(sources.iter().cloned().map(|source| (source, distance + 1)));
        }
    }
    distances
}
