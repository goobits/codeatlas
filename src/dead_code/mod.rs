mod analyze;
mod model;

pub(crate) use analyze::analyze;
pub(crate) use model::DeadCodeReport;
#[cfg(test)]
pub(crate) use model::{DeadCodeFinding, DeadCodeFindingKind};
