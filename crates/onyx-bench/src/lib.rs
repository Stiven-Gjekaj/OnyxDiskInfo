//! The disk tests of Onyx.
//!
//! Each test takes a [`Device`], and never a path. The command line opens a
//! real drive and gives it here. A test of this crate gives it memory or a
//! temporary file. That is the only reason the tests of this crate can write.

#![forbid(unsafe_code)]

pub mod bench;
pub mod capacity;
mod device;
pub mod scan;
pub mod verify;

pub use device::{AlignedBuffer, Device, FileDevice, MemoryDevice};
