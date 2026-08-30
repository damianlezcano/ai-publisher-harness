//! Generic subprocess supervision (stdout/stderr line capture, stop, reap).

#![forbid(unsafe_code)]

pub mod error;
pub mod supervisor;

pub use error::{ProcessError, ProcessResult};
pub use supervisor::ChildGuard;
