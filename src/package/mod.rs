//! Package dependency resolution, manifest discovery, and report annotation.

mod annotation;
mod dependency;
mod manifest;
mod runtime;
mod source_layout;

pub(crate) use annotation::{annotate, consolidate_declaration_symbols};
pub(crate) use dependency::{
    is_local as is_local_dependency, resolve as resolve_dependency,
    split_specifier as split_package_specifier,
};
pub(crate) use manifest::{discover, discover_for_docs};
pub(crate) use runtime::discover_entrypoints as discover_runtime_entrypoints;
pub(crate) use runtime::discover_tooling_entrypoints;
