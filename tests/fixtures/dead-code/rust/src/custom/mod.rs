mod operation;

pub use operation::*;

pub(crate) fn internal_api() {}

pub fn custom_api() -> &'static str {
    "custom"
}
