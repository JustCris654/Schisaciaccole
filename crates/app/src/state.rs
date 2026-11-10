//! Application state: the single source of truth that the Slint UI renders.

use std::fmt::Display;
use std::rc::Rc;

use chrono::{DateTime, FixedOffset, Local, TimeZone};
use log::{debug, error, info, warn};
use schisaciaccole_core::api::{Me, Meeting};
use schisaciaccole_core::{
    DEFAULT_OPTION_COUNT, QuarterOption, TickOutcome, TimeError, evaluate, quarter_options,
    resolve_target,
};
use slint::{ModelRc, VecModel};

use crate::ui::{MainWindow, MeetingRow, Page, StopType, TimeOption};

/// Login page status while the browser login is in progress.
pub const LOGIN_WAITING_STATUS: &str = "Completa l'accesso nel browser…";
/// Selection page notice when the backend cannot be reached at startup.
pub const OFFLINE_NOTICE: &str = "Server non raggiungibile: modalità locale";
/// Selection page notice when the backend client cannot be created.
pub const LOCAL_MODE_NOTICE: &str = "Accesso non disponibile: modalità locale";
/// Selection page notice while the logout runs.
pub const LOGOUT_NOTICE: &str = "Disconnessione…";

/// Sound the caller must play after a state update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickSound {
    /// No sound.
    None,
    /// The meeting or countdown finished: the finish sound.
    MeetingEnd,
}

/// What the meetings page shows besides the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeetingsStatus {
    /// The meetings are being fetched.
    Loading,
    /// The rows are up to date; an empty list means no meeting left today.
    Ready,
    /// The last fetch failed; the message explains why.
    Failed(&'static str),
}

/// Everything the main window shows, owned by Rust and pushed to Slint by [`AppState::render`].
pub struct AppState {
    page: Page,
    options: Vec<DateTime<Local>>,
    target: Option<DateTime<Local>>,
    timer_ms: i64,
    options_model: Rc<VecModel<TimeOption>>,
    /// Label of the signed-in account; `None` when not signed in.
    account: Option<String>,
    /// Status line of the selection page; empty when there is none.
    notice: &'static str,
    /// Status line of the login page; empty when there is none.
    login_status: &'static str,
    login_waiting: bool,
    /// Meetings of the last successful fetch, as received.
    meetings: Vec<Meeting>,
    /// Meetings shown on the meetings page, in row order: what a click on row `i` selects.
    shown: Vec<Meeting>,
    meetings_model: Rc<VecModel<MeetingRow>>,
    /// True once a fetch succeeded for the current account.
    meetings_loaded: bool,
    meetings_status: MeetingsStatus,
    /// Meeting selected from the list while its countdown runs.
    run: Option<Meeting>,
}

impl AppState {
    /// Creates the state on the selection page, with options computed from the current time.
    ///
    /// # Errors
    ///
    /// Returns the [`TimeError`] of [`quarter_options`] when the options cannot
    /// be computed (time arithmetic out of range).
    pub fn new() -> Result<Self, TimeError> {
        let (options, rows) = split_options(quarter_options(&Local::now(), DEFAULT_OPTION_COUNT)?);

        Ok(Self {
            page: Page::SelectionPage,
            options,
            target: None,
            timer_ms: 0,
            options_model: Rc::new(VecModel::from(rows)),
            account: None,
            notice: "",
            login_status: "",
            login_waiting: false,
            meetings: Vec::new(),
            shown: Vec::new(),
            meetings_model: Rc::new(VecModel::from(Vec::new())),
            meetings_loaded: false,
            meetings_status: MeetingsStatus::Loading,
            run: None,
        })
    }

    /// Recomputes the selectable end times from the current time.
    ///
    /// On error the error is logged and the previous options are kept. The meeting rows are
    /// rebuilt too, so meetings that have just ended disappear.
    pub fn refresh_options(&mut self) {
        self.rebuild_meeting_rows();
        let now = Local::now();
        debug!("refresh_options at {}", now.format("%H:%M:%S"));

        match quarter_options(&now, DEFAULT_OPTION_COUNT) {
            Ok(options) => {
                let (options, rows) = split_options(options);
                debug!("refresh_options refreshed {} options", rows.len());
                self.options = options;
                self.options_model.set_vec(rows);
            }
            Err(err) => error!("refresh_options failed, keeping previous options: {err}"),
        }
    }

