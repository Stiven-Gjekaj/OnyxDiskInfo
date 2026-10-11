//! The part of Onyx that no host changes.
//!
//! This crate decodes what a disk reports and scores its health. It makes no
//! system call, so each host compiles it and runs its tests.

#![forbid(unsafe_code)]

pub mod ata;
mod drive;
pub mod emmc;
mod error;
pub mod health;
mod listing;
pub mod measure;
pub mod nvme;
mod record;
mod report;
mod safety;
mod size;
pub mod snapshot;

pub use drive::{Bus, Connection, DriveId, DriveInfo};
pub use error::{Error, Result};
pub use listing::{DriveList, by_index, in_list_order, natural_cmp};
pub use record::{Evidence, Record};
pub use report::Report;
pub use safety::{Force, check_same_drive, check_target, describe, force_phrase, phrase_matches};
pub use size::{grouped, human_size, size_column};
