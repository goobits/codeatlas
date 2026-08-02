//! Package dependency, manifest, workspace, and entrypoint discovery.

mod dependency;
mod entrypoints;
mod manifest;
mod python_manifest;
mod source_layout;
mod workspace;

pub(crate) use dependency::{
    declares_any as declares_any_dependency, is_local as is_local_dependency,
    resolve as resolve_dependency, split_specifier as split_package_specifier,
};
pub(crate) use entrypoints::discover_bundled_entrypoints;
pub(crate) use entrypoints::discover_entrypoints as discover_runtime_entrypoints;
pub(crate) use entrypoints::discover_tooling_entrypoints;
pub(crate) use manifest::{discover, discover_for_docs, discover_javascript};
pub(crate) use python_manifest::discover as discover_python;
pub(crate) use python_manifest::source_roots as discover_python_source_roots;
pub(crate) use workspace::discover as discover_workspace;
pub(crate) use workspace::{
    nearest_root as nearest_workspace_root, owns_descendants as workspace_owns_descendants,
};
