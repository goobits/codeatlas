use super::model::{self, GlobVisible};

pub(super) fn uses_scoped() -> bool {
    model::module_marker();
    let _ = GlobVisible::construct();
    let _ = GlobVisible;
    let _ = super::ScopedVisible;
    true
}
