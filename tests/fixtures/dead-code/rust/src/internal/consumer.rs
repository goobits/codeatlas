pub(super) fn uses_scoped() -> bool {
    let _ = super::ScopedVisible;
    true
}
