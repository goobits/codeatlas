#[path = "../tooling.rs"]
mod tooling;

#[path = "../internal/mod.rs"]
mod internal;

fn main() {
    tooling::run();
    let _ = internal::exercise();
    let _ = codeatlas_rust_fixture::public_api();
}
