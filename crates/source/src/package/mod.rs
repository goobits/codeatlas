//! Package dependency, manifest, workspace, and entrypoint discovery.

mod dependency;
mod entrypoints;
mod manifest;
mod python_manifest;
mod source_layout;
mod workspace;

pub use dependency::{
    declares_any as declares_any_dependency, is_local as is_local_dependency,
    resolve as resolve_dependency, split_specifier as split_package_specifier, ResolvedDependency,
};
pub use entrypoints::discover_bundled_entrypoints;
pub use entrypoints::discover_entrypoints as discover_runtime_entrypoints;
pub use entrypoints::discover_tooling_entrypoints;
pub use entrypoints::read_scripts;
pub use manifest::{discover, discover_for_docs, discover_javascript};
pub use python_manifest::discover as discover_python;
pub use python_manifest::discover_entrypoints as discover_python_entrypoints;
pub use python_manifest::source_roots as discover_python_source_roots;
pub use workspace::discover as discover_workspace;
pub use workspace::{
    nearest_root as nearest_workspace_root, owns_descendants as workspace_owns_descendants,
    PackageWorkspace, PackageWorkspaceMember,
};
