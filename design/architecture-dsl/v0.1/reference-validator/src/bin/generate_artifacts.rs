use codeatlas_architecture_dsl_reference_validator::{
    check_generated_artifacts, write_generated_artifacts,
};
use std::path::Path;

fn main() {
    let design_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("design root");
    let result = match std::env::args().nth(1).as_deref() {
        Some("--write") => write_generated_artifacts(design_root),
        Some("--check") => check_generated_artifacts(design_root),
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
