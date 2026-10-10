//! The part of Onyx that no host changes.
//!
//! This crate decodes what a disk reports and scores its health. It makes no
//! system call, so each host compiles it and runs its tests.

#![forbid(unsafe_code)]

mod drive;
mod error;

pub use drive::{Bus, Connection, DriveId, DriveInfo};
pub use error::{Error, Result};
