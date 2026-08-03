pub(crate) fn is_not_found(error: &(dyn std::error::Error + 'static)) -> bool {
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
