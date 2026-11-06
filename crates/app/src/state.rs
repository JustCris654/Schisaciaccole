//! Application state: the single source of truth that the Slint UI renders.

use std::fmt::Display;
use std::rc::Rc;

use chrono::{DateTime, FixedOffset, Local, TimeZone};
use log::{debug, error, info, warn};
use schisaciaccole_core::api::{Me, Meeting, PlanEntry, SlotPlan};
use schisaciaccole_core::{
    DEFAULT_OPTION_COUNT, QuarterOption, SlotSchedule, TickOutcome, TimeError, build_plan,
    evaluate, participants, quarter_options, resolve_target,
};
use slint::{ModelRc, VecModel};

use crate::backend::BackendError;
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
    /// The current speaking slot ended and the next one started: a short beep.
    SlotEnd,
    /// The meeting or countdown finished: the finish sound.
    MeetingEnd,
}

/// What a call to [`AppState::select_meeting`] asks of the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingSelection {
    /// Sound to play, see [`AppState::tick`].
    pub sound: TickSound,
    /// Event id of the meeting whose slot plan the caller must fetch, if any; the result goes to
    /// [`AppState::on_plan_loaded`].
    pub plan_event_id: Option<String>,
}

/// Meeting whose countdown is running, kept to build the speaking slots when its plan arrives.
struct MeetingRun {
    event_id: String,
    meeting: Meeting,
    /// Instant of the click: the slots split the time from here to the meeting end.
    started_at: DateTime<Local>,
    /// True until the plan result (or its fallback) has been applied.
    plan_pending: bool,
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
    /// Speaking slots of the running meeting; `None` for a plain countdown.
    slots: Option<SlotSchedule<Local>>,
    /// Index of the slot seen at the last update, to detect an automatic advance.
    slot_index: Option<usize>,
    /// True when the attendee list of the selected meeting is incomplete.
    slots_warning: bool,
    /// Meeting selected from the list while its countdown runs.
    run: Option<MeetingRun>,
    /// True when the saved plan could not be loaded and equal slots are used instead.
    plan_fallback: bool,
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
            slots: None,
            slot_index: None,
            slots_warning: false,
            run: None,
            plan_fallback: false,
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
    /// past moves to the next one. Any speaking slots and the attendees warning are cleared.
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

