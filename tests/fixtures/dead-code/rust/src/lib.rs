mod api;
mod custom;

#[path = "renamed.rs"]
mod special;

#[cfg(feature = "extra")]
mod feature;

pub use api::public_api;
pub use custom::*;

pub fn direct_path() -> &'static str {
    let _ = helper_crate::helper();
    crate::special::renamed()
}
