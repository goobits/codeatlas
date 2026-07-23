use codeatlas_architecture_dsl_reference_validator::{
    check_generated_artifacts, write_generated_artifacts,
};
use std::path::Path;

fn main() {
    let specification_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("Code Atlas repository root")
        .join("spec/architecture/v0.1");
    let result = match std::env::args().nth(1).as_deref() {
        Some("--write") => write_generated_artifacts(&specification_root),
        Some("--check") => check_generated_artifacts(&specification_root),
        _ => {
            eprintln!("usage: generate_artifacts --write | --check");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
