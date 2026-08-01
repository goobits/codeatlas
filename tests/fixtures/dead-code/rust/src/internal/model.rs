pub(crate) struct GlobVisible;

impl GlobVisible {
    pub(crate) fn construct() -> Self {
        constructor_marker();
        Self
    }
}

fn constructor_marker() {}

#[cfg(test)]
mod tests {
    struct TestHelper;

    impl TestHelper {
        fn prepare() {}
    }

    #[test]
    fn helper_is_test_only() {
        TestHelper::prepare();
    }
}

impl Default for GlobVisible {
    fn default() -> Self {
        Self
    }
}
