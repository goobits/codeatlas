#![deny(unreachable_pub)]

pub mod package;
pub mod paths;
pub mod source_discovery;
pub mod source_policy;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

/// Supplies deterministic parsed-file facts without coupling language adapters
/// to the application's cache implementation.
pub trait SourceFactProvider {
    fn parse_file<T, F>(
        &self,
        namespace: &str,
        source_path: &Path,
        project_root: &Path,
        parse: F,
    ) -> Result<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(&str) -> Result<T>;
}

fn is_not_found(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        || error
            .downcast_ref::<ignore::Error>()
            .and_then(ignore::Error::io_error)
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::is_not_found;

    #[test]
    fn recognizes_typed_missing_path_errors() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        assert!(is_not_found(&missing));
        assert!(!is_not_found(&denied));
    }
}
