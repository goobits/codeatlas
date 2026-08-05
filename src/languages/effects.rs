//! Shared pure matching primitives for adapter-owned effect evidence.

pub(super) fn has_qualified_action(
    path: &str,
    separator: &str,
    namespaces: &[&str],
    actions: &[&str],
) -> bool {
    let Some((namespace, action)) = path.rsplit_once(separator) else {
        return false;
    };
    actions.contains(&action)
        && namespaces.iter().any(|candidate| {
            namespace == *candidate
                || namespace
                    .strip_prefix(candidate)
                    .is_some_and(|suffix| suffix.starts_with(separator))
        })
}

#[cfg(test)]
mod tests {
    use super::has_qualified_action;

    #[test]
    fn qualified_actions_require_namespace_boundaries_and_exact_actions() {
        for (path, separator, expected) in [
            ("fs.readFile", ".", true),
            ("fs.promises.readFile", ".", true),
            ("filesystem.readFile", ".", false),
            ("fs.readFileSync", ".", false),
            ("std::fs::read", "::", true),
            ("std::filesystem::read", "::", false),
        ] {
            assert_eq!(
                has_qualified_action(path, separator, &["fs", "std::fs"], &["readFile", "read"]),
                expected,
                "{path}"
            );
        }
    }
}
