mod analyze;
mod classification;
mod model;

#[cfg(test)]
pub(crate) use analyze::{analyze, analyze_check};
pub(crate) use analyze::{analyze_check_with_reachability, analyze_with_reachability};
#[cfg(test)]
pub(crate) use model::DEAD_CODE_SCHEMA_VERSION;
pub(crate) use model::{DeadCodeFinding, DeadCodeFindingKind, DeadCodeReport};
