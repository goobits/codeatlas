use std::path::PathBuf;

pub(crate) fn cache_base() -> PathBuf {
    if let Some(path) = std::env::var_os("CODEATLAS_CACHE_DIR") {
        return PathBuf::from(path);
    }
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home
            .map(|path| path.join("Library").join("Caches"))
            .unwrap_or_else(std::env::temp_dir);
    }
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|path| path.join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
}
