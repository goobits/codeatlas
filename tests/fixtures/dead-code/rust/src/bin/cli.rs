#[path = "../tooling.rs"]
mod tooling;

fn main() {
    tooling::run();
    let _ = codeatlas_rust_fixture::public_api();
}