    /// Starts the countdown to the option at `index` and moves to the timer page.
    ///
    /// The target is chosen by [`resolve_target`], so an option already in the
    /// past moves to the next one. Any running meeting is forgotten.
    /// Returns the sound to play, which is [`TickSound::MeetingEnd`] only if the target is
    /// reached at once (see [`AppState::tick`]).
    ///
    /// Edge cases: a negative `index` or one out of range is logged and
    /// ignored; the state does not change and [`TickSound::None`] is returned.
    #[must_use]
    pub fn select(&mut self, index: i32) -> TickSound {
        let Ok(position) = usize::try_from(index) else {
            warn!("select: ignoring negative option index {index}");
            return TickSound::None;
        };

        let now = Local::now();
        let Some(target) = resolve_target(&self.options, position, &now) else {
            warn!("select: ignoring out of range option index {index}");
            return TickSound::None;
        };

        info!(
            "select: target={} remaining={}s",
            target.format("%H:%M"),
            target.signed_duration_since(now).num_seconds()
        );

        self.run = None;
        self.page = Page::TimerPage;
        self.target = Some(target);
        self.tick()
    }

    /// Starts the countdown to the end of the meeting in row `index` and moves to the timer page.
    ///
    /// The countdown is plain: it shows the meeting title and end time and has no speaking
    /// slots. Returns the sound of the first update (see [`AppState::tick`]), which is
    /// [`TickSound::MeetingEnd`] only if the end is reached at once.
    ///
    /// Edge cases: a negative `index` or one out of range is logged and ignored; the state does
    /// not change and [`TickSound::None`] is returned.
    #[must_use]
    pub fn select_meeting(&mut self, index: i32) -> TickSound {
        let Ok(position) = usize::try_from(index) else {
            warn!("select_meeting: ignoring negative meeting index {index}");
            return TickSound::None;
        };
        let Some(meeting) = self.shown.get(position) else {
            warn!("select_meeting: ignoring out of range meeting index {index}");
            return TickSound::None;
        };

        info!(
            "select_meeting: target={}",
            meeting.end.with_timezone(&Local).format("%H:%M")
        );
        let target = meeting.end.with_timezone(&Local);
        self.run = Some(meeting.clone());
        self.page = Page::TimerPage;
        self.target = Some(target);
        self.tick()
    }

    /// Updates the countdown against the current time.
    ///
    /// While the target is ahead, stores the remaining milliseconds. When the target has just
    /// been reached, stops with [`StopType::TimerFinished`] and returns [`TickSound::MeetingEnd`].
    /// When it passed more than the grace period ago (e.g. after a suspend), stops with
    /// [`StopType::UserStopped`] without sound.
    ///
    /// Edge cases: without a running countdown nothing changes and [`TickSound::None`] is
    /// returned.
    #[must_use]
    pub fn tick(&mut self) -> TickSound {
        let Some(target) = self.target else {
            return TickSound::None;
        };
        let now = Local::now();

        match evaluate(&target, &now) {
            TickOutcome::Running { remaining_ms } => {
                self.timer_ms = remaining_ms;
                debug!("Remaining: {remaining_ms}");
                TickSound::None
            }
            TickOutcome::Finished => {
                self.stop(StopType::TimerFinished);
                TickSound::MeetingEnd
            }
            TickOutcome::Expired => {
                self.stop(StopType::UserStopped);
                TickSound::None
            }
        }
    }

    /// Ends the countdown: refreshes the options, clears the target and the timer.
    ///
    /// Moves to the finished page for [`StopType::TimerFinished`] and to the home page (see
    /// [`AppState::show_home`]) for [`StopType::UserStopped`]. Calling it without a
    /// running countdown still refreshes the options and changes the page.
    pub fn stop(&mut self, stop_type: StopType) {
        info!("stop: timer stopped, back to selection");

        self.refresh_options();
        self.target = None;
        self.timer_ms = 0;
        self.run = None;
        self.page = match stop_type {
            StopType::TimerFinished => Page::TimerFinished,
            StopType::UserStopped => self.home_page(),
        };
    }

    /// Moves to the selection page without touching the countdown or the options.
    pub fn show_selection_page(&mut self) {
        self.page = Page::SelectionPage;
    }

