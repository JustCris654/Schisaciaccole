//! Pure domain logic shared by the Schisaciaccole app and server.
//!
//! Every function takes the current time as a parameter, so nothing here reads the clock.

pub mod countdown;
pub mod error;
pub mod quarters;

pub use countdown::{FINISH_GRACE_MS, TickOutcome, evaluate, resolve_target};
pub use error::TimeError;
pub use quarters::{
    DEFAULT_OPTION_COUNT, LEAD_SECONDS, QUARTER_MINUTES, QuarterOption, quarter_options,
};
