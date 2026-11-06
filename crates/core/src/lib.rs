//! Pure domain logic and API types shared by the Schisaciaccole app and server.
//!
//! Every function takes the current time as a parameter, so nothing here reads the clock.

pub mod api;
pub mod attendee;
pub mod countdown;
pub mod error;
pub mod plan;
pub mod quarters;
pub mod slots;

pub use api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    HEALTH_PATH, ME_PATH, MEETING_PLAN_ROUTE, MEETINGS_PATH, TEMPLATE_ROUTE, TEMPLATES_PATH,
    meeting_plan_path, template_path,
};
pub use api::{
    ApiError, Attendee, EntryKind, Me, Meeting, PlanEntry, PlanSource, PutTemplateRequest,
    RefreshRequest, ResponseStatus, SlotPlan, Template, TemplateEntry, TokenRequest, TokenResponse,
    error_codes,
};
pub use attendee::display_name_from_email;
pub use countdown::{FINISH_GRACE_MS, TickOutcome, evaluate, resolve_target};
pub use error::TimeError;
pub use plan::{
    EditorEntry, FREE_TIME_LABEL, build_plan, editor_to_template, normalize_title, plan_to_editor,
    template_to_plan, unassigned_minutes,
};
pub use quarters::{
    DEFAULT_OPTION_COUNT, LEAD_SECONDS, QUARTER_MINUTES, QuarterOption, quarter_options,
};
pub use slots::{MIN_SLOT_MS, Participant, SlotSchedule, SlotView, allocate_ms, participants};
