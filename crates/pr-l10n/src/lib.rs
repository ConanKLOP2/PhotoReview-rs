//! Port target: `PhotoReview.Core/Localization` + `PhotoReview.Localization.Generator` (key codegen moves to build.rs).

pub mod template;
pub mod catalog;
pub mod loader;
pub mod localizer;

pub mod keys {
    include!(concat!(env!("OUT_DIR"), "/tr_keys.rs"));
}

pub mod tr {
    #![allow(non_snake_case, clippy::too_many_arguments)] // generated accessors: placeholders like {X}
    include!(concat!(env!("OUT_DIR"), "/tr_fns.rs"));
}
