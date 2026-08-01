//! Package dependency, manifest, workspace, and entrypoint discovery.

mod dependency;
mod entrypoints;
mod manifest;
mod source_layout;
mod workspace;

pub(crate) use dependency::{
    is_local as is_local_dependency, resolve as resolve_dependency,
    split_specifier as split_package_specifier,
};
pub(crate) use entrypoints::discover_bundled_entrypoints;
pub(crate) use entrypoints::discover_entrypoints as discover_runtime_entrypoints;
pub(crate) use entrypoints::discover_tooling_entrypoints;
pub(crate) use manifest::{discover, discover_for_docs};
pub(crate) use workspace::discover as discover_workspace;
