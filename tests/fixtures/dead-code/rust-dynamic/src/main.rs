#[cfg(target_os = "linux")]
mod platform;

#[cfg(test)]
fn runtime_mode() -> &'static str {
    "test"
}

#[cfg(not(test))]
fn runtime_mode() -> &'static str {
    "production"
}

fn main() {
    let _ = runtime_mode();
    custom_codegen!();
}
