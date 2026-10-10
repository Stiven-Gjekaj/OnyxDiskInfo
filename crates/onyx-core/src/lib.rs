//! The part of Onyx that no host changes.
//!
//! This crate decodes what a disk reports and scores its health. It makes no
//! system call, so each host compiles it and runs its tests.

#![forbid(unsafe_code)]

mod error;

pub use error::{Error, Result};