    /// Moves to the meetings page, keeping the meetings already loaded. Without an account it moves to
    /// the selection page instead.
    pub fn show_meetings_page(&mut self) {
        self.page = self.home_page();
    }

    /// Leaves the finished page: the meetings page when signed in, otherwise the selection page.
    pub fn show_home(&mut self) {
        self.page = self.home_page();
    }

    /// Replaces the meetings with those of a successful fetch and shows them.
    pub fn on_meetings_loaded(&mut self, meetings: Vec<Meeting>) {
        self.meetings = meetings;
        self.meetings_loaded = true;
        self.meetings_status = MeetingsStatus::Ready;
        self.rebuild_meeting_rows();
    }

    /// Shows `message` on the meetings page: as a notice above the list when meetings were
    /// already loaded (the list is kept), in place of the list otherwise.
    pub fn on_meetings_failed(&mut self, message: &'static str) {
        self.meetings_status = MeetingsStatus::Failed(message);
    }

    /// The session is gone: stops any countdown and moves to the login page with `status`
    /// (empty for none).
    pub fn on_session_lost(&mut self, status: &'static str) {
        self.target = None;
        self.timer_ms = 0;
        self.run = None;
        self.on_login_required(status);
    }

    /// Drops the loaded meetings, so the next visit to the meetings page starts loading.
    fn forget_meetings(&mut self) {
        self.meetings.clear();
        self.shown.clear();
        self.meetings_model.set_vec(Vec::new());
        self.meetings_loaded = false;
        self.meetings_status = MeetingsStatus::Loading;
    }

    /// True when an account is signed in.
    pub fn is_signed_in(&self) -> bool {
        self.account.is_some()
    }

    /// Page a finished or stopped countdown returns to: the meetings page when signed in (marked
    /// as loading only when no meetings were loaded yet), the selection page otherwise.
    fn home_page(&mut self) -> Page {
        if self.account.is_some() {
            if !self.meetings_loaded {
                self.meetings_status = MeetingsStatus::Loading;
            }
            Page::MeetingsPage
        } else {
            Page::SelectionPage
        }
    }

    /// Rebuilds the shown meetings and their rows from the received ones and the current time.
    fn rebuild_meeting_rows(&mut self) {
        let now = Local::now();
        let shown = visible_meetings(&self.meetings, &now.fixed_offset());
        let mut rows = Vec::with_capacity(shown.len());
        for meeting in &shown {
            rows.push(meeting_row(meeting, &now.fixed_offset(), &Local));
        }
        self.shown = shown;
        self.meetings_model.set_vec(rows);
    }

    /// Moves to the loading page shown while the saved session is checked.
    pub fn show_loading(&mut self) {
        self.page = Page::LoadingPage;
    }

    /// Records the signed-in account and moves to the meetings page, waiting for a fetch..
    pub fn on_signed_in(&mut self, me: &Me) {
        self.account = Some(account_label(me));
        self.notice = "";
        self.login_status = "";
        self.login_waiting = false;
        self.forget_meetings();
        self.page = Page::MeetingsPage;
    }

    /// Moves to the idle login page with `status` (empty for none), forgetting any account.
    pub fn on_login_required(&mut self, status: &'static str) {
        self.forget_meetings();
        self.account = None;
        self.login_status = status;
        self.login_waiting = false;
        self.page = Page::LoginPage;
    }

    /// Moves to the selection page without an account, showing `notice` (empty for none)..
    pub fn on_local_mode(&mut self, notice: &'static str) {
        self.account = None;
        self.notice = notice;
        self.login_waiting = false;
        self.page = Page::SelectionPage;
    }

    /// Shows the login page as waiting for the browser.
    pub fn on_login_started(&mut self) {
        self.login_status = LOGIN_WAITING_STATUS;
        self.login_waiting = true;
    }

    /// Ends the wait on the login page and shows `message`.
    pub fn on_login_failed(&mut self, message: &'static str) {
        self.login_status = message;
        self.login_waiting = false;
    }

    /// Ends the wait on the login page without a message.
    pub fn on_login_cancelled(&mut self) {
        self.login_status = "";
        self.login_waiting = false;
    }

    /// Hides the account (and its meetings) while the logout runs and tells the user it is in progress, on the
    /// selection page.
    pub fn on_logout_started(&mut self) {
        self.forget_meetings();
        self.account = None;
        self.notice = LOGOUT_NOTICE;
        self.page = Page::SelectionPage;
    }

