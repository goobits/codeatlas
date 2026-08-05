use super::{InspectionDirection, InspectionRequest};
use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const CURSOR_VERSION: u32 = 1;
const MAX_CURSOR_BYTES: usize = 4_096;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4_096;

pub(crate) struct ProjectionRequest<'a> {
    pub(crate) targets: &'a [String],
    pub(crate) depth: usize,
    pub(crate) max_nodes: usize,
    pub(crate) direction: InspectionDirection,
    pub(crate) continuation: Option<&'a str>,
    pub(crate) subject: &'static str,
    pub(crate) graph_subject: &'static str,
}

impl<'a> ProjectionRequest<'a> {
    pub(crate) fn from_request(
        request: &'a InspectionRequest,
        subject: &'static str,
        graph_subject: &'static str,
    ) -> Self {
        Self {
            targets: &request.targets,
            depth: request.depth,
            max_nodes: request.max_nodes,
            direction: request.direction,
            continuation: request.continuation.as_deref(),
            subject,
            graph_subject,
        }
    }
}

pub(crate) struct ProjectionPage<N> {
    pub(crate) graph_digest: String,
    pub(crate) ordered_nodes: Vec<N>,
    pub(crate) included_nodes: BTreeSet<N>,
    pub(crate) page_nodes: BTreeSet<N>,
    pub(crate) page_offset: usize,
    pub(crate) page_end: usize,
    pub(crate) continuation: Option<String>,
}

pub(crate) struct ProjectedGraph<N, V, E> {
    pub(crate) page: ProjectionPage<N>,
    pub(crate) nodes: BTreeMap<N, V>,
    pub(crate) edges: Vec<E>,
    pub(crate) omitted_nodes: usize,
    pub(crate) omitted_edges: usize,
}

impl<N: Ord> ProjectionPage<N> {
    pub(crate) fn owns_node(&self, node: &N) -> bool {
        self.page_nodes.contains(node)
    }

    pub(crate) fn owns_any<'a>(&self, nodes: impl IntoIterator<Item = &'a N>) -> bool
    where
        N: 'a,
    {
        let candidates = nodes
            .into_iter()
            .filter(|node| self.included_nodes.contains(*node))
            .collect::<BTreeSet<_>>();
        self.ordered_nodes
            .iter()
            .find(|node| candidates.contains(node))
            .is_some_and(|node| self.owns_node(node))
    }
}

#[derive(Serialize, Deserialize)]
struct ProjectionCursor {
    version: u32,
    graph_digest: String,
    request_digest: String,
    offset: usize,
}

pub(crate) fn validate_request(request: &ProjectionRequest<'_>) -> Result<()> {
    if request.targets.is_empty() {
        anyhow::bail!("at least one {} target is required", request.subject);
    }
    if request
        .targets
        .iter()
        .any(|target| target.trim().is_empty())
    {
        anyhow::bail!("{} targets cannot be empty", request.subject);
    }
    if request.depth > MAX_DEPTH {
        anyhow::bail!("{} depth cannot exceed {MAX_DEPTH}", request.subject);
    }
    if request.max_nodes == 0 || request.max_nodes > MAX_NODES {
        anyhow::bail!("max-nodes must be between 1 and {MAX_NODES}");
    }
    Ok(())
}

