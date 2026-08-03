use super::{is_opaque_vendor_source, OPAQUE_VENDOR_BYTES};
use std::fs;

#[test]
fn large_vendor_bundles_are_opaque_even_when_pretty_printed() {
    let root = std::env::temp_dir().join(format!("codeatlas-large-vendor-{}", std::process::id()));
    let path = root.join("src/vendor/bundle.js");
    fs::create_dir_all(path.parent().expect("vendor parent")).expect("vendor directory");
    fs::write(&path, vec![b'\n'; OPAQUE_VENDOR_BYTES as usize]).expect("large vendor bundle");

    assert!(is_opaque_vendor_source(&path, "src/vendor/bundle.js"));

    fs::remove_dir_all(root).expect("temporary vendor cleanup");
}