    /// Stops any countdown, forgets the account and moves to the idle login page.
    pub fn on_logged_out(&mut self) {
        self.target = None;
        self.timer_ms = 0;
        self.run = None;
        self.notice = "";
        self.on_login_required("");
    }

    /// Binds the shared options model to `window`; call once after creating the window.
    ///
    /// Later option refreshes update the same model, so the UI follows without rebinding.
    pub fn bind_options(&self, window: &MainWindow) {
        window.set_options(ModelRc::from(self.options_model.clone()));
    }

    /// Binds the shared meeting rows model to `window`; call once after creating the window.
    pub fn bind_meetings(&self, window: &MainWindow) {
        window.set_meetings(ModelRc::from(self.meetings_model.clone()));
    }

    /// Pushes page, running state, timer, account and status lines to `window`.
    pub fn render(&self, window: &MainWindow) {
        window.set_page(self.page);
        window.set_running_state(self.target.is_some());
        window.set_timer_time(self.timer_ms);
        window.set_current_time(Local::now().format("%H:%M").to_string().into());
        let title = self.run.as_ref().map_or("", |meeting| {
            if meeting.title.is_empty() {
                "Meeting"
            } else {
                meeting.title.as_str()
            }
        });
        window.set_meeting_title(title.into());
        let end = self
            .target
            .map_or_else(String::new, |target| target.format("%H:%M").to_string());
        window.set_timer_end(end.into());
        let progress = self.run.as_ref().map_or(0.0, |meeting| {
            meeting_remaining_fraction(meeting, self.timer_ms)
        });
        window.set_timer_progress(progress);
        window.set_signed_in(self.account.is_some());
        window.set_account_name(self.account.as_deref().unwrap_or_default().into());
        window.set_notice(self.notice.into());
        window.set_login_status(self.login_status.into());
        window.set_login_waiting(self.login_waiting);
        window.set_meetings_loading(self.meetings_status == MeetingsStatus::Loading);
        let error = match self.meetings_status {
            MeetingsStatus::Failed(message) => message,
            MeetingsStatus::Loading | MeetingsStatus::Ready => "",
        };
        window.set_meetings_error(error.into());
    }
}

#[cfg(test)]
impl AppState {
    /// Page currently shown.
    pub fn page(&self) -> Page {
        self.page
    }

    /// Label of the signed-in account.
    pub fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }

    /// Status line of the login page.
    pub fn login_status(&self) -> &'static str {
        self.login_status
    }

    /// True when the meetings page shows an error.
    pub fn has_meetings_error(&self) -> bool {
        matches!(self.meetings_status, MeetingsStatus::Failed(_))
    }

    /// True while the login page waits for the browser.
    pub fn is_login_waiting(&self) -> bool {
        self.login_waiting
    }
}

/// Name shown for the signed-in account: the display name, or the email when the display name
/// is missing or blank.
fn account_label(me: &Me) -> String {
    match me.display_name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => me.email.clone(),
    }
}

/// True when `meeting` has started and not ended at `now`.
fn is_in_progress(meeting: &Meeting, now: &DateTime<FixedOffset>) -> bool {
    meeting.start <= *now && *now < meeting.end
}

/// Meetings worth showing at `now`: those not ended yet, the one in progress first and the rest
/// in start order.
///
/// Edge cases: with several meetings in progress they are ordered by start among themselves;
/// an empty input gives an empty list.
fn visible_meetings(meetings: &[Meeting], now: &DateTime<FixedOffset>) -> Vec<Meeting> {
    let mut shown = Vec::with_capacity(meetings.len());
    for meeting in meetings {
        if meeting.end > *now {
            shown.push(meeting.clone());
        }
    }
    shown.sort_by_key(|meeting| (!is_in_progress(meeting, now), meeting.start));
    shown
}

/// Fraction of the nominal meeting duration that remains, clamped to zero through one.
///
/// Invalid or sub-millisecond durations and expired countdowns give zero. A remaining time
/// longer than the nominal duration gives one; starting late never resets the bar to full.
fn meeting_remaining_fraction(meeting: &Meeting, remaining_ms: i64) -> f32 {
    let total_ms = meeting
        .end
        .signed_duration_since(meeting.start)
        .num_milliseconds();
    if total_ms <= 0 || remaining_ms <= 0 {
        return 0.0;
    }
    (remaining_ms as f64 / total_ms as f64).clamp(0.0, 1.0) as f32
}

