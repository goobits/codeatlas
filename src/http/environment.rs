use std::path::PathBuf;

pub(super) use crate::environment::cache_base;

pub(super) fn configured_python() -> PathBuf {
    std::env::var_os("CODEATLAS_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }))
}
