//! The macOS device layer, which reads the IOKit registry.

#[cfg(target_os = "macos")]
pub mod host;
pub mod model;
#[cfg(target_os = "macos")]
pub mod open;
pub mod parse;
#[cfg(target_os = "macos")]
pub mod smart;
#[cfg(target_os = "macos")]
pub mod trim;