/// Builds the row of `meeting`: times in `tz` as `HH:MM–HH:MM`, the title and the attendee count.
///
/// The count is the length of `attendees`, so it can be short for an event with omitted
/// attendees; one attendee is "1 partecipante", any other count is plural.
fn meeting_row<Tz: TimeZone>(meeting: &Meeting, now: &DateTime<FixedOffset>, tz: &Tz) -> MeetingRow
where
    Tz::Offset: Display,
{
    let count = meeting.attendees.len();
    let attendees = if count == 1 {
        "1 partecipante".to_string()
    } else {
        format!("{count} partecipanti")
    };

    MeetingRow {
        time_range: time_range(meeting, tz).into(),
        title: meeting.title.as_str().into(),
        attendees: attendees.into(),
        in_progress: is_in_progress(meeting, now),
    }
}

/// Formats the start and end of `meeting` in `tz` as `HH:MM–HH:MM`.
fn time_range<Tz: TimeZone>(meeting: &Meeting, tz: &Tz) -> String
where
    Tz::Offset: Display,
{
    let start = meeting.start.with_timezone(tz);
    let end = meeting.end.with_timezone(tz);
    format!("{}–{}", start.format("%H:%M"), end.format("%H:%M"))
}

/// Splits quarter options into their end times and the rows the UI shows.
///
/// Each row has the end time as `HH:MM` and the minutes left as a decimal
/// number. Order is preserved; an empty input gives two empty vectors.
fn split_options<Tz: TimeZone>(
    options: Vec<QuarterOption<Tz>>,
) -> (Vec<DateTime<Tz>>, Vec<TimeOption>)
where
    Tz::Offset: Display,
{
    let mut ends = Vec::with_capacity(options.len());
    let mut rows = Vec::with_capacity(options.len());

    for option in options {
        rows.push(TimeOption {
            time: option.end.format("%H:%M").to_string().into(),
            minutes: option.minutes_until.to_string().into(),
        });
        ends.push(option.end);
    }

    (ends, rows)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
    use schisaciaccole_core::api::{Attendee, ResponseStatus};
    use slint::Model;

    /// State on the selection page, as at startup without a backend.
    pub fn sample_state() -> AppState {
        AppState::new().unwrap()
    }

    fn me(display_name: Option<&str>) -> Me {
        Me {
            email: "ada@x.it".to_string(),
            display_name: display_name.map(str::to_string),
        }
    }

    fn utc(hour: u32, minute: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 3, 10)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn late_meeting_start_keeps_the_nominal_progress() {
        let now = Local::now().fixed_offset();
        let meeting = meeting(
            "daily",
            now - chrono::Duration::minutes(10),
            now + chrono::Duration::minutes(5),
        );

        let fraction = meeting_remaining_fraction(&meeting, 5 * 60 * 1000);

        assert!((fraction - 1.0 / 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn meeting_progress_clamps_clock_changes_and_expiry() {
        let now = Local::now().fixed_offset();
        let meeting = meeting("daily", now, now + chrono::Duration::minutes(15));

        assert_eq!(meeting_remaining_fraction(&meeting, 30 * 60 * 1000), 1.0);
        assert_eq!(meeting_remaining_fraction(&meeting, -1), 0.0);
        assert_eq!(meeting_remaining_fraction(&meeting, 0), 0.0);
    }

    #[test]
    fn invalid_meeting_duration_has_no_progress() {
        let now = Local::now().fixed_offset();
        let empty = meeting("empty", now, now);
        let reversed = meeting("reversed", now, now - chrono::Duration::minutes(1));
        let sub_millisecond = meeting("short", now, now + chrono::Duration::microseconds(1));

        assert_eq!(meeting_remaining_fraction(&empty, 60_000), 0.0);
        assert_eq!(meeting_remaining_fraction(&reversed, 60_000), 0.0);
        assert_eq!(meeting_remaining_fraction(&sub_millisecond, 1), 0.0);
    }

    #[test]
    fn split_formats_time_and_minutes() {
        let options = vec![QuarterOption {
            end: utc(9, 5),
            minutes_until: 8,
        }];

        let (_, rows) = split_options(options);

        assert_eq!(rows[0].time, "09:05");
        assert_eq!(rows[0].minutes, "8");
    }

    #[test]
    fn split_keeps_end_times_in_order() {
        let options = vec![
            QuarterOption {
                end: utc(9, 15),
                minutes_until: 8,
            },
            QuarterOption {
                end: utc(9, 30),
                minutes_until: 23,
            },
        ];

        let (ends, rows) = split_options(options);

        assert_eq!(ends, vec![utc(9, 15), utc(9, 30)]);
        assert_eq!(rows[1].time, "09:30");
        assert_eq!(rows[1].minutes, "23");
    }

    #[test]
    fn split_empty_gives_empty_vectors() {
        let (ends, rows) = split_options::<Utc>(Vec::new());

        assert!(ends.is_empty());
        assert!(rows.is_empty());
    }

    fn at(hour: u32, minute: u32) -> DateTime<FixedOffset> {
        utc(hour, minute).fixed_offset()
    }

    fn meeting(title: &str, start: DateTime<FixedOffset>, end: DateTime<FixedOffset>) -> Meeting {
        Meeting {
            event_id: title.to_string(),
            title: title.to_string(),
            start,
            end,
            attendees: Vec::new(),
            attendees_omitted: false,
            recurring_event_id: None,
            has_template: false,
        }
    }

    fn attendees(count: usize) -> Vec<Attendee> {
        let attendee = Attendee {
            email: "a@x.it".to_string(),
            name: "A".to_string(),
            is_self: false,
            optional: false,
            response: ResponseStatus::Accepted,
        };
        vec![attendee; count]
    }

    fn titles(meetings: &[Meeting]) -> Vec<&str> {
        meetings.iter().map(|m| m.title.as_str()).collect()
    }

    #[test]
    fn visible_puts_the_meeting_in_progress_first() {
        let meetings = vec![
            meeting("later", at(15, 0), at(16, 0)),
            meeting("now", at(9, 0), at(11, 0)),
            meeting("soon", at(12, 0), at(13, 0)),
        ];

        let shown = visible_meetings(&meetings, &at(10, 0));

        assert_eq!(titles(&shown), vec!["now", "soon", "later"]);
    }

    #[test]
    fn visible_drops_ended_meetings() {
        let meetings = vec![
            meeting("over", at(8, 0), at(9, 0)),
            meeting("ends-now", at(9, 0), at(10, 0)),
            meeting("next", at(11, 0), at(12, 0)),
        ];

        let shown = visible_meetings(&meetings, &at(10, 0));

        assert_eq!(titles(&shown), vec!["next"]);
    }

    #[test]
    fn visible_orders_overlapping_in_progress_by_start() {
        let meetings = vec![
            meeting("b", at(9, 30), at(11, 0)),
            meeting("a", at(9, 0), at(11, 0)),
        ];

        let shown = visible_meetings(&meetings, &at(10, 0));

        assert_eq!(titles(&shown), vec!["a", "b"]);
    }

    #[test]
    fn visible_of_nothing_is_empty() {
        assert!(visible_meetings(&[], &at(10, 0)).is_empty());
    }

    #[test]
    fn row_formats_times_in_the_given_zone() {
        let plus_two = FixedOffset::east_opt(2 * 3600).unwrap();
        let meeting = meeting("Daily", at(7, 5), at(7, 20));

        let row = meeting_row(&meeting, &at(6, 0), &plus_two);

        assert_eq!(row.time_range, "09:05–09:20");
        assert_eq!(row.title, "Daily");
    }

    #[test]
    fn row_marks_the_meeting_in_progress() {
        let meeting = meeting("Daily", at(9, 0), at(10, 0));

        assert!(meeting_row(&meeting, &at(9, 0), &Utc).in_progress);
        assert!(!meeting_row(&meeting, &at(8, 59), &Utc).in_progress);
        assert!(!meeting_row(&meeting, &at(10, 0), &Utc).in_progress);
    }

    #[test]
    fn row_counts_all_attendees() {
        let mut meeting = meeting("Review", at(9, 0), at(10, 0));
        meeting.attendees = attendees(3);

        let row = meeting_row(&meeting, &at(8, 0), &Utc);

        assert_eq!(row.attendees, "3 partecipanti");
    }

    #[test]
    fn row_uses_the_singular_for_one_attendee() {
        let mut meeting = meeting("1:1", at(9, 0), at(10, 0));
        meeting.attendees = attendees(1);

        let row = meeting_row(&meeting, &at(8, 0), &Utc);

        assert_eq!(row.attendees, "1 partecipante");
    }

    #[test]
    fn row_uses_the_plural_for_no_attendees() {
        let meeting = meeting("Solo", at(9, 0), at(10, 0));

        let row = meeting_row(&meeting, &at(8, 0), &Utc);

        assert_eq!(row.attendees, "0 partecipanti");
    }

    fn state_with_meetings() -> AppState {
        let now = Local::now().fixed_offset();
        let hour = chrono::Duration::hours(1);
        let mut state = sample_state();
        state.on_signed_in(&me(None));
        state.on_meetings_loaded(vec![
            meeting("later", now + hour, now + hour * 2),
            meeting("now", now - hour, now + hour),
        ]);
        state
    }

    #[test]
    fn loaded_meetings_fill_the_rows_in_order() {
        let state = state_with_meetings();

        assert_eq!(titles(&state.shown), vec!["now", "later"]);
        assert_eq!(state.meetings_model.row_count(), 2);
        assert_eq!(state.meetings_status, MeetingsStatus::Ready);
    }

    #[test]
    fn selecting_a_meeting_counts_down_to_its_end() {
        let mut state = state_with_meetings();
        let end = state.shown[0].end;

        let sound = state.select_meeting(0);

        assert_eq!(sound, TickSound::None);
        assert_eq!(state.page, Page::TimerPage);
        assert_eq!(state.target, Some(end.with_timezone(&Local)));
        assert!(state.timer_ms > 0);
    }

    #[test]
    fn selecting_an_out_of_range_meeting_changes_nothing() {
        let mut state = state_with_meetings();

        let past_the_end = state.select_meeting(2);
        let negative = state.select_meeting(-1);

        assert_eq!(past_the_end, TickSound::None);
        assert_eq!(negative, TickSound::None);

        assert_eq!(state.page, Page::MeetingsPage);
        assert_eq!(state.target, None);
    }

    #[test]
    fn stop_returns_to_meetings_when_signed_in() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);

        state.stop(StopType::UserStopped);

        assert_eq!(state.page, Page::MeetingsPage);
        assert_eq!(state.meetings_status, MeetingsStatus::Ready);
        assert_eq!(state.shown.len(), 2);
    }

    #[test]
    fn entering_meetings_without_data_shows_loading() {
        let mut state = sample_state();
        state.on_signed_in(&me(None));
        state.page = Page::SelectionPage;

        state.show_meetings_page();

        assert_eq!(state.meetings_status, MeetingsStatus::Loading);
    }

    #[test]
    fn entering_meetings_with_data_keeps_the_list() {
        let mut state = state_with_meetings();
        state.page = Page::SelectionPage;

        state.show_meetings_page();

        assert_eq!(state.page, Page::MeetingsPage);
        assert_eq!(state.meetings_status, MeetingsStatus::Ready);
        assert_eq!(state.meetings_model.row_count(), 2);
    }

    #[test]
    fn failed_refresh_keeps_the_list() {
        let mut state = state_with_meetings();

        state.on_meetings_failed("errore");

        assert_eq!(state.meetings_model.row_count(), 2);
        assert_eq!(state.meetings_status, MeetingsStatus::Failed("errore"));
    }

    #[test]
    fn successful_refresh_clears_the_error() {
        let mut state = state_with_meetings();
        state.on_meetings_failed("errore");

        state.on_meetings_loaded(Vec::new());

        assert_eq!(state.meetings_status, MeetingsStatus::Ready);
    }

    #[test]
    fn signing_in_again_forgets_the_old_meetings() {
        let mut state = state_with_meetings();
        state.on_logged_out();

        state.on_signed_in(&me(None));

        assert_eq!(state.meetings_status, MeetingsStatus::Loading);
        assert_eq!(state.meetings_model.row_count(), 0);
    }

    #[test]
    fn stop_returns_to_selection_when_not_signed_in() {
        let mut state = sample_state();
        state.target = Some(Local::now());

        state.stop(StopType::UserStopped);

        assert_eq!(state.page, Page::SelectionPage);
    }

    #[test]
    fn leaving_the_finished_page_goes_home() {
        let mut signed_in = state_with_meetings();
        signed_in.page = Page::TimerFinished;
        signed_in.show_home();
        assert_eq!(signed_in.page, Page::MeetingsPage);

        let mut local = sample_state();
        local.page = Page::TimerFinished;
        local.show_home();
        assert_eq!(local.page, Page::SelectionPage);
    }

    #[test]
    fn meetings_failure_is_kept_for_the_page() {
        let mut state = state_with_meetings();

        state.on_meetings_failed("errore");

        assert_eq!(state.meetings_status, MeetingsStatus::Failed("errore"));
    }

    #[test]
    fn session_lost_stops_the_timer_and_shows_login() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);

        state.on_session_lost("msg");

        assert_eq!(state.page, Page::LoginPage);
        assert_eq!(state.target, None);
        assert_eq!(state.login_status, "msg");
        assert!(!state.is_signed_in());
    }

    #[test]
    fn label_prefers_the_display_name() {
        assert_eq!(account_label(&me(Some("Ada Lovelace"))), "Ada Lovelace");
    }

    #[test]
    fn label_falls_back_to_the_email() {
        assert_eq!(account_label(&me(None)), "ada@x.it");
        assert_eq!(account_label(&me(Some("  "))), "ada@x.it");
    }

    #[test]
    fn loading_shows_the_loading_page() {
        let mut state = sample_state();
        state.show_loading();
        assert_eq!(state.page, Page::LoadingPage);
    }

    #[test]
    fn signed_in_shows_meetings_with_account() {
        let mut state = sample_state();
        state.on_login_required("");
        state.on_login_started();

        state.on_signed_in(&me(Some("Ada")));

        assert_eq!(state.page, Page::MeetingsPage);
        assert_eq!(state.account.as_deref(), Some("Ada"));
        assert!(!state.login_waiting);
        assert_eq!(state.login_status, "");
        assert_eq!(state.notice, "");
    }

    #[test]
    fn login_required_shows_login_page_with_status() {
        let mut state = sample_state();
        state.on_signed_in(&me(None));

        state.on_login_required("msg");

        assert_eq!(state.page, Page::LoginPage);
        assert_eq!(state.account, None);
        assert_eq!(state.login_status, "msg");
    }

    #[test]
    fn local_mode_shows_selection_with_notice() {
        let mut state = sample_state();
        state.show_loading();

        state.on_local_mode(OFFLINE_NOTICE);

        assert_eq!(state.page, Page::SelectionPage);
        assert_eq!(state.account, None);
        assert_eq!(state.notice, OFFLINE_NOTICE);
    }

    #[test]
    fn login_started_waits_for_the_browser() {
        let mut state = sample_state();
        state.on_login_required("");

        state.on_login_started();

        assert!(state.login_waiting);
        assert_eq!(state.login_status, LOGIN_WAITING_STATUS);
        assert_eq!(state.page, Page::LoginPage);
    }

    #[test]
    fn login_failed_stops_waiting_and_shows_message() {
        let mut state = sample_state();
        state.on_login_required("");
        state.on_login_started();

        state.on_login_failed("errore");

        assert!(!state.login_waiting);
        assert_eq!(state.login_status, "errore");
        assert_eq!(state.page, Page::LoginPage);
    }

    #[test]
    fn login_cancelled_clears_the_status() {
        let mut state = sample_state();
        state.on_login_required("");
        state.on_login_started();

        state.on_login_cancelled();

        assert!(!state.login_waiting);
        assert_eq!(state.login_status, "");
    }

    #[test]
    fn logout_started_hides_the_account() {
        let mut state = sample_state();
        state.on_signed_in(&me(None));

        state.on_logout_started();

        assert_eq!(state.account, None);
        assert_eq!(state.notice, LOGOUT_NOTICE);
        assert_eq!(state.page, Page::SelectionPage);
    }

    #[test]
    fn logged_out_stops_the_timer_and_shows_login() {
        let mut state = sample_state();
        state.on_signed_in(&me(None));
        state.target = Some(Local::now());
        state.timer_ms = 1000;
        state.on_logout_started();

        state.on_logged_out();

        assert_eq!(state.page, Page::LoginPage);
        assert_eq!(state.target, None);
        assert_eq!(state.timer_ms, 0);
        assert_eq!(state.account, None);
        assert_eq!(state.notice, "");
        assert_eq!(state.login_status, "");
    }
}