/// Preserve the pre-extraction context-slice v5 digest bytes. New artifacts use
/// the execution artifact owner's domain-separated RFC 8785 digest instead.
pub(crate) fn digest_legacy_graph(value: &impl Serialize, subject: &str) -> Result<String> {
    let bytes = serde_json::to_vec(value).with_context(|| format!("serialize {subject}"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub(crate) fn create_page<N>(
    graph_digest: String,
    request: &ProjectionRequest<'_>,
    roots: &BTreeSet<N>,
    edges: impl IntoIterator<Item = (N, N)>,
) -> Result<ProjectionPage<N>>
where
    N: Clone + Ord,
{
    validate_request(request)?;
    let request_digest = request_digest(request)?;
    let ordered_nodes = expand(roots, edges, request.depth, request.direction);
    let included_nodes = ordered_nodes.iter().cloned().collect::<BTreeSet<_>>();
    let page_offset = match request.continuation {
        Some(cursor) => decode_cursor(cursor, &graph_digest, &request_digest, request)?,
        None => 0,
    };
    if page_offset >= ordered_nodes.len() && !ordered_nodes.is_empty() {
        anyhow::bail!(
            "{} continuation cursor points beyond the available graph slice",
            request.subject
        );
    }
    let page_end = page_offset
        .saturating_add(request.max_nodes)
        .min(ordered_nodes.len());
    let page_nodes = ordered_nodes[page_offset..page_end]
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let continuation = (page_end < ordered_nodes.len())
        .then(|| encode_cursor(&graph_digest, &request_digest, page_end))
        .transpose()?;
    Ok(ProjectionPage {
        graph_digest,
        ordered_nodes,
        included_nodes,
        page_nodes,
        page_offset,
        page_end,
        continuation,
    })
}

pub(crate) fn project_graph<N, V, E>(
    graph_digest: String,
    request: &ProjectionRequest<'_>,
    roots: &BTreeSet<N>,
    nodes: &BTreeMap<N, V>,
    edges: &BTreeSet<E>,
    endpoints: impl Fn(&E) -> (&N, &N),
) -> Result<ProjectedGraph<N, V, E>>
where
    N: Clone + Ord,
    V: Clone,
    E: Clone + Ord,
{
    if roots.iter().any(|root| !nodes.contains_key(root)) {
        anyhow::bail!(
            "{} target resolved to an absent graph node",
            request.subject
        );
    }
    let page = create_page(
        graph_digest,
        request,
        roots,
        edges.iter().map(|edge| {
            let (from, to) = endpoints(edge);
            (from.clone(), to.clone())
        }),
    )?;
    let projected_nodes = page
        .page_nodes
        .iter()
        .filter_map(|id| nodes.get(id).cloned().map(|node| (id.clone(), node)))
        .collect::<BTreeMap<_, _>>();
    let all_edges = edges
        .iter()
        .filter(|edge| {
            let (from, to) = endpoints(edge);
            from != to && page.included_nodes.contains(from) && page.included_nodes.contains(to)
        })
        .collect::<Vec<_>>();
    let projected_edges = all_edges
        .iter()
        .filter(|edge| page.owns_node(endpoints(edge).0))
        .map(|edge| (*edge).clone())
        .collect::<Vec<_>>();
    Ok(ProjectedGraph {
        omitted_nodes: page
            .ordered_nodes
            .len()
            .saturating_sub(projected_nodes.len()),
        omitted_edges: all_edges.len().saturating_sub(projected_edges.len()),
        page,
        nodes: projected_nodes,
        edges: projected_edges,
    })
}

fn expand<N>(
    roots: &BTreeSet<N>,
    edges: impl IntoIterator<Item = (N, N)>,
    depth: usize,
    direction: InspectionDirection,
) -> Vec<N>
where
    N: Clone + Ord,
{
    let mut outgoing = BTreeMap::<N, BTreeSet<N>>::new();
    let mut incoming = BTreeMap::<N, BTreeSet<N>>::new();
    for (from, to) in edges {
        if from == to {
            continue;
        }
        if matches!(
            direction,
            InspectionDirection::Outgoing | InspectionDirection::Both
        ) {
            outgoing.entry(from.clone()).or_default().insert(to.clone());
        }
        if matches!(
            direction,
            InspectionDirection::Incoming | InspectionDirection::Both
        ) {
            incoming.entry(to).or_default().insert(from);
        }
    }

    let mut included = roots.clone();
    let mut ordered = roots.iter().cloned().collect::<Vec<_>>();
    let mut frontier = roots.iter().cloned().collect::<VecDeque<_>>();
    for _ in 0..depth {
        let mut candidates = BTreeSet::new();
        while let Some(current) = frontier.pop_front() {
            if matches!(
                direction,
                InspectionDirection::Outgoing | InspectionDirection::Both
            ) {
                candidates.extend(outgoing.get(&current).into_iter().flatten().cloned());
            }
            if matches!(
                direction,
                InspectionDirection::Incoming | InspectionDirection::Both
            ) {
                candidates.extend(incoming.get(&current).into_iter().flatten().cloned());
            }
        }
        candidates.retain(|candidate| !included.contains(candidate));
        if candidates.is_empty() {
            break;
        }
        included.extend(candidates.iter().cloned());
        ordered.extend(candidates.iter().cloned());
        frontier.extend(candidates);
    }
    ordered
}

fn request_digest(request: &ProjectionRequest<'_>) -> Result<String> {
    let targets = request
        .targets
        .iter()
        .map(|target| target.trim())
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(targets, request.depth, request.max_nodes, request.direction))
        .with_context(|| {
            format!(
                "serialize {} request for continuation cursor",
                request.subject
            )
        })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn encode_cursor(graph_digest: &str, request_digest: &str, offset: usize) -> Result<String> {
    let payload = ProjectionCursor {
        version: CURSOR_VERSION,
        graph_digest: graph_digest.to_owned(),
        request_digest: request_digest.to_owned(),
        offset,
    };
    Ok(URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&payload).context("serialize inspection continuation cursor")?))
}

fn decode_cursor(
    cursor: &str,
    graph_digest: &str,
    request_digest: &str,
    request: &ProjectionRequest<'_>,
) -> Result<usize> {
    if cursor.len() > MAX_CURSOR_BYTES {
        anyhow::bail!(
            "{} continuation cursor exceeds {MAX_CURSOR_BYTES} bytes",
            request.subject
        );
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .with_context(|| format!("decode {} continuation cursor", request.subject))?;
    let payload: ProjectionCursor = serde_json::from_slice(&decoded)
        .with_context(|| format!("parse {} continuation cursor", request.subject))?;
    if payload.version != CURSOR_VERSION {
        anyhow::bail!(
            "unsupported {} continuation cursor version {}",
            request.subject,
            payload.version
        );
    }
    if payload.graph_digest != graph_digest {
        anyhow::bail!(
            "{} continuation cursor is stale because the {} changed",
            request.subject,
            request.graph_subject
        );
    }
    if payload.request_digest != request_digest {
        anyhow::bail!(
            "{} continuation cursor does not match the current request",
            request.subject
        );
    }
    Ok(payload.offset)
}

#[cfg(test)]
mod tests {
    use super::{create_page, ProjectionRequest};
    use crate::inspection::InspectionDirection;
    use std::collections::BTreeSet;

    #[test]
    fn projection_orders_deduplicates_and_pages_one_shared_graph() {
        let targets = vec!["b".to_string()];
        let request = ProjectionRequest {
            targets: &targets,
            depth: 1,
            max_nodes: 2,
            direction: InspectionDirection::Both,
            continuation: None,
            subject: "fixture",
            graph_subject: "fixture graph",
        };
        let page = create_page(
            "sha256:fixture".to_string(),
            &request,
            &BTreeSet::from(["b".to_string()]),
            [
                ("a".to_string(), "b".to_string()),
                ("b".to_string(), "c".to_string()),
                ("b".to_string(), "b".to_string()),
            ],
        )
        .expect("projection page");
        assert_eq!(page.ordered_nodes, ["b", "a", "c"]);
        assert_eq!(
            page.page_nodes,
            BTreeSet::from(["a".to_string(), "b".to_string()])
        );
        assert!(page.continuation.is_some());
    }
}
