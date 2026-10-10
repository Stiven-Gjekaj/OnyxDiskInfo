//! The part of Onyx that no host changes.
//!
//! This crate decodes what a disk reports and scores its health. It makes no
//! system call, so each host compiles it and runs its tests.

#![forbid(unsafe_code)]

mod drive;
mod error;
mod listing;
pub mod nvme;
mod size;

pub use drive::{Bus, Connection, DriveId, DriveInfo};
pub use error::{Error, Result};
pub use listing::{DriveList, by_index, in_list_order, natural_cmp};
pub use size::{grouped, human_size, size_column};
