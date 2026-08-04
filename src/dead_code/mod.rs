mod analyze;
mod classification;
mod model;

pub(crate) use analyze::analyze;
#[cfg(test)]
pub(crate) use model::DEAD_CODE_SCHEMA_VERSION;
pub(crate) use model::{DeadCodeFinding, DeadCodeFindingKind, DeadCodeReport};
