use crate::{
    custom::{self, FacadeType},
};

fn used_private() -> &'static str {
    "used"
}

fn unused_private() -> &'static str {
    "unused"
}

pub fn public_api() -> &'static str {
    custom::internal_api();
    used_private()
}

pub fn facade_type() -> FacadeType {
    FacadeType
}
