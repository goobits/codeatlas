fn nested_helper() -> &'static str {
    "nested"
}

pub fn nested_public() -> &'static str {
    nested_helper()
}
