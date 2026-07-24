#[cfg(target_os = "linux")]
mod platform;

fn main() {
    custom_codegen!();
}
