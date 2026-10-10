//! The desktop app of Onyx.
//!
//! The window is in `ui/`, and it holds no logic. This crate turns what the
//! command line prints into the values that the window shows.

/// The code that Slint makes from `ui/`.
mod ui {
    // The generated code unwraps values that Slint itself guarantees, and the
    // lint of this workspace is for code that people write.
    #![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]
    slint::include_modules!();
}

pub use ui::*;

pub mod elevate;
pub mod view;
