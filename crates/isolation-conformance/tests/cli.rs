use std::process::Command;

#[test]
fn private_probe_rejects_unknown_modes_and_missing_evidence() {
    let executable = env!("CARGO_BIN_EXE_isolation-conformance");
    let unknown = Command::new(executable)
        .arg("invented")
        .env_clear()
        .output()
        .expect("unknown-mode probe");
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("Unknown"));

    let missing = Command::new(executable)
        .arg("verify")
        .env_clear()
        .output()
        .expect("missing-evidence probe");
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Missing required environment"));
    assert!(missing.stdout.is_empty());
}
