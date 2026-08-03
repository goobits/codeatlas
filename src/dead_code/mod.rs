mod analyze;
mod classification;
mod model;

pub(crate) use analyze::analyze;
pub(crate) use model::{DeadCodeFinding, DeadCodeFindingKind, DeadCodeReport};
