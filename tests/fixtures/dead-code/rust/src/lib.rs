mod api;
mod custom;
pub mod exposed;
mod internal;

#[path = "renamed.rs"]
mod special;

#[cfg(feature = "extra")]
mod feature;

pub use api::public_api;
pub use custom::*;

pub fn direct_path() -> &'static str {
    let _ = helper_crate::helper();
    let _ = crate::internal::GlobVisible;
    let _ = crate::internal::ParentVisible;
    let _ = crate::internal::collision();
    let _ = crate::internal::exercise();
    crate::special::renamed()
}
