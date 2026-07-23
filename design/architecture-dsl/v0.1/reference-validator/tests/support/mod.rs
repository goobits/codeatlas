#![allow(dead_code)]

use codeatlas_architecture_dsl_reference_validator::{
    parse_restricted_yaml, ParseLimits, Vocabulary,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn design_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("design root")
        .to_path_buf()
}

pub fn specification_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("Code Atlas repository root")
        .join("spec/architecture/v0.1")
}

pub fn read_design_yaml(relative_path: &str) -> Value {
    read_yaml(&design_root().join(relative_path))
}

pub fn read_specification_yaml(relative_path: &str) -> Value {
    read_yaml(&specification_root().join(relative_path))
}

pub fn vocabulary() -> Vocabulary {
    Vocabulary::from_document(&read_specification_yaml(
        "vocabularies/core.v0.1.atlas.yaml",
    ))
    .expect("core vocabulary")
}

fn read_yaml(path: &Path) -> Value {
    let bytes = fs::read(path).expect("read YAML");
    parse_restricted_yaml(&bytes, ParseLimits::default())
        .expect("parse YAML")
        .value
}
