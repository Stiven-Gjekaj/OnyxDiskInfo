//! The Windows device layer, which asks SetupAPI and the IO control codes.

#[cfg(windows)]
pub mod host;
pub mod parse;
pub mod passthru;
pub mod raw;
#[cfg(windows)]
pub mod smart;
pub mod volumes;
