fn used_private() -> &'static str {
    "used"
}

fn unused_private() -> &'static str {
    "unused"
}

pub fn public_api() -> &'static str {
    used_private()
}
