use super::model::GlobVisible;

pub(super) fn uses_scoped() -> bool {
    let _ = GlobVisible::construct();
    let _ = GlobVisible;
    let _ = super::ScopedVisible;
    true
}
