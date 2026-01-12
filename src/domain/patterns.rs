// Common heuristics/patterns can go here.
// For now, this is a placeholder for shared logic like route matching utilities if needed.

#[derive(Debug, Clone)]
pub struct RouteCandidate {
    pub method: String,
    pub path: String,
    pub handler_name: String,
}
