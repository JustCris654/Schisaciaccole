//! Pure domain logic and API types shared by the Schisaciaccole app and server.
//!
//! Every function takes the current time as a parameter, so nothing here reads the clock.

pub mod api;
pub mod attendee;
pub mod countdown;
pub mod daily;
pub mod error;
pub mod plan;
pub mod quarters;
pub mod slots;
pub mod turn;

pub use api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    HEALTH_PATH, ME_PATH, MEETINGS_PATH, PREFERENCE_ROUTE, PREFERENCES_PATH, preference_path,
};
pub use api::{
    ApiError, Attendee, Me, Meeting, MeetingPreference, ParticipantResolution, PersonKind,
    PreferencePerson, PutPreferenceRequest, RefreshRequest, ResponseStatus, TokenRequest,
    TokenResponse, error_codes,
};
pub use attendee::{
    GroupExpansion, GroupExpansionOutcome, GroupMember, ResolvedAttendees, abbreviate_name,
    display_name_from_email, merge_group_attendees,
};
pub use countdown::{FINISH_GRACE_MS, TickOutcome, evaluate, resolve_target};
pub use daily::{DailyRound, MIN_TURN_MS, looks_daily, shuffled};
pub use error::TimeError;
pub use plan::{
    EditorEntry, editor_rows, merge_rows, normalize_title, preference_people, start_names,
};
pub use quarters::{
    DEFAULT_OPTION_COUNT, LEAD_SECONDS, QUARTER_MINUTES, QuarterOption, quarter_options,
};
pub use slots::{Participant, participants};
pub use turn::{SpeakingTurn, max_turn_minutes};
