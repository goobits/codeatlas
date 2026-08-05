mod collision;
mod consumer;
mod model;
mod parent;
mod scoped;

pub(crate) use collision::dispatch as collision;
pub(crate) use model::*;
pub(super) use parent::ParentVisible;
pub(in crate::internal) use scoped::ScopedVisible;

pub(crate) fn exercise() -> bool {
    consumer::uses_scoped()
}
