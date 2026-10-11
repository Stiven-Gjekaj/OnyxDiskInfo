//! The Windows device layer, which asks SetupAPI and the IO control codes.

#[cfg(windows)]
pub mod host;
#[cfg(windows)]
pub mod open;
pub mod parse;
pub mod passthru;
pub mod raw;
#[cfg(windows)]
pub mod smart;
#[cfg(windows)]
pub mod trim;
pub mod volumes;
