//! The partition tables and the file systems that Onyx writes.
//!
//! The table is computed here as bytes and offsets, so a test checks each
//! byte with no drive. The file systems write through a target that takes
//! whole sectors, so a test gives them a file.

#![forbid(unsafe_code)]

mod crc32;
pub mod fs;
pub mod layout;
pub mod table;
mod target;

pub use target::DriveTarget;
