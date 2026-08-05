mod callable_contract;
mod code_fuzz;
mod dead_code;
mod docs;
mod interop;
mod public_api;
mod testing;
mod unused_public;

pub(crate) fn agentspeak_contracts_root() -> std::path::PathBuf {
    std::env::var_os("AGENTSPEAK_CONTRACTS_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("CodeAtlas repository should have a parent directory")
                .join("agentspeak-contracts")
        })
}
