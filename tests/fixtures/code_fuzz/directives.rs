pub trait ArtifactPublisher {
    /// @codeatlas-fuzz deny: publishes to the real artifact registry
    fn publish(&self, bundle: String) -> bool;

    /// @codeatlas-fuzz allow: stale comments may not grant authority
    fn stale_allow(&self, bundle: String) -> bool;
}