        self.clear_slots();
        self.page = Page::TimerPage;
        self.target = Some(target);
        self.tick()
    }

    /// Starts the countdown to the end of the meeting in row `index` and moves to the timer page.
    ///
    /// The countdown starts at once and without speaking slots: the slot plan is fetched by the
    /// caller, which must do it for the event id in [`MeetingSelection::plan_event_id`] and pass
    /// the result to [`AppState::on_plan_loaded`]. Until then the timer shows the plan as
    /// loading. The attendees warning is set when the calendar omitted some attendees. The sound
    /// is [`TickSound::MeetingEnd`] only if the end is reached at once (see [`AppState::tick`]).
    ///
    /// Edge cases: a negative `index` or one out of range is logged and ignored; the state does
    /// not change, the sound is [`TickSound::None`] and there is no plan to fetch. When the
    /// countdown stops at once (meeting already over) there is no plan to fetch either.
    #[must_use]
    pub fn select_meeting(&mut self, index: i32) -> MeetingSelection {
        let nothing = MeetingSelection {
            sound: TickSound::None,
            plan_event_id: None,
        };
        let Ok(position) = usize::try_from(index) else {
            warn!("select_meeting: ignoring negative meeting index {index}");
            return nothing;
        };
        let Some(meeting) = self.shown.get(position) else {
            warn!("select_meeting: ignoring out of range meeting index {index}");
            return nothing;
        };

        let now = Local::now();
        let target = meeting.end.with_timezone(&Local);
        let slots_warning = meeting.attendees_omitted;
        let run = MeetingRun {
            event_id: meeting.event_id.clone(),
            meeting: meeting.clone(),
            started_at: now,
            plan_pending: true,
        };
        info!("select_meeting: target={}", target.format("%H:%M"));

        self.clear_slots();
        self.slots_warning = slots_warning;
        self.run = Some(run);
        self.page = Page::TimerPage;
        self.target = Some(target);
        let sound = self.tick();
        let plan_event_id = self
            .run
            .as_ref()
            .filter(|run| run.plan_pending)
            .map(|run| run.event_id.clone());
        MeetingSelection {
            sound,
            plan_event_id,
        }
    }

    /// Applies the slot plan fetched for `event_id`; see [`AppState::apply_plan`].
    ///
    /// Never stops the countdown and never plays a sound, whatever the result.
    pub fn on_plan_loaded(&mut self, event_id: &str, result: Result<SlotPlan, BackendError>) {
        self.apply_plan(event_id, result, &Local::now());
    }

    /// Builds the speaking slots of the running meeting from a plan result, at time `now`.
    ///
    /// The time from the click to the meeting end is split by the weights of the plan entries
    /// above zero, in order. On an error, whatever it is, equal slots for the participants of the
    /// meeting are used instead and the fallback warning is shown; a running countdown is never
    /// interrupted by a plan failure. The current slot is the one covering `now`, so a plan that
    /// arrives late does not replay the slots already over and no sound is due.
    ///
    /// Edge cases: the result is dropped when the timer is not running on the meeting
    /// `event_id` (stopped, another meeting, other page) or when its plan was already applied.
    /// With fewer than two slots, or slots that would be too short, the countdown stays plain.
    fn apply_plan(
        &mut self,
        event_id: &str,
        result: Result<SlotPlan, BackendError>,
        now: &DateTime<Local>,
    ) {
        let (Some(target), Some(run)) = (self.target, self.run.as_mut()) else {
            debug!("apply_plan: no running meeting, dropping the plan");
            return;
        };
        if self.page != Page::TimerPage || run.event_id != event_id || !run.plan_pending {
            debug!("apply_plan: plan is not for the running meeting, dropping it");
            return;
        }
        run.plan_pending = false;
        let started_at = run.started_at;

        let weighted = match result {
            Ok(plan) => weighted_slots(&plan.entries),
            Err(err) => {
                warn!("apply_plan: plan not loaded, using equal slots: {err}");
                let equal = build_plan(&participants(&run.meeting), None);
                self.plan_fallback = true;
                weighted_slots(&equal)
            }
        };
        let slots = SlotSchedule::new(weighted, &started_at, &target);
        self.slot_index = slots
            .as_ref()
            .and_then(|schedule| schedule.current(now))
            .map(|view| view.index);
        info!(
            "apply_plan: slots={}",
            slots.as_ref().map_or(0, SlotSchedule::len)
        );
        self.slots = slots;
    }

    /// Updates the countdown against the current time.
    ///
    /// While the target is ahead, stores the remaining milliseconds; if the speaking slot
    /// advanced since the last update it returns [`TickSound::SlotEnd`], once even when several
    /// slots passed (e.g. after a suspend). When the target has just been reached, stops with
    /// [`StopType::TimerFinished`] and returns [`TickSound::MeetingEnd`]: the end of the last
    /// slot is the end of the meeting, so no slot beep is added. When it passed more than the
    /// grace period ago (e.g. after a suspend), stops with [`StopType::UserStopped`] without
    /// sound.
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
                self.update_slot_index(&now)
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

    /// Stores the index of the current slot and tells whether it moved forward.
    fn update_slot_index(&mut self, now: &DateTime<Local>) -> TickSound {
        let Some(view) = self
            .slots
            .as_ref()
            .and_then(|schedule| schedule.current(now))
        else {
            return TickSound::None;
        };
        let advanced = self.slot_index.is_some_and(|seen| view.index > seen);
        self.slot_index = Some(view.index);
        if advanced {
            TickSound::SlotEnd
        } else {
            TickSound::None
        }
    }

    /// Ends the current speaking slot now and shares the time left among the next ones.
    ///
    /// No sound is played for a manual skip.
    ///
    /// Edge cases: nothing happens without slots, outside the timer page, or when the current
    /// slot is the last one.
    pub fn next_slot(&mut self) {
        if self.page != Page::TimerPage {
            return;
        }
        let now = Local::now();
        let Some(schedule) = self.slots.as_mut() else {
            return;
        };
        if schedule.skip(&now) {
            self.slot_index = schedule.current(&now).map(|view| view.index);
        }
    }

    /// Drops the speaking slots, the seen index, the warnings and the running meeting, so a plan
    /// still being fetched is ignored when it arrives.
    fn clear_slots(&mut self) {
        self.slots = None;
        self.slot_index = None;
        self.slots_warning = false;
        self.run = None;
        self.plan_fallback = false;
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
        self.clear_slots();
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
        self.clear_slots();
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

    /// Records the signed-in account and moves to the meetings page, waiting for a fetch.
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

    /// Moves to the selection page without an account, showing `notice` (empty for none).
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
        self.clear_slots();
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

        let view = self
            .slots
            .as_ref()
            .and_then(|schedule| schedule.current(&Local::now()));
        window.set_has_slots(view.is_some());
        window.set_slots_warning(self.slots_warning);
        window.set_plan_loading(self.run.as_ref().is_some_and(|run| run.plan_pending));
        window.set_plan_warning(self.plan_fallback);
        match view {
            Some(view) => {
                window.set_slot_name(view.name.as_str().into());
                window.set_slot_time(view.remaining_ms);
                window.set_next_slot_name(view.next_name.unwrap_or_default().into());
            }
            None => {
                window.set_slot_name("".into());
                window.set_slot_time(0);
                window.set_next_slot_name("".into());
            }
        }
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

    /// True when speaking slots are active.
    pub fn has_slots(&self) -> bool {
        self.slots.is_some()
    }

    /// Name of the person speaking now; `None` without slots.
    pub fn current_slot_name(&self) -> Option<String> {
        let schedule = self.slots.as_ref()?;
        schedule.current(&Local::now()).map(|view| view.name)
    }

    /// True while the slot plan of the running meeting is being fetched.
    pub fn is_plan_pending(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.plan_pending)
    }

    /// True when the fallback warning for a plan that could not be loaded is shown.
    pub fn has_plan_warning(&self) -> bool {
        self.plan_fallback
    }

    /// True when the attendees warning is shown.
    pub fn has_slots_warning(&self) -> bool {
        self.slots_warning
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

/// Turns plan entries into `(label, weight)` slots: entries with weight 0 are skipped, the order
/// is kept. Empty input gives an empty list.
fn weighted_slots(entries: &[PlanEntry]) -> Vec<(String, u64)> {
    let mut weighted = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.weight > 0 {
            weighted.push((entry.label.clone(), entry.weight));
        }
    }
    weighted
}

