use clap::Args;

#[derive(Args, Clone, Copy, Debug, Default)]
pub(super) struct RepositoryScopeArgs {
    /// Discover projects from the nearest pnpm workspace
    #[arg(long)]
    pub(super) workspace: bool,
}
