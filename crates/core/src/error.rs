//! Errors returned by the time logic of this crate.

use thiserror::Error;

/// Failure while computing a time value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimeError {
    /// A date-time or duration computation went out of the representable range.
    #[error("time arithmetic overflow")]
    Overflow,
    /// Setting seconds and nanoseconds to zero did not give a valid local time.
    #[error("cannot truncate seconds and nanoseconds to a valid time")]
    InvalidTruncation,
}