/// Builds the row of `meeting`: times in `tz` as `HH:MM–HH:MM`, the title, the attendee count and
/// whether the meeting has saved slots.
///
/// The count is the length of `attendees`, so it can be short for an event with omitted
/// attendees; one attendee is "1 partecipante", any other count is plural.
fn meeting_row<Tz: TimeZone>(meeting: &Meeting, now: &DateTime<FixedOffset>, tz: &Tz) -> MeetingRow
where
    Tz::Offset: Display,
{
    let start = meeting.start.with_timezone(tz);
    let end = meeting.end.with_timezone(tz);
    let count = meeting.attendees.len();
    let attendees = if count == 1 {
        "1 partecipante".to_string()
    } else {
        format!("{count} partecipanti")
    };

    MeetingRow {
        time_range: format!("{}–{}", start.format("%H:%M"), end.format("%H:%M")).into(),
        title: meeting.title.as_str().into(),
        attendees: attendees.into(),
        in_progress: is_in_progress(meeting, now),
        has_template: meeting.has_template,
    }
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
    use schisaciaccole_core::api::{Attendee, EntryKind, PlanSource, ResponseStatus};
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

        let selection = state.select_meeting(0);

        assert_eq!(selection.sound, TickSound::None);
        assert_eq!(state.page, Page::TimerPage);
        assert_eq!(state.target, Some(end.with_timezone(&Local)));
        assert!(state.timer_ms > 0);
    }

    #[test]
    fn selecting_an_out_of_range_meeting_changes_nothing() {
        let mut state = state_with_meetings();

        let past_the_end = state.select_meeting(2);
        let negative = state.select_meeting(-1);

        assert_eq!(past_the_end.sound, TickSound::None);
        assert_eq!(past_the_end.plan_event_id, None);
        assert_eq!(negative.plan_event_id, None);

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

    fn named_attendee(name: &str) -> Attendee {
        Attendee {
            email: format!("{}@x.it", name.to_lowercase()),
            name: name.to_string(),
            is_self: false,
            optional: false,
            response: ResponseStatus::Accepted,
        }
    }

    /// State with one meeting in progress (ends in about 30 minutes), selected on the timer page.
    pub fn state_on_timer(names: &[&str], attendees_omitted: bool) -> AppState {
        let now = Local::now().fixed_offset();
        let mut in_progress = meeting(
            "now",
            now - chrono::Duration::hours(1),
            now + chrono::Duration::minutes(30),
        );
        in_progress.attendees = names.iter().map(|name| named_attendee(name)).collect();
        in_progress.attendees_omitted = attendees_omitted;
        let mut state = sample_state();
        state.on_signed_in(&me(None));
        state.on_meetings_loaded(vec![in_progress]);
        let _ = state.select_meeting(0);
        state
    }

    /// State on the timer with the equal plan of the attendees already applied.
    fn state_with_equal_slots(names: &[&str]) -> AppState {
        let mut state = state_on_timer(names, false);
        state.on_plan_loaded("now", Err(BackendError::Unreachable));
        state
    }

    #[test]
    fn meeting_with_several_attendees_gets_slots() {
        let state = state_with_equal_slots(&["Carl", "Ann", "Bob"]);

        assert!(state.has_slots());
        assert_eq!(state.current_slot_name().as_deref(), Some("Ann"));
        assert!(!state.has_slots_warning());
    }

    #[test]
    fn meeting_with_one_attendee_has_no_slots() {
        let state = state_with_equal_slots(&["Ann"]);

        assert!(!state.has_slots());
        assert_eq!(state.current_slot_name(), None);
    }

    #[test]
    fn next_slot_without_slots_does_nothing() {
        let mut state = state_with_equal_slots(&["Ann"]);
        let target = state.target;

        state.next_slot();

        assert!(!state.has_slots());
        assert_eq!(state.slot_index, None);
        assert_eq!(state.target, target);
        assert_eq!(state.page, Page::TimerPage);
    }

    #[test]
    fn next_slot_moves_to_the_second_participant() {
        let mut state = state_with_equal_slots(&["Carl", "Ann", "Bob"]);

        state.next_slot();

        assert_eq!(state.current_slot_name().as_deref(), Some("Bob"));
        assert_eq!(state.slot_index, Some(1));
    }

    #[test]
    fn manual_skip_does_not_beep() {
        let mut state = state_with_equal_slots(&["Carl", "Ann", "Bob"]);

        state.next_slot();

        assert_eq!(state.tick(), TickSound::None);
    }

    // Simulates a slot that ended by itself: the stored index is behind the schedule.
    #[test]
    fn tick_reports_the_end_of_a_slot_once() {
        let mut state = state_with_equal_slots(&["Carl", "Ann", "Bob"]);
        state.next_slot();
        state.slot_index = Some(0);

        assert_eq!(state.tick(), TickSound::SlotEnd);
        assert_eq!(state.tick(), TickSound::None);
    }

    #[test]
    fn stop_clears_the_slots() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], true);
        state.on_plan_loaded("now", Err(BackendError::Unreachable));

        state.stop(StopType::UserStopped);

        assert!(!state.has_slots());
        assert_eq!(state.slot_index, None);
        assert!(!state.has_slots_warning());
    }

    #[test]
    fn omitted_attendees_set_the_warning_even_without_slots() {
        let state = state_on_timer(&["Ann"], true);

        assert!(!state.has_slots());
        assert!(state.has_slots_warning());
    }

    #[test]
    fn selecting_a_quarter_clears_the_slots_and_the_warning() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], true);
        state.on_plan_loaded("now", Err(BackendError::Unreachable));
        state.stop(StopType::UserStopped);
        state.slots_warning = true;

        let _ = state.select(0);

        assert!(!state.has_slots());
        assert!(!state.has_slots_warning());
        assert!(!state.has_plan_warning());
    }

    fn plan_entry(label: &str, weight: u64) -> PlanEntry {
        PlanEntry {
            kind: EntryKind::Attendee,
            email: Some(format!("{}@x.it", label.to_lowercase())),
            label: label.to_string(),
            weight,
        }
    }

    fn plan_of(entries: Vec<PlanEntry>) -> SlotPlan {
        SlotPlan {
            source: PlanSource::Template,
            template_id: Some("t1".to_string()),
            entries,
        }
    }

    /// Timer on a meeting that started 40 minutes ago (click instant) and still has its plan
    /// pending; returns the state, the click instant and the meeting end.
    fn pending_state() -> (AppState, DateTime<Local>, DateTime<Local>) {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);
        let target = state.target.unwrap();
        let started = target - chrono::Duration::minutes(40);
        state.run.as_mut().unwrap().started_at = started;
        (state, started, target)
    }

    #[test]
    fn selecting_a_meeting_starts_without_slots_and_waits_for_the_plan() {
        let mut state = state_with_meetings();

        let selection = state.select_meeting(0);

        assert_eq!(selection.plan_event_id.as_deref(), Some("now"));
        assert_eq!(selection.sound, TickSound::None);
        assert!(state.timer_ms > 0);
        assert!(!state.has_slots());
        assert!(state.is_plan_pending());
    }

    #[test]
    fn plan_splits_the_time_from_the_click_by_weight() {
        let (mut state, started, _) = pending_state();
        let now = started + chrono::Duration::minutes(10);
        let plan = plan_of(vec![
            plan_entry("A", 2),
            plan_entry("B", 1),
            plan_entry("C", 1),
        ]);

        state.apply_plan("now", Ok(plan), &now);

        let view = state.slots.as_ref().unwrap().current(&now).unwrap();
        assert_eq!(view.name, "A");
        // First slot is 20 of the 40 minutes, 10 of which are gone.
        assert_eq!(view.remaining_ms, 10 * 60 * 1000);
        assert!(!state.is_plan_pending());
        assert!(!state.has_plan_warning());
    }

    #[test]
    fn plan_skips_zero_weight_entries() {
        let (mut state, started, _) = pending_state();
        let plan = plan_of(vec![
            plan_entry("A", 60),
            plan_entry("B", 0),
            plan_entry("C", 60),
        ]);

        state.apply_plan("now", Ok(plan), &started);

        let schedule = state.slots.as_ref().unwrap();
        assert_eq!(schedule.len(), 2);
        let view = schedule.current(&started).unwrap();
        assert_eq!(view.next_name.as_deref(), Some("C"));
    }

    #[test]
    fn plan_with_one_positive_entry_stays_a_plain_timer() {
        let (mut state, started, _) = pending_state();
        let plan = plan_of(vec![plan_entry("A", 60), plan_entry("B", 0)]);

        state.apply_plan("now", Ok(plan), &started);

        assert!(!state.has_slots());
        assert_eq!(state.slot_index, None);
        assert!(!state.is_plan_pending());
    }

    // The plan arrives 15 minutes after the click, when the first slot is already over.
    #[test]
    fn late_plan_starts_on_the_covering_slot_without_a_beep() {
        let (mut state, started, _) = pending_state();
        let now = started + chrono::Duration::minutes(15);
        let plan = plan_of(vec![
            plan_entry("A", 1),
            plan_entry("B", 1),
            plan_entry("C", 2),
        ]);

        state.apply_plan("now", Ok(plan), &now);

        assert_eq!(state.slot_index, Some(1));
        assert_eq!(state.update_slot_index(&now), TickSound::None);
    }

    #[test]
    fn plan_error_falls_back_to_equal_slots_with_a_warning() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);

        state.on_plan_loaded("now", Err(BackendError::Unreachable));

        assert_eq!(state.current_slot_name().as_deref(), Some("Ann"));
        assert_eq!(state.slots.as_ref().unwrap().len(), 3);
        assert!(state.has_plan_warning());
        assert!(!state.is_plan_pending());
        assert_eq!(state.page, Page::TimerPage);
        assert!(state.target.is_some());
    }

    #[test]
    fn plan_auth_error_does_not_end_the_session_or_the_timer() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);

        state.on_plan_loaded("now", Err(BackendError::NotLoggedIn));
        state.on_plan_loaded("now", Err(BackendError::ReauthRequired));

        assert_eq!(state.page, Page::TimerPage);
        assert!(state.target.is_some());
        assert!(state.is_signed_in());
        assert!(state.has_plan_warning());
    }

    #[test]
    fn plan_error_with_one_attendee_leaves_a_plain_timer_with_the_warning() {
        let mut state = state_on_timer(&["Ann"], false);

        state.on_plan_loaded("now", Err(BackendError::Decode));

        assert!(!state.has_slots());
        assert!(state.has_plan_warning());
    }

    #[test]
    fn plan_for_another_meeting_is_ignored() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);
        let plan = plan_of(vec![plan_entry("A", 1), plan_entry("B", 1)]);

        state.on_plan_loaded("other", Ok(plan));

        assert!(!state.has_slots());
        assert!(state.is_plan_pending());
    }

    #[test]
    fn plan_after_stop_is_ignored() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);
        state.stop(StopType::UserStopped);
        let plan = plan_of(vec![plan_entry("A", 1), plan_entry("B", 1)]);

        state.on_plan_loaded("now", Ok(plan));

        assert!(!state.has_slots());
        assert_eq!(state.page, Page::MeetingsPage);
    }

    #[test]
    fn plan_applied_twice_keeps_the_first_result() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);
        state.on_plan_loaded("now", Err(BackendError::Unreachable));
        let plan = plan_of(vec![plan_entry("A", 1), plan_entry("B", 1)]);

        state.on_plan_loaded("now", Ok(plan));

        assert_eq!(state.slots.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn selecting_a_quarter_drops_the_pending_plan() {
        let mut state = state_on_timer(&["Carl", "Ann", "Bob"], false);
        state.stop(StopType::UserStopped);

        let _ = state.select(0);

        assert!(!state.is_plan_pending());
        assert!(!state.has_plan_warning());
    }

    #[test]
    fn row_shows_whether_the_meeting_has_saved_slots() {
        let mut meeting = meeting("Retro", at(9, 0), at(10, 0));
        assert!(!meeting_row(&meeting, &at(8, 0), &Utc).has_template);

        meeting.has_template = true;

        assert!(meeting_row(&meeting, &at(8, 0), &Utc).has_template);
    }
}
