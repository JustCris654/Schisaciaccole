//! Application state: the single source of truth that the Slint UI renders.

use std::collections::HashMap;
use std::fmt::Display;
use std::rc::Rc;

use crate::backend::BackendError;
use crate::editor::SlotEditor;
use crate::login::REAUTH_MESSAGE;
use chrono::{DateTime, FixedOffset, Local, TimeZone};
use log::{debug, error, info, warn};
use schisaciaccole_core::api::{
    EntryKind, Me, Meeting, PlanEntry, PutTemplateRequest, SlotPlan, Template, error_codes,
};
use schisaciaccole_core::{
    DEFAULT_OPTION_COUNT, DailyRound, EditorEntry, FREE_TIME_LABEL, QuarterOption, SlotSchedule,
    SpeakingTurn, TickOutcome, TimeError, abbreviate_name, evaluate, looks_daily, max_turn_minutes,
    participants, quarter_options, shuffled,
};
use slint::{ComponentHandle, LogicalSize, Model, ModelRc, SharedString, VecModel};

use crate::ui::{EditorRow, MainWindow, MeetingRow, Page, StopType, TimeOption, TurnView};

/// Login page status while the browser login is in progress.
pub const LOGIN_WAITING_STATUS: &str = "Completa l'accesso nel browser…";
/// Manual page notice when the backend cannot be reached at startup.
pub const OFFLINE_NOTICE: &str = "Server non raggiungibile: modalità locale";
/// Manual page notice when the backend client cannot be created.
pub const LOCAL_MODE_NOTICE: &str = "Accesso non disponibile: modalità locale";
/// Manual page notice while the logout runs.
pub const LOGOUT_NOTICE: &str = "Disconnessione…";

/// Editor notice when the saved plan could not be loaded.
const EDITOR_PLAN_FALLBACK_NOTICE: &str = "Preferenza non caricata: slot uguali";
/// Editor notice while a template save runs.
const TEMPLATE_SAVING_NOTICE: &str = "Salvataggio…";
/// Editor notice after a template save.
const TEMPLATE_SAVED_NOTICE: &str = "Preferenza salvata";
/// Editor notice when the backend rejects the template as invalid.
const TEMPLATE_INVALID_NOTICE: &str = "Preferenza non valida";
/// Editor notice when a template save fails for any other reason.
const TEMPLATE_SAVE_FAILED_NOTICE: &str = "Salvataggio non riuscito";
/// Editor notice while a template deletion runs.
const TEMPLATE_DELETING_NOTICE: &str = "Eliminazione…";
/// Editor notice after a template deletion.
const TEMPLATE_DELETED_NOTICE: &str = "Preferenza eliminata";
/// Editor notice when a template deletion fails.
const TEMPLATE_DELETE_FAILED_NOTICE: &str = "Eliminazione non riuscita";
/// Editor notice when "Avvia" is pressed after the meeting ended.
const MEETING_OVER_NOTICE: &str = "Il meeting è già terminato";

/// Minutes a speaking turn starts with.
const DEFAULT_TURN_MINUTES: u32 = 5;
const MINUTE_MS: i64 = 60_000;
/// Name shown by the Daily screen once everybody has spoken.
const DAILY_FINISHED_NAME: &str = "Tutti hanno parlato";

/// Sound the caller must play after a state update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickSound {
    /// No sound.
    None,
    /// The meeting or countdown finished: the finish sound.
    MeetingEnd,
    /// The running speaking turn ran out: the soft beep.
    TurnEnd,
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

/// Where the running countdown was started from, so leaving it goes back there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerOrigin {
    /// A slot picked on the manual page.
    Manual,
    /// A meeting picked on the meetings page.
    Meeting,
}

/// Size of the window while a countdown shows.
const TIMER_WINDOW_SIZE: LogicalSize = LogicalSize::new(360.0, 240.0);

/// Window geometry to bring back when the countdown screens are left.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SavedWindow {
    size: LogicalSize,
    on_top: bool,
}

/// What [`AppState::render`] must do to the window size and on-top state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowStep {
    /// The window already matches the page.
    Keep,
    /// A countdown screen just opened: shrink the window and keep it on top.
    Enter,
    /// The countdown screens were left: restore the window.
    Leave,
}

/// Template save the caller must send; see [`AppState::start_template_save`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSave {
    /// Meeting of the editor that asked for the save; its result must come back with it.
    pub event_id: String,
    /// Template to save.
    pub request: PutTemplateRequest,
}

/// Template deletion the caller must send; see [`AppState::start_template_delete`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDelete {
    /// Meeting of the editor that asked for the deletion; its result must come back with it.
    pub event_id: String,
    /// Id of the template to delete.
    pub template_id: String,
}

/// Editor page open on one meeting.
struct OpenEditor {
    editor: SlotEditor,
    /// True until the plan result (or its fallback) has filled the rows.
    loading: bool,
    /// True while a template save or deletion runs.
    busy: bool,
    /// Status line of the editor page; empty when there is none.
    notice: &'static str,
}

/// Everything the main window shows, owned by Rust and pushed to Slint by [`AppState::render`].
pub struct AppState {
    page: Page,
    editor: Option<OpenEditor>,
    editor_model: Rc<VecModel<EditorRow>>,
    slots: Option<SlotSchedule<Local>>,
    slot_index: Option<usize>,
    free_time: bool,
    options: Vec<DateTime<Local>>,
    /// End instant of the slot picked on the manual page; `None` when no slot is picked.
    selected: Option<DateTime<Local>>,
    target: Option<DateTime<Local>>,
    timer_ms: i64,
    options_model: Rc<VecModel<TimeOption>>,
    /// Label of the signed-in account; `None` when not signed in.
    account: Option<String>,
    /// Status line of the manual page; empty when there is none.
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
    /// Origin of the running countdown, kept while the finished page shows.
    origin: Option<TimerOrigin>,
    /// View of the meeting countdown: plain, turn settings or a running turn.
    turn_view: TurnView,
    /// Names of the participants of the running meeting countdown, in the select order.
    turn_names: Vec<String>,
    turn_names_model: Rc<VecModel<SharedString>>,
    /// Position of the chosen participant in `turn_names`.
    turn_selected: usize,
    /// Minutes the next turn will last.
    turn_minutes: u32,
    /// The running speaking turn; `Some` only in [`TurnView::Running`].
    turn: Option<SpeakingTurn>,
    /// Daily switch set by the user, per calendar event id, for the events still listed.
    daily_choices: HashMap<String, bool>,
    /// The Daily round; `Some` only in [`TurnView::Daily`].
    daily: Option<DailyRound>,
    /// Chips last pushed to the window, to avoid rebuilding them every tick.
    daily_chips: Vec<String>,
    daily_chips_model: Rc<VecModel<SharedString>>,
    /// Random number the finished page turns into its first caption.
    finish_seed: i32,
    /// True while the window is in its countdown shape (small and on top).
    timer_window: bool,
    /// Window geometry before the countdown shape; `None` when it was not changed.
    saved_window: Option<SavedWindow>,
}

impl AppState {
    /// Creates the state on the manual page, with options computed from the current time.
    ///
    /// # Errors
    ///
    /// Returns the [`TimeError`] of [`quarter_options`] when the options cannot
    /// be computed (time arithmetic out of range).
    pub fn new() -> Result<Self, TimeError> {
        let (options, rows) = split_options(quarter_options(&Local::now(), DEFAULT_OPTION_COUNT)?);

        Ok(Self {
            page: Page::ManualPage,
            editor: None,
            editor_model: Rc::new(VecModel::from(Vec::new())),
            slots: None,
            slot_index: None,
            free_time: false,
            options,
            selected: None,
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
            origin: None,
            finish_seed: 0,
            turn_view: TurnView::Plain,
            turn_names: Vec::new(),
            turn_names_model: Rc::new(VecModel::from(Vec::new())),
            turn_selected: 0,
            turn_minutes: DEFAULT_TURN_MINUTES,
            turn: None,
            daily_choices: HashMap::new(),
            daily: None,
            daily_chips: Vec::new(),
            daily_chips_model: Rc::new(VecModel::from(Vec::new())),
            timer_window: false,
            saved_window: None,
        })
    }

    /// Recomputes the selectable end times from the current time.
    ///
    /// On error the error is logged and the previous options and selection are kept. The
    /// picked slot stays picked while its end instant is still offered and is cleared otherwise.
    /// The meeting rows are rebuilt too, so meetings that have just ended disappear.
    pub fn refresh_options(&mut self) {
        self.rebuild_meeting_rows();
        let now = Local::now();
        debug!("refresh_options at {}", now.format("%H:%M:%S"));

        match quarter_options(&now, DEFAULT_OPTION_COUNT) {
            Ok(options) => {
                let (options, rows) = split_options(options);
                debug!("refresh_options refreshed {} options", rows.len());
                self.selected = retain_selection(self.selected, &options);
                self.options = options;
                self.options_model.set_vec(rows);
            }
            Err(err) => error!("refresh_options failed, keeping previous options: {err}"),
        }
    }

    /// Picks the option at `index` on the manual page; the countdown does not start.
    ///
    /// Picking another option replaces the previous pick.
    ///
    /// Edge cases: a negative `index` or one out of range is logged and ignored; the pick does
    /// not change.
    pub fn select_slot(&mut self, index: i32) {
        let Ok(position) = usize::try_from(index) else {
            warn!("select_slot: ignoring negative option index {index}");
            return;
        };
        let Some(end) = self.options.get(position) else {
            warn!("select_slot: ignoring out of range option index {index}");
            return;
        };
        self.selected = Some(*end);
    }

    /// Starts the countdown to the picked option and moves to the timer page.
    ///
    /// The pick is consumed and any running meeting is forgotten. Returns the sound of the first
    /// update (see [`AppState::tick`]).
    ///
    /// Edge cases: without a pick nothing changes. A pick whose end has already passed (the
    /// options were not refreshed yet) is dropped after refreshing the options, and nothing
    /// starts. Both return [`TickSound::None`].
    #[must_use]
    pub fn start_selected(&mut self) -> TickSound {
        let Some(target) = self.selected else {
            return TickSound::None;
        };
        let now = Local::now();
        if target <= now {
            warn!("start_selected: the picked end has passed, refreshing the options");
            self.refresh_options();
            return TickSound::None;
        }

        info!(
            "start_selected: target={} remaining={}s",
            target.format("%H:%M"),
            target.signed_duration_since(now).num_seconds()
        );

        self.selected = None;
        self.origin = Some(TimerOrigin::Manual);
        self.reset_turns(Vec::new());
        self.page = Page::TimerPage;
        self.target = Some(target);
        self.tick()
    }

    /// Starts the countdown to the end of the meeting in row `index` and moves to the timer page.
    ///
    /// Works for a meeting in progress and for one that has not started. With the Daily switch of
    /// the row on and at least one participant the Daily screen starts (see
    /// [`AppState::set_daily`]); otherwise the countdown is plain, with optional speaking turns.
    /// Returns the sound of the first update (see [`AppState::tick`]), which is
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
        let names: Vec<String> = participants(meeting)
            .into_iter()
            .map(|participant| participant.name)
            .collect();
        let use_daily = self.daily_on(meeting) && !names.is_empty();
        self.editor = None;
        self.reset_turns(names.clone());
        if use_daily {
            let round = DailyRound::new(
                shuffled(names, random_u64()),
                target.timestamp_millis(),
                Local::now().timestamp_millis(),
            );
            self.daily = Some(round);
            self.turn_view = TurnView::Daily;
        }
        self.origin = Some(TimerOrigin::Meeting);
        self.page = Page::TimerPage;
        self.target = Some(target);
        self.tick()
    }

    /// Opens the slot editor on the meeting in row `index` and moves to the editor page.
    ///
    /// The editor starts without rows, loading: the caller must fetch the slot plan of the
    /// returned event id for the editor and pass the result to
    /// [`AppState::on_editor_plan_loaded`].
    ///
    /// Edge cases: a negative `index` or one out of range is logged and ignored; the state does
    /// not change and `None` is returned.
    pub fn open_editor(&mut self, index: i32) -> Option<String> {
        let Ok(position) = usize::try_from(index) else {
            warn!("open_editor: ignoring negative meeting index {index}");
            return None;
        };
        let Some(meeting) = self.shown.get(position) else {
            warn!("open_editor: ignoring out of range meeting index {index}");
            return None;
        };

        let editor = SlotEditor::new(meeting.clone());
        let event_id = editor.event_id().to_string();
        self.editor = Some(OpenEditor {
            editor,
            loading: true,
            busy: false,
            notice: "",
        });
        self.page = Page::EditorPage;
        Some(event_id)
    }

    /// Leaves the editor for the meetings page (see [`AppState::show_home`]) and drops its rows;
    /// results still on their way for it are ignored when they arrive.
    pub fn close_editor(&mut self) {
        self.editor = None;
        self.page = self.home_page();
    }

    /// Fills the editor rows from the slot plan fetched for `event_id`.
    ///
    /// On success the rows come from the plan over the nominal meeting duration and the
    /// template id of the plan is remembered. On any error, whatever it is, the rows come from
    /// the equal plan of the participants and a notice says so; the session is never ended by
    /// a plan failure.
    ///
    /// Edge cases: the result is dropped when the editor is not open on `event_id` or its plan
    /// was already applied.
    pub fn on_editor_plan_loaded(
        &mut self,
        event_id: &str,
        result: Result<SlotPlan, BackendError>,
    ) {
        let Some(open) = self.editor_on(event_id) else {
            debug!("on_editor_plan_loaded: editor not open on the meeting, dropping the plan");
            return;
        };
        if !open.loading {
            debug!("on_editor_plan_loaded: plan already applied, dropping it");
            return;
        }
        open.loading = false;
        match result {
            Ok(plan) => open.editor.load_plan(&plan),
            Err(err) => {
                warn!("on_editor_plan_loaded: plan not loaded, using equal slots: {err}");
                open.editor.reset_to_equal();
                open.notice = EDITOR_PLAN_FALLBACK_NOTICE;
            }
        }
    }

    /// Sets the label of editor row `index`; see [`SlotEditor::set_label`].
    ///
    /// Edge cases: ignored without an editor, while its plan loads, or for a negative or out of
    /// range index (logged).
    pub fn editor_set_label(&mut self, index: i32, label: &str) {
        if let (Some(row), Some(editor)) = (row_index(index), self.editable()) {
            editor.set_label(row, label);
        }
    }

    /// Sets the minutes of editor row `index`, clamped; see [`SlotEditor::set_minutes`].
    ///
    /// Edge cases: as for [`AppState::editor_set_label`].
    pub fn editor_set_minutes(&mut self, index: i32, minutes: i32) {
        if let (Some(row), Some(editor)) = (row_index(index), self.editable()) {
            editor.set_minutes(row, minutes);
        }
    }

    /// Includes or excludes the attendee of editor row `index`; see
    /// [`SlotEditor::set_included`].
    ///
    /// Edge cases: as for [`AppState::editor_set_label`]; manual rows stay included.
    pub fn editor_set_included(&mut self, index: i32, included: bool) {
        if let (Some(row), Some(editor)) = (row_index(index), self.editable()) {
            editor.set_included(row, included);
        }
    }

    /// Moves editor row `index` one place up; see [`SlotEditor::move_up`].
    ///
    /// Edge cases: as for [`AppState::editor_set_label`]; the first row does not move.
    pub fn editor_move_up(&mut self, index: i32) {
        if let (Some(row), Some(editor)) = (row_index(index), self.editable()) {
            editor.move_up(row);
        }
    }

    /// Moves editor row `index` one place down; see [`SlotEditor::move_down`].
    ///
    /// Edge cases: as for [`AppState::editor_set_label`]; the last row does not move.
    pub fn editor_move_down(&mut self, index: i32) {
        if let (Some(row), Some(editor)) = (row_index(index), self.editable()) {
            editor.move_down(row);
        }
    }

    /// Removes editor row `index` when it was added by hand; see [`SlotEditor::remove_row`].
    ///
    /// Edge cases: as for [`AppState::editor_set_label`]; attendee rows are kept.
    pub fn editor_remove_row(&mut self, index: i32) {
        if let (Some(row), Some(editor)) = (row_index(index), self.editable()) {
            editor.remove_row(row);
        }
    }

    /// Appends a manual row to the editor; see [`SlotEditor::add_manual_row`].
    ///
    /// Edge cases: ignored without an editor, while its plan loads, or at the row limit.
    pub fn editor_add_row(&mut self) {
        if let Some(editor) = self.editable() {
            editor.add_manual_row();
        }
    }

    /// Replaces the editor rows with the equal plan, keeping the template id; see
    /// [`SlotEditor::reset_to_equal`].
    ///
    /// Edge cases: ignored without an editor or while its plan loads.
    pub fn editor_reset(&mut self) {
        if let Some(editor) = self.editable() {
            editor.reset_to_equal();
        }
    }

    /// Starts the countdown of the edited meeting now, with speaking slots from the editor rows,
    /// and moves to the timer page. No plan is fetched: the slots are built at once.
    ///
    /// The time from now to the meeting end is split by the saved form of the rows (see
    /// [`SlotEditor::start_slots`]) with the same rules as a fetched plan. Returns the sound of
    /// the first update (see [`AppState::tick`]).
    ///
    /// Edge cases: without an editor or while its plan loads nothing happens. When the meeting
    /// is already over the editor stays open with a notice. With fewer than two slots, or slots
    /// too short, the countdown is plain.
    #[must_use]
    pub fn start_from_editor(&mut self) -> TickSound {
        let now = Local::now();
        let Some(open) = self.editor.as_mut() else {
            debug!("start_from_editor: no editor");
            return TickSound::None;
        };
        if open.loading {
            debug!("start_from_editor: plan still loading");
            return TickSound::None;
        }
        if open.editor.meeting().end <= now {
            warn!("start_from_editor: the meeting is over");
            open.notice = MEETING_OVER_NOTICE;
            return TickSound::None;
        }

        let meeting = open.editor.meeting().clone();
        let weighted = open.editor.start_slots();
        info!(
            "start_from_editor: target={}",
            meeting.end.with_timezone(&Local).format("%H:%M")
        );
        self.editor = None;
        self.reset_turns(Vec::new());
        self.origin = Some(TimerOrigin::Meeting);
        self.page = Page::TimerPage;
        self.target = Some(meeting.end.with_timezone(&Local));
        self.turn_view = TurnView::Scheduled;
        let sound = self.tick();
        if let Some(target) = self.target {
            self.install_slots(weighted, &now, &target, &now);
            if self.slots.is_none() {
                self.turn_view = TurnView::Plain;
            }
        }
        sound
    }

    /// Starts saving the editor rows as the template of the meeting title.
    ///
    /// Returns the request the caller must send; its result goes to
    /// [`AppState::on_template_saved`]. Until then the editor is busy and says it is saving.
    ///
    /// Edge cases: returns `None` and changes nothing without an editor, while its plan loads,
    /// or while another save or deletion runs.
    pub fn start_template_save(&mut self) -> Option<TemplateSave> {
        let open = self.idle_editor()?;
        open.busy = true;
        open.notice = TEMPLATE_SAVING_NOTICE;
        Some(TemplateSave {
            event_id: open.editor.event_id().to_string(),
            request: open.editor.save_request(),
        })
    }

    /// Applies the result of the template save started for `event_id`.
    ///
    /// On success the template id is remembered and a notice confirms the save; the return
    /// value is then `true`: the caller must fetch the meetings again so their saved-slots
    /// marker follows. On error the rows are kept: a missing or dead session ends it as a
    /// meetings fetch does (login page), a 400 shows "invalid", anything else a failure notice.
    ///
    /// Edge cases: the result is dropped (`false`) when the editor is no longer open on
    /// `event_id` or no request runs for it.
    #[must_use]
    pub fn on_template_saved(
        &mut self,
        event_id: &str,
        result: Result<Template, BackendError>,
    ) -> bool {
        let Some(open) = self.busy_editor(event_id) else {
            debug!("on_template_saved: editor gone or not saving, dropping the result");
            return false;
        };
        open.busy = false;
        match result {
            Ok(template) => {
                open.editor.set_template_id(Some(template.id));
                open.notice = TEMPLATE_SAVED_NOTICE;
                true
            }
            Err(err) => {
                self.on_template_failed(err, TEMPLATE_INVALID_NOTICE, TEMPLATE_SAVE_FAILED_NOTICE);
                false
            }
        }
    }

    /// Starts deleting the saved template of the edited meeting.
    ///
    /// Returns what the caller must send; its result goes to [`AppState::on_template_deleted`].
    /// Until then the editor is busy and says it is deleting.
    ///
    /// Edge cases: returns `None` and changes nothing without an editor, without a known
    /// template, while its plan loads, or while another save or deletion runs.
    pub fn start_template_delete(&mut self) -> Option<TemplateDelete> {
        let open = self.idle_editor()?;
        let template_id = open.editor.template_id()?.to_string();
        open.busy = true;
        open.notice = TEMPLATE_DELETING_NOTICE;
        Some(TemplateDelete {
            event_id: open.editor.event_id().to_string(),
            template_id,
        })
    }

    /// Applies the result of the template deletion started for `event_id`.
    ///
    /// On success, or when the template was already gone (404 `not_found`), the template id is
    /// forgotten, the rows go back to the equal plan and a notice confirms it; the return value
    /// is then `true`: the caller must fetch the meetings again. Other errors are handled as for
    /// [`AppState::on_template_saved`], with a deletion failure notice.
    ///
    /// Edge cases: the result is dropped (`false`) when the editor is no longer open on
    /// `event_id` or no request runs for it.
    #[must_use]
    pub fn on_template_deleted(
        &mut self,
        event_id: &str,
        result: Result<(), BackendError>,
    ) -> bool {
        let Some(open) = self.busy_editor(event_id) else {
            debug!("on_template_deleted: editor gone or not deleting, dropping the result");
            return false;
        };
        open.busy = false;
        match result {
            Err(err) if !is_not_found(&err) => {
                self.on_template_failed(
                    err,
                    TEMPLATE_DELETE_FAILED_NOTICE,
                    TEMPLATE_DELETE_FAILED_NOTICE,
                );
                false
            }
            _ => {
                open.editor.set_template_id(None);
                open.editor.reset_to_equal();
                open.notice = TEMPLATE_DELETED_NOTICE;
                true
            }
        }
    }

    /// Handles a failed template request: a missing or dead session is lost (login page), a
    /// 400 shows `invalid` on the editor and any other error shows `failed`.
    fn on_template_failed(
        &mut self,
        error: BackendError,
        invalid: &'static str,
        failed: &'static str,
    ) {
        match error {
            BackendError::NotLoggedIn => self.on_session_lost(""),
            BackendError::ReauthRequired => self.on_session_lost(REAUTH_MESSAGE),
            other => {
                warn!("Template request failed: {other}");
                let notice = match other {
                    BackendError::Server { status: 400, .. } => invalid,
                    _ => failed,
                };
                if let Some(open) = self.editor.as_mut() {
                    open.notice = notice;
                }
            }
        }
    }

    /// Open editor on the meeting `event_id`, if any.
    fn editor_on(&mut self, event_id: &str) -> Option<&mut OpenEditor> {
        self.editor
            .as_mut()
            .filter(|open| open.editor.event_id() == event_id)
    }

    /// Open editor on the meeting `event_id` while one of its requests runs, if any.
    fn busy_editor(&mut self, event_id: &str) -> Option<&mut OpenEditor> {
        self.editor_on(event_id).filter(|open| open.busy)
    }

    /// Open editor that may send a request: loaded and with no request running.
    fn idle_editor(&mut self) -> Option<&mut OpenEditor> {
        self.editor
            .as_mut()
            .filter(|open| !open.loading && !open.busy)
    }

    /// Editor whose rows may change: open and loaded.
    fn editable(&mut self) -> Option<&mut SlotEditor> {
        match self.editor.as_mut() {
            Some(open) if !open.loading => Some(&mut open.editor),
            _ => {
                debug!("Editor not open or still loading, ignoring the change");
                None
            }
        }
    }

    /// Splits the time from `started_at` to `target` into speaking slots by the `weighted`
    /// entries, in order, and stores the index of the slot covering `now`.
    ///
    /// Edge cases: with fewer than two slots, or slots that would be too short, the countdown
    /// stays plain (no slots).
    fn install_slots(
        &mut self,
        weighted: Vec<(String, u64)>,
        started_at: &DateTime<Local>,
        target: &DateTime<Local>,
        now: &DateTime<Local>,
    ) {
        let slots = SlotSchedule::new(weighted, started_at, target);
        self.slot_index = slots
            .as_ref()
            .and_then(|schedule| schedule.current(now))
            .map(|view| view.index);
        info!(
            "install_slots: slots={}",
            slots.as_ref().map_or(0, SlotSchedule::len)
        );
        self.slots = slots;
    }

    /// Stores the current slot index and tells whether it moved forward. Without a current
    /// slot or during free time, returns no sound.
    fn update_slot_index(&mut self, now: &DateTime<Local>) -> TickSound {
        if self.free_time {
            return TickSound::None;
        }
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
            TickSound::TurnEnd
        } else {
            TickSound::None
        }
    }

    /// Ends the current speaking slot now and shares the time left among the next ones.
    ///
    /// Ending the last slot enters free time without changing the meeting countdown.
    /// No sound is played for a manual skip.
    ///
    /// Edge cases: nothing happens without a current slot, outside the timer page, or once
    /// free time has started.
    pub fn next_slot(&mut self) {
        if self.page != Page::TimerPage || self.free_time {
            return;
        }
        let now = Local::now();
        let Some(schedule) = self.slots.as_mut() else {
            return;
        };
        let Some(view) = schedule.current(&now) else {
            return;
        };
        if view.next_name.is_none() {
            self.free_time = true;
        } else if schedule.skip(&now) {
            self.slot_index = schedule.current(&now).map(|view| view.index);
        }
    }

    /// Updates the countdown against the current time.
    ///
    /// While the target is ahead, stores the remaining milliseconds. When the target has just
    /// been reached, stops with [`StopType::TimerFinished`] and returns [`TickSound::MeetingEnd`].
    /// When it passed more than the grace period ago (e.g. after a suspend), stops with
    /// [`StopType::UserStopped`] without sound.
    ///
    /// While a turn runs, also checks whether it ran out and returns [`TickSound::TurnEnd`] once;
    /// in the turn settings it keeps the minutes within the time the meeting has left.
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
                if self.turn_view == TurnView::Scheduled {
                    self.update_slot_index(&now)
                } else {
                    self.tick_turns(now.timestamp_millis(), target.timestamp_millis())
                }
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
    /// Moves to the finished page for [`StopType::TimerFinished`], which remembers where the
    /// countdown came from, and to the page the countdown was started from for
    /// [`StopType::UserStopped`] (see [`AppState::show_home`]). Calling it without a running
    /// countdown still refreshes the options and changes the page.
    pub fn stop(&mut self, stop_type: StopType) {
        info!("stop: timer stopped");

        self.refresh_options();
        self.target = None;
        self.timer_ms = 0;
        self.reset_turns(Vec::new());
        match stop_type {
            StopType::TimerFinished => {
                self.finish_seed = random_seed();
                self.page = Page::TimerFinished;
            }
            StopType::UserStopped => self.show_home(),
        }
    }

    /// True when the running countdown is a meeting with at least one participant, so speaking
    /// turns can be used.
    pub fn turns_available(&self) -> bool {
        self.slots.is_none()
            && self.target.is_some()
            && self.origin == Some(TimerOrigin::Meeting)
            && !self.turn_names.is_empty()
    }

    /// Opens the turn settings from the plain countdown view.
    ///
    /// Edge cases: ignored when turns are not available or the view is not the plain one.
    pub fn show_turns(&mut self) {
        if !self.turns_available() || self.turn_view != TurnView::Plain {
            return;
        }
        self.turn_view = TurnView::Settings;
        self.fit_turn_minutes();
    }

    /// Hides the turn settings and goes back to the plain countdown view; the countdown keeps
    /// running.
    ///
    /// Edge cases: ignored outside the turn settings (a running turn is ended with
    /// [`AppState::end_turn`]).
    pub fn hide_turns(&mut self) {
        if self.turn_view == TurnView::Settings {
            self.turn_view = TurnView::Plain;
        }
    }

    /// Chooses the participant at `index` for the next turn.
    ///
    /// Edge cases: a negative or out of range `index` is logged and ignored.
    pub fn select_turner(&mut self, index: i32) {
        let position = usize::try_from(index).unwrap_or(usize::MAX);
        if position >= self.turn_names.len() {
            warn!("select_turner: ignoring participant index {index}");
            return;
        }
        self.turn_selected = position;
    }

    /// Moves the minutes of the next turn by `delta`, between 1 and the longest turn that fits in
    /// the meeting (see [`max_turn_minutes`]).
    ///
    /// Edge cases: ignored outside the turn settings or without a running countdown.
    pub fn adjust_turn_minutes(&mut self, delta: i32) {
        let Some((now_ms, end_ms)) = self.turn_clock() else {
            return;
        };
        if self.turn_view != TurnView::Settings {
            return;
        }
        let moved = i64::from(self.turn_minutes).saturating_add(i64::from(delta));
        let moved = u32::try_from(moved.max(1)).unwrap_or(u32::MAX);
        self.turn_minutes = clamp_turn_minutes(moved, now_ms, end_ms);
    }

    /// Starts a turn of the chosen participant and the chosen minutes, capped at the meeting end.
    ///
    /// Edge cases: ignored outside the turn settings or without a running countdown.
    pub fn start_turn(&mut self) {
        let Some((now_ms, end_ms)) = self.turn_clock() else {
            return;
        };
        if self.turn_view != TurnView::Settings {
            return;
        }
        let Some(name) = self.turn_names.get(self.turn_selected) else {
            return;
        };
        self.turn = Some(SpeakingTurn::new(
            name.clone(),
            self.turn_minutes,
            now_ms,
            end_ms,
        ));
        self.turn_view = TurnView::Running;
    }

    /// Adds `delta_minutes` (negative to remove) to the running turn, never past the meeting end
    /// and never below zero. Adding time re-arms the end-of-turn beep.
    ///
    /// Edge cases: ignored when no turn runs.
    pub fn adjust_turn_time(&mut self, delta_minutes: i32) {
        let Some((now_ms, end_ms)) = self.turn_clock() else {
            return;
        };
        if let Some(turn) = self.turn.as_mut() {
            turn.adjust(i64::from(delta_minutes) * MINUTE_MS, now_ms, end_ms);
        }
    }

    /// Ends the running turn and goes back to the turn settings.
    ///
    /// Edge cases: ignored when no turn runs.
    pub fn end_turn(&mut self) {
        if self.turn_view == TurnView::Running {
            self.turn = None;
            self.turn_view = TurnView::Settings;
            self.fit_turn_minutes();
        }
    }

    /// Sets the Daily switch of the meeting in row `index` and remembers it for that calendar
    /// event while the app runs.
    ///
    /// Edge cases: a negative or out of range `index` is logged and ignored.
    pub fn set_daily(&mut self, index: i32, on: bool) {
        let Ok(position) = usize::try_from(index) else {
            warn!("set_daily: ignoring negative meeting index {index}");
            return;
        };
        let Some(meeting) = self.shown.get(position) else {
            warn!("set_daily: ignoring out of range meeting index {index}");
            return;
        };
        self.daily_choices.insert(meeting.event_id.clone(), on);
        let row = meeting_row(
            meeting,
            &Local::now().fixed_offset(),
            &Local,
            self.daily_choices.get(&meeting.event_id).copied(),
        );
        self.meetings_model.set_row_data(position, row);
    }

    /// True when the Daily switch of `meeting` is on: the user's choice when there is one,
    /// otherwise whether the title looks like a daily.
    fn daily_on(&self, meeting: &Meeting) -> bool {
        match self.daily_choices.get(&meeting.event_id) {
            Some(choice) => *choice,
            None => looks_daily(&meeting.title),
        }
    }

    /// Pauses the Daily speaker, or resumes them.
    ///
    /// Edge cases: ignored outside the Daily screen and when everybody has spoken.
    pub fn toggle_daily_pause(&mut self) {
        let now_ms = Local::now().timestamp_millis();
        if let Some(round) = self.daily.as_mut() {
            round.toggle_pause(now_ms);
        }
    }

    /// Passes the word to the next Daily speaker.
    ///
    /// Edge cases: ignored outside the Daily screen and when everybody has spoken.
    pub fn next_daily_speaker(&mut self) {
        let now_ms = Local::now().timestamp_millis();
        if let Some(round) = self.daily.as_mut() {
            round.next(now_ms);
        }
    }

    /// Adds `delta_minutes` (negative to remove) to the Daily speaker, within what the core round
    /// allows.
    ///
    /// Edge cases: ignored outside the Daily screen and when everybody has spoken.
    pub fn adjust_daily_time(&mut self, delta_minutes: i32) {
        let now_ms = Local::now().timestamp_millis();
        if let Some(round) = self.daily.as_mut() {
            round.adjust(now_ms, i64::from(delta_minutes) * MINUTE_MS);
        }
    }

    /// Forgets every turn setting and installs `names` as the participants of a new countdown:
    /// plain view, first participant chosen, default minutes.
    fn reset_turns(&mut self, names: Vec<String>) {
        self.slots = None;
        self.slot_index = None;
        self.free_time = false;
        let rows: Vec<SharedString> = names.iter().map(|name| name.as_str().into()).collect();
        self.turn_names_model.set_vec(rows);
        self.turn_names = names;
        self.turn_view = TurnView::Plain;
        self.turn_selected = 0;
        self.turn_minutes = DEFAULT_TURN_MINUTES;
        self.turn = None;
        self.daily = None;
    }

    /// Current time and countdown end in milliseconds; `None` without a running countdown.
    fn turn_clock(&self) -> Option<(i64, i64)> {
        let target = self.target?;
        Some((Local::now().timestamp_millis(), target.timestamp_millis()))
    }

    /// Lowers the minutes of the next turn to what the meeting still allows.
    fn fit_turn_minutes(&mut self) {
        if let Some((now_ms, end_ms)) = self.turn_clock() {
            self.turn_minutes = clamp_turn_minutes(self.turn_minutes, now_ms, end_ms);
        }
    }

    /// Turn part of a tick: keeps the settings minutes valid, or checks whether the running turn
    /// ran out.
    fn tick_turns(&mut self, now_ms: i64, end_ms: i64) -> TickSound {
        match self.turn_view {
            TurnView::Plain | TurnView::Scheduled => TickSound::None,
            TurnView::Settings => {
                self.turn_minutes = clamp_turn_minutes(self.turn_minutes, now_ms, end_ms);
                TickSound::None
            }
            TurnView::Daily => {
                let Some(round) = self.daily.as_mut() else {
                    return TickSound::None;
                };
                if round.tick(now_ms) {
                    TickSound::TurnEnd
                } else {
                    TickSound::None
                }
            }
            TurnView::Running => {
                let Some(turn) = self.turn.as_mut() else {
                    return TickSound::None;
                };
                if turn.tick(now_ms, end_ms) {
                    TickSound::TurnEnd
                } else {
                    TickSound::None
                }
            }
        }
    }

    /// Moves to the manual page without touching the countdown or the options.
    pub fn show_manual_page(&mut self) {
        self.page = Page::ManualPage;
    }

    /// Moves to the meetings page, keeping the meetings already loaded. Without an account it moves to
    /// the manual page instead.
    pub fn show_meetings_page(&mut self) {
        self.page = self.home_page();
    }

    /// Leaves the countdown screens for the page the countdown was started from: the manual page
    /// after a manual countdown, the meetings page after a meeting countdown. The origin is
    /// forgotten.
    ///
    /// Edge cases: after a meeting countdown without an account it moves to the manual page.
    /// Without any origin it behaves like [`AppState::show_meetings_page`].
    pub fn show_home(&mut self) {
        self.page = match self.origin.take() {
            Some(TimerOrigin::Manual) => Page::ManualPage,
            Some(TimerOrigin::Meeting) | None => self.home_page(),
        };
    }

    /// Replaces the meetings with those of a successful fetch and shows them.
    pub fn on_meetings_loaded(&mut self, meetings: Vec<Meeting>) {
        self.daily_choices
            .retain(|id, _| meetings.iter().any(|meeting| meeting.event_id == *id));
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
        self.origin = None;
        self.reset_turns(Vec::new());
        self.on_login_required(status);
    }

    /// Drops the loaded meetings, so the next visit to the meetings page starts loading.
    fn forget_meetings(&mut self) {
        self.editor = None;
        self.meetings.clear();
        self.shown.clear();
        self.meetings_model.set_vec(Vec::new());
        self.meetings_loaded = false;
        self.meetings_status = MeetingsStatus::Loading;
    }

    /// Pushes the turn view, the settings and the running turn to `window`.
    fn render_turns(&mut self, window: &MainWindow) {
        let (now_ms, end_ms) = self.turn_clock().unwrap_or((0, 0));
        window.set_turn_available(self.turns_available());
        window.set_turn_view(self.turn_view);
        window.set_turn_selected(i32::try_from(self.turn_selected).unwrap_or(0));
        window.set_turn_minutes(i32::try_from(self.turn_minutes).unwrap_or(i32::MAX));
        let max = max_turn_minutes(now_ms, end_ms);
        window.set_turn_max_minutes(i32::try_from(max).unwrap_or(i32::MAX));
        let (name, left, can_add) = match &self.turn {
            Some(turn) => (
                turn.name(),
                turn.left_ms(now_ms, end_ms),
                turn.can_add(end_ms),
            ),
            None => ("", 0, false),
        };
        window.set_turn_speaker(name.into());
        window.set_turn_left(left);
        window.set_turn_can_add(can_add);
        self.render_daily(window, now_ms);
    }

    /// Pushes the Daily screen to `window`; without a round everything is empty.
    fn render_daily(&mut self, window: &MainWindow, now_ms: i64) {
        let view = match &self.daily {
            Some(round) => daily_view(round, now_ms),
            None => DailyView {
                name: String::new(),
                short_name: String::new(),
                left_ms: 0,
                paused: false,
                finished: false,
                can_add: false,
                can_remove: false,
                chips: Vec::new(),
                remaining: String::new(),
            },
        };
        window.set_daily_name(view.name.as_str().into());
        window.set_daily_short_name(view.short_name.as_str().into());
        window.set_daily_left(view.left_ms);
        window.set_daily_paused(view.paused);
        window.set_daily_finished(view.finished);
        window.set_daily_can_add(view.can_add);
        window.set_daily_can_remove(view.can_remove);
        window.set_daily_remaining(view.remaining.as_str().into());
        if self.daily_chips != view.chips {
            let rows: Vec<SharedString> =
                view.chips.iter().map(|chip| chip.as_str().into()).collect();
            self.daily_chips_model.set_vec(rows);
            self.daily_chips = view.chips;
        }
    }

    /// Shrinks the window to the countdown shape when the countdown screens open and restores it
    /// when they close.
    ///
    /// The previous size and on-top state are saved on entering and put back on leaving, so a
    /// change of the on-top state during the countdown does not stick. A fullscreen window is
    /// neither resized nor made on top. The resize is queued on the event loop so the new
    /// minimum size is in force first.
    fn sync_window(&mut self, window: &MainWindow) {
        let wants_timer = shows_countdown(self.page);
        match window_step(self.timer_window, wants_timer) {
            WindowStep::Keep => {}
            WindowStep::Enter => {
                self.timer_window = true;
                window.set_timer_mode(true);
                if window.window().is_fullscreen() {
                    self.saved_window = None;
                    return;
                }
                let size = window.window().size();
                self.saved_window = Some(SavedWindow {
                    size: size.to_logical(window.window().scale_factor()),
                    on_top: window.get_on_top(),
                });
                window.set_on_top(true);
                resize_later(window, TIMER_WINDOW_SIZE);
            }
            WindowStep::Leave => {
                self.timer_window = false;
                window.set_timer_mode(false);
                if let Some(saved) = self.saved_window.take() {
                    window.set_on_top(saved.on_top);
                    resize_later(window, saved.size);
                }
            }
        }
    }

    /// Position of the picked slot among the options, or -1 when none is picked.
    fn selected_index(&self) -> i32 {
        let Some(selected) = self.selected else {
            return -1;
        };
        match self.options.iter().position(|end| *end == selected) {
            Some(position) => i32::try_from(position).unwrap_or(-1),
            None => -1,
        }
    }

    /// True when an account is signed in.
    pub fn is_signed_in(&self) -> bool {
        self.account.is_some()
    }

    /// Page a finished or stopped countdown returns to: the meetings page when signed in (marked
    /// as loading only when no meetings were loaded yet), the manual page otherwise.
    fn home_page(&mut self) -> Page {
        if self.account.is_some() {
            if !self.meetings_loaded {
                self.meetings_status = MeetingsStatus::Loading;
            }
            Page::MeetingsPage
        } else {
            Page::ManualPage
        }
    }

    /// Rebuilds the shown meetings and their rows from the received ones and the current time.
    fn rebuild_meeting_rows(&mut self) {
        let now = Local::now();
        let shown = visible_meetings(&self.meetings, &now.fixed_offset());
        let mut rows = Vec::with_capacity(shown.len());
        for meeting in &shown {
            let choice = self.daily_choices.get(&meeting.event_id).copied();
            rows.push(meeting_row(meeting, &now.fixed_offset(), &Local, choice));
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

    /// Moves to the manual page without an account, showing `notice` (empty for none)..
    pub fn on_local_mode(&mut self, notice: &'static str) {
        self.account = None;
        self.notice = notice;
        self.login_waiting = false;
        self.page = Page::ManualPage;
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
    /// manual page.
    pub fn on_logout_started(&mut self) {
        self.forget_meetings();
        self.account = None;
        self.notice = LOGOUT_NOTICE;
        self.page = Page::ManualPage;
    }

    /// Stops any countdown, forgets the account and moves to the idle login page.
    pub fn on_logged_out(&mut self) {
        self.target = None;
        self.timer_ms = 0;
        self.origin = None;
        self.reset_turns(Vec::new());
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

    /// Binds the shared editor rows model to `window`; call once after creating the window.
    pub fn bind_editor_rows(&self, window: &MainWindow) {
        window.set_editor_rows(ModelRc::from(self.editor_model.clone()));
    }

    /// Pushes the editor header, rows, footer and status to `window`. Without an editor the
    /// rows are emptied and the other editor properties are left as they are (not shown).
    fn render_editor(&self, window: &MainWindow) {
        let Some(open) = self.editor.as_ref() else {
            sync_editor_rows(&self.editor_model, &[]);
            return;
        };
        let editor = &open.editor;
        let meeting = editor.meeting();
        sync_editor_rows(&self.editor_model, editor.rows());
        window.set_editor_title(meeting.title.as_str().into());
        window.set_editor_time_range(time_range(meeting, &Local).into());
        window.set_editor_duration(format!("Durata {} min", editor.duration_minutes()).into());
        window.set_editor_attendees_warning(meeting.attendees_omitted);
        window.set_editor_loading(open.loading);
        window.set_editor_busy(open.busy);
        window.set_editor_notice(open.notice.into());
        let unassigned = if open.loading {
            String::new()
        } else if editor.is_over() {
            editor.unassigned_text()
        } else {
            let duration = editor.duration_minutes();
            let assigned = duration - editor.unassigned_minutes();
            format!("{assigned} di {duration} minuti assegnati")
        };
        window.set_editor_unassigned(unassigned.into());
        window.set_editor_over(editor.is_over());
        window.set_editor_has_template(editor.template_id().is_some());
    }

    /// Binds the shared participant names model to `window`; call once after creating the window.
    pub fn bind_turns(&self, window: &MainWindow) {
        window.set_turn_names(ModelRc::from(self.turn_names_model.clone()));
        window.set_daily_chips(ModelRc::from(self.daily_chips_model.clone()));
    }

    /// Pushes page, running state, timer, account and status lines to `window`, and fits the
    /// window shape to the page (see [`window_step`]).
    pub fn render(&mut self, window: &MainWindow) {
        self.sync_window(window);
        window.set_page(self.page);
        window.set_running_state(self.target.is_some());
        window.set_timer_time(self.timer_ms);
        window.set_current_time(Local::now().format("%H:%M").to_string().into());
        let end = self
            .target
            .map_or_else(String::new, |target| target.format("%H:%M").to_string());
        window.set_timer_end(end.into());
        window.set_finish_seed(self.finish_seed);
        self.render_turns(window);
        self.render_editor(window);
        let view = self
            .slots
            .as_ref()
            .and_then(|schedule| schedule.current(&Local::now()));
        window.set_slot_name(if self.free_time {
            FREE_TIME_LABEL.into()
        } else {
            view.as_ref().map_or("", |view| view.name.as_str()).into()
        });
        window.set_slot_time(view.as_ref().map_or(0, |view| view.remaining_ms));
        window.set_next_slot_name(if self.free_time {
            "".into()
        } else {
            view.as_ref()
                .and_then(|view| view.next_name.clone())
                .unwrap_or_default()
                .into()
        });
        window.set_slot_free_time(self.free_time);
        window.set_selected_slot(self.selected_index());
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

/// Converts a negative row index from the UI into `None` (logged).
fn row_index(index: i32) -> Option<usize> {
    match usize::try_from(index) {
        Ok(row) => Some(row),
        Err(_) => {
            debug!("Ignoring negative editor row {index}");
            None
        }
    }
}

/// True when `error` says the requested resource does not exist (404 `not_found`).
fn is_not_found(error: &BackendError) -> bool {
    matches!(
        error,
        BackendError::Server { status: 404, code, .. } if code == error_codes::NOT_FOUND
    )
}

/// Builds the UI row of an editor entry. Minutes above `i32::MAX` (never produced by the
/// editor) are shown as `i32::MAX`.
fn editor_row(entry: &EditorEntry) -> EditorRow {
    EditorRow {
        label: entry.label.as_str().into(),
        minutes: i32::try_from(entry.minutes).unwrap_or(i32::MAX),
        included: entry.included,
        attendee: entry.kind == EntryKind::Attendee,
        email: entry.email.as_deref().unwrap_or_default().into(),
    }
}

/// Makes `model` show `entries`. When the row count is the same only the rows that differ are
/// replaced, so a row being edited keeps its focus; otherwise the whole model is replaced.
fn sync_editor_rows(model: &VecModel<EditorRow>, entries: &[EditorEntry]) {
    if model.row_count() != entries.len() {
        model.set_vec(entries.iter().map(editor_row).collect::<Vec<_>>());
        return;
    }
    for (position, entry) in entries.iter().enumerate() {
        let row = editor_row(entry);
        if model.row_data(position).as_ref() != Some(&row) {
            model.set_row_data(position, row);
        }
    }
}

/// Turns plan entries into `(label, weight)` slots: entries with weight 0 are skipped, the order
/// is kept. Empty input gives an empty list.
pub(crate) fn weighted_slots(entries: &[PlanEntry]) -> Vec<(String, u64)> {
    let mut weighted = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.weight > 0 {
            weighted.push((entry.label.clone(), entry.weight));
        }
    }
    weighted
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

/// Longest value `minutes` can take for a turn started at `now_ms` in a meeting ending at
/// `end_ms`: it is lowered to [`max_turn_minutes`] and never goes below 1.
fn clamp_turn_minutes(minutes: u32, now_ms: i64, end_ms: i64) -> u32 {
    minutes.min(max_turn_minutes(now_ms, end_ms)).max(1)
}

/// True for the pages that show a countdown: the running one and the finished one.
fn shows_countdown(page: Page) -> bool {
    matches!(page, Page::TimerPage | Page::TimerFinished)
}

/// Decides how the window shape must change when the shown page moves from `in_timer_shape` to
/// one that does (`wants_timer`) or does not show a countdown.
fn window_step(in_timer_shape: bool, wants_timer: bool) -> WindowStep {
    match (in_timer_shape, wants_timer) {
        (false, true) => WindowStep::Enter,
        (true, false) => WindowStep::Leave,
        _ => WindowStep::Keep,
    }
}

/// Queues a resize of `window` to `size` on the event loop.
fn resize_later(window: &MainWindow, size: LogicalSize) {
    let weak = window.as_weak();
    if let Err(err) = slint::invoke_from_event_loop(move || {
        if let Some(window) = weak.upgrade() {
            window.window().set_size(size);
        }
    }) {
        warn!("resize_later: cannot queue the resize: {err}");
    }
}

/// Random non-negative number for the first caption of the finished page; 0 when the system
/// source of randomness fails.
fn random_seed() -> i32 {
    let mut bytes = [0u8; 2];
    if getrandom::fill(&mut bytes).is_err() {
        return 0;
    }
    i32::from(u16::from_le_bytes(bytes))
}

/// Random seed for the Daily speaking order; 0 when the system source of randomness fails (the
/// order is then the same every time, but still a valid permutation).
fn random_u64() -> u64 {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return 0;
    }
    u64::from_le_bytes(bytes)
}

/// What the Daily screen shows for a round at one instant.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DailyView {
    /// Full name of the speaker, or the finished text.
    name: String,
    /// Speaker name shortened for the compact layout.
    short_name: String,
    left_ms: i64,
    paused: bool,
    finished: bool,
    can_add: bool,
    can_remove: bool,
    /// One "Name ~mm:ss" per upcoming person, in speaking order.
    chips: Vec<String>,
    /// Compact line under the speaker bar: "3 persone ~ 03:08", "Ultimo turno" or empty.
    remaining: String,
}

/// Builds the [`DailyView`] of `round` at `now_ms`.
///
/// Edge cases: a finished round has the finished name, no chips, no remaining line and nothing
/// enabled. The estimate is rounded to the nearest second.
fn daily_view(round: &DailyRound, now_ms: i64) -> DailyView {
    let share = format_minutes_seconds(round.share_ms());
    let mut chips = Vec::with_capacity(round.upcoming().len());
    for name in round.upcoming() {
        chips.push(format!("{name} ~{share}"));
    }
    let remaining = match round.upcoming().len() {
        0 if round.is_finished() => String::new(),
        0 => "Ultimo turno".to_string(),
        1 => format!("1 persona ~ {share}"),
        count => format!("{count} persone ~ {share}"),
    };
    let (name, short_name) = match round.current() {
        Some(name) => (name.to_string(), abbreviate_name(name, 1)),
        None => (
            DAILY_FINISHED_NAME.to_string(),
            DAILY_FINISHED_NAME.to_string(),
        ),
    };
    DailyView {
        name,
        short_name,
        left_ms: round.current_left_ms(now_ms),
        paused: round.is_paused(),
        finished: round.is_finished(),
        can_add: round.can_add(now_ms),
        can_remove: round.can_remove(now_ms),
        chips,
        remaining,
    }
}

/// Formats milliseconds as `MM:SS` rounded to the nearest second; minutes can pass 59.
fn format_minutes_seconds(ms: i64) -> String {
    let total = (ms.max(0) + 500) / 1000;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Builds the row of `meeting`: times in `tz` as `HH:MM–HH:MM`, the title, the meta line (see
/// [`meeting_meta`]) and the Daily switch: `daily_choice` when the user set it, otherwise on for
/// a title that looks like a daily. `daily_auto` tells that the title looks like a daily.
fn meeting_row<Tz: TimeZone>(
    meeting: &Meeting,
    now: &DateTime<FixedOffset>,
    tz: &Tz,
    daily_choice: Option<bool>,
) -> MeetingRow
where
    Tz::Offset: Display,
{
    let daily_auto = looks_daily(&meeting.title);
    MeetingRow {
        daily_on: daily_choice.unwrap_or(daily_auto),
        daily_auto,
        time_range: time_range(meeting, tz).into(),
        title: meeting.title.as_str().into(),
        meta: meeting_meta(meeting, now).into(),
        in_progress: is_in_progress(meeting, now),
    }
}

/// Meta line of a meeting row: "In corso" or the time to the start, then the participant count.
///
/// The count is the length of [`participants`], so declined attendees are not counted and the
/// attendees the calendar omitted are not known. One participant is singular, any other count
/// (zero included) is plural.
fn meeting_meta(meeting: &Meeting, now: &DateTime<FixedOffset>) -> String {
    let count = participants(meeting).len();
    let people = if count == 1 {
        "1 partecipante accettato".to_string()
    } else {
        format!("{count} partecipanti accettati")
    };

    if is_in_progress(meeting, now) {
        return format!("In corso · {people}");
    }
    let seconds = meeting.start.signed_duration_since(*now).num_seconds();
    let minutes = seconds.saturating_add(59).div_euclid(60).max(1);
    format!("{} · {people}", starts_in_text(minutes))
}

/// Text for a start `minutes` away: "Tra 52 min", "Tra 1 h", "Tra 1 h 20 min".
///
/// Hours are used from 60 minutes on; a whole number of hours omits the minutes.
fn starts_in_text(minutes: i64) -> String {
    if minutes < 60 {
        return format!("Tra {minutes} min");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest == 0 {
        format!("Tra {hours} h")
    } else {
        format!("Tra {hours} h {rest} min")
    }
}

/// Keeps `selected` only while `options` still offers that end instant.
fn retain_selection<Tz: TimeZone>(
    selected: Option<DateTime<Tz>>,
    options: &[DateTime<Tz>],
) -> Option<DateTime<Tz>> {
    let selected = selected?;
    if options.contains(&selected) {
        Some(selected)
    } else {
        None
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

    /// State on the manual page, as at startup without a backend.
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

        let row = meeting_row(&meeting, &at(6, 0), &plus_two, None);

        assert_eq!(row.time_range, "09:05–09:20");
        assert_eq!(row.title, "Daily");
    }

    #[test]
    fn row_marks_the_meeting_in_progress() {
        let meeting = meeting("Daily", at(9, 0), at(10, 0));

        assert!(meeting_row(&meeting, &at(9, 0), &Utc, None).in_progress);
        assert!(!meeting_row(&meeting, &at(8, 59), &Utc, None).in_progress);
        assert!(!meeting_row(&meeting, &at(10, 0), &Utc, None).in_progress);
    }

    fn meta(meeting: &Meeting, now: DateTime<FixedOffset>) -> String {
        meeting_meta(meeting, &now)
    }

    #[test]
    fn meta_of_a_running_meeting_says_in_corso() {
        let mut meeting = meeting("Review", at(9, 0), at(10, 0));
        meeting.attendees = attendees(3);

        assert_eq!(
            meta(&meeting, at(9, 30)),
            "In corso · 3 partecipanti accettati"
        );
    }

    #[test]
    fn meta_of_a_future_meeting_counts_minutes() {
        let mut meeting = meeting("Retro", at(9, 52), at(10, 30));
        meeting.attendees = attendees(7);

        assert_eq!(
            meta(&meeting, at(9, 0)),
            "Tra 52 min · 7 partecipanti accettati"
        );
    }

    #[test]
    fn meta_rounds_a_partial_minute_up() {
        let meeting = meeting("Retro", at(9, 0), at(10, 0));
        let now = at(8, 57) + chrono::Duration::seconds(30);

        assert!(meta(&meeting, now).starts_with("Tra 3 min"));
    }

    #[test]
    fn meta_switches_to_hours_from_sixty_minutes() {
        let meeting = meeting("Retro", at(10, 20), at(11, 0));

        assert!(meta(&meeting, at(9, 0)).starts_with("Tra 1 h 20 min · "));
        assert!(meta(&meeting, at(9, 20)).starts_with("Tra 1 h · "));
    }

    #[test]
    fn meta_uses_the_singular_for_one_participant() {
        let mut meeting = meeting("1:1", at(9, 0), at(10, 0));
        meeting.attendees = attendees(1);

        assert_eq!(
            meta(&meeting, at(9, 10)),
            "In corso · 1 partecipante accettato"
        );
    }

    #[test]
    fn meta_uses_the_plural_for_no_participants() {
        let meeting = meeting("Solo", at(9, 0), at(10, 0));

        assert!(meta(&meeting, at(8, 59)).ends_with("0 partecipanti accettati"));
    }

    #[test]
    fn meta_does_not_count_declined_attendees() {
        let mut meeting = meeting("Review", at(9, 0), at(10, 0));
        meeting.attendees = attendees(2);
        meeting.attendees[0].response = ResponseStatus::Declined;

        assert!(meta(&meeting, at(9, 10)).ends_with("1 partecipante accettato"));
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
    fn a_future_meeting_can_be_started() {
        let mut state = state_with_meetings();
        assert!(!is_in_progress(
            &state.shown[1],
            &Local::now().fixed_offset()
        ));
        let end = state.shown[1].end;

        let sound = state.select_meeting(1);

        assert_eq!(sound, TickSound::None);
        assert_eq!(state.page, Page::TimerPage);
        assert_eq!(state.target, Some(end.with_timezone(&Local)));
    }

    #[test]
    fn selecting_a_slot_does_not_start_the_timer() {
        let mut state = sample_state();

        state.select_slot(2);

        assert_eq!(state.page, Page::ManualPage);
        assert_eq!(state.target, None);
        assert_eq!(state.selected, Some(state.options[2]));
        assert_eq!(state.selected_index(), 2);
    }

    #[test]
    fn selecting_another_slot_replaces_the_pick() {
        let mut state = sample_state();
        state.select_slot(2);

        state.select_slot(5);

        assert_eq!(state.selected_index(), 5);
    }

    #[test]
    fn selecting_an_invalid_slot_keeps_the_pick() {
        let mut state = sample_state();
        state.select_slot(1);

        state.select_slot(-1);
        state.select_slot(99);

        assert_eq!(state.selected_index(), 1);
    }

    #[test]
    fn starting_without_a_pick_does_nothing() {
        let mut state = sample_state();

        let sound = state.start_selected();

        assert_eq!(sound, TickSound::None);
        assert_eq!(state.page, Page::ManualPage);
        assert_eq!(state.target, None);
    }

    #[test]
    fn starting_uses_the_picked_end() {
        let mut state = sample_state();
        state.select_slot(3);
        let end = state.options[3];

        let sound = state.start_selected();

        assert_eq!(sound, TickSound::None);
        assert_eq!(state.page, Page::TimerPage);
        assert_eq!(state.target, Some(end));
        assert!(state.timer_ms > 0);
        assert_eq!(state.selected, None);
    }

    #[test]
    fn starting_a_pick_that_has_passed_drops_it() {
        let mut state = sample_state();
        state.selected = Some(Local::now() - chrono::Duration::minutes(1));

        let sound = state.start_selected();

        assert_eq!(sound, TickSound::None);
        assert_eq!(state.page, Page::ManualPage);
        assert_eq!(state.target, None);
        assert_eq!(state.selected, None);
    }

    #[test]
    fn refresh_keeps_a_pick_that_is_still_offered() {
        let mut state = sample_state();
        state.select_slot(4);
        let end = state.options[4];

        state.refresh_options();

        assert_eq!(state.selected, Some(end));
        assert_eq!(state.selected_index(), 4);
    }

    #[test]
    fn refresh_clears_a_pick_that_is_no_longer_offered() {
        let mut state = sample_state();
        state.selected = Some(Local::now() - chrono::Duration::minutes(30));

        state.refresh_options();

        assert_eq!(state.selected, None);
        assert_eq!(state.selected_index(), -1);
    }

    #[test]
    fn retain_selection_follows_the_offered_options() {
        let options = vec![utc(9, 15), utc(9, 30)];

        assert_eq!(
            retain_selection(Some(utc(9, 30)), &options),
            Some(utc(9, 30))
        );
        assert_eq!(retain_selection(Some(utc(9, 0)), &options), None);
        assert_eq!(retain_selection(None, &options), None);
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
        state.page = Page::ManualPage;

        state.show_meetings_page();

        assert_eq!(state.meetings_status, MeetingsStatus::Loading);
    }

    #[test]
    fn entering_meetings_with_data_keeps_the_list() {
        let mut state = state_with_meetings();
        state.page = Page::ManualPage;

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
    fn stopping_a_manual_timer_returns_to_the_manual_page_even_when_signed_in() {
        let mut state = state_with_meetings();
        state.select_slot(1);
        let _ = state.start_selected();

        state.stop(StopType::UserStopped);

        assert_eq!(state.page, Page::ManualPage);
        assert_eq!(state.origin, None);
    }

    #[test]
    fn stopping_a_meeting_timer_returns_to_the_meetings_page() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(1);

        state.stop(StopType::UserStopped);

        assert_eq!(state.page, Page::MeetingsPage);
        assert_eq!(state.origin, None);
    }

    #[test]
    fn finishing_keeps_the_origin_until_leaving_the_finished_page() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);

        state.stop(StopType::TimerFinished);

        assert_eq!(state.page, Page::TimerFinished);
        assert_eq!(state.origin, Some(TimerOrigin::Meeting));
        assert_eq!(state.target, None);
        assert!(state.finish_seed >= 0);
    }

    #[test]
    fn leaving_the_finished_page_after_a_manual_timer_goes_to_the_manual_page() {
        let mut state = state_with_meetings();
        state.select_slot(0);
        let _ = state.start_selected();
        state.stop(StopType::TimerFinished);

        state.show_home();

        assert_eq!(state.page, Page::ManualPage);
        assert_eq!(state.origin, None);
    }

    #[test]
    fn leaving_the_finished_page_after_a_meeting_timer_goes_to_the_meetings_page() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);
        state.stop(StopType::TimerFinished);

        state.show_home();

        assert_eq!(state.page, Page::MeetingsPage);
        assert_eq!(state.origin, None);
    }

    #[test]
    fn a_meeting_timer_without_account_goes_to_the_manual_page() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);
        state.account = None;

        state.stop(StopType::UserStopped);

        assert_eq!(state.page, Page::ManualPage);
    }

    #[test]
    fn session_lost_forgets_the_origin() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);

        state.on_session_lost("");

        assert_eq!(state.origin, None);
    }

    #[test]
    fn only_countdown_pages_use_the_timer_window() {
        assert!(shows_countdown(Page::TimerPage));
        assert!(shows_countdown(Page::TimerFinished));
        assert!(!shows_countdown(Page::ManualPage));
        assert!(!shows_countdown(Page::MeetingsPage));
        assert!(!shows_countdown(Page::LoginPage));
        assert!(!shows_countdown(Page::LoadingPage));
    }

    #[test]
    fn window_shape_changes_only_when_crossing_into_or_out_of_the_countdown() {
        assert_eq!(window_step(false, true), WindowStep::Enter);
        assert_eq!(window_step(true, false), WindowStep::Leave);
        assert_eq!(window_step(true, true), WindowStep::Keep);
        assert_eq!(window_step(false, false), WindowStep::Keep);
    }

    #[test]
    fn random_seed_is_never_negative() {
        assert!(random_seed() >= 0);
    }

    fn person(name: &str) -> Attendee {
        Attendee {
            email: format!("{}@x.it", name.to_lowercase()),
            name: name.to_string(),
            is_self: false,
            optional: false,
            response: ResponseStatus::Accepted,
        }
    }

    /// State whose meeting timer runs for a meeting of Zoe, Ann and Bob that ends in 10 minutes.
    fn state_with_turns() -> AppState {
        let now = Local::now().fixed_offset();
        let mut state = sample_state();
        state.on_signed_in(&me(None));
        let mut talk = meeting(
            "Planning",
            now - chrono::Duration::minutes(1),
            now + chrono::Duration::minutes(10),
        );
        talk.attendees = vec![person("Zoe"), person("Ann"), person("Bob")];
        state.on_meetings_loaded(vec![talk]);
        let _ = state.select_meeting(0);
        state
    }

    fn ends_in(state: &mut AppState, minutes: i64, seconds: i64) {
        state.target = Some(
            Local::now() + chrono::Duration::minutes(minutes) + chrono::Duration::seconds(seconds),
        );
    }

    #[test]
    fn turns_are_not_available_for_a_manual_timer() {
        let mut state = sample_state();
        state.select_slot(1);
        let _ = state.start_selected();

        assert!(!state.turns_available());
        state.show_turns();
        assert_eq!(state.turn_view, TurnView::Plain);
    }

    #[test]
    fn turns_are_not_available_without_participants() {
        let mut state = state_with_meetings();
        let _ = state.select_meeting(0);

        assert!(!state.turns_available());
        state.show_turns();
        assert_eq!(state.turn_view, TurnView::Plain);
    }

    #[test]
    fn turns_are_available_for_a_meeting_with_participants() {
        let state = state_with_turns();

        assert!(state.turns_available());
        assert_eq!(state.turn_names, vec!["Ann", "Bob", "Zoe"]);
        assert_eq!(state.turn_names_model.row_count(), 3);
    }

    #[test]
    fn a_new_meeting_timer_starts_plain_with_defaults() {
        let mut state = state_with_turns();
        state.show_turns();
        state.select_turner(2);
        state.adjust_turn_minutes(2);
        state.start_turn();
        state.stop(StopType::UserStopped);

        let _ = state.select_meeting(0);

        assert_eq!(state.turn_view, TurnView::Plain);
        assert_eq!(state.turn_selected, 0);
        assert_eq!(state.turn_minutes, DEFAULT_TURN_MINUTES);
        assert!(state.turn.is_none());
    }

    #[test]
    fn showing_the_settings_keeps_the_defaults_and_hiding_goes_back_to_plain() {
        let mut state = state_with_turns();

        state.show_turns();

        assert_eq!(state.turn_view, TurnView::Settings);
        assert_eq!(state.turn_selected, 0);
        assert_eq!(state.turn_minutes, 5);

        state.hide_turns();

        assert_eq!(state.turn_view, TurnView::Plain);
        assert!(state.target.is_some());
    }

    #[test]
    fn the_chosen_participant_is_remembered_across_hiding() {
        let mut state = state_with_turns();
        state.show_turns();
        state.select_turner(2);

        state.hide_turns();
        state.show_turns();

        assert_eq!(state.turn_selected, 2);
    }

    #[test]
    fn choosing_an_invalid_participant_is_ignored() {
        let mut state = state_with_turns();
        state.show_turns();
        state.select_turner(1);

        state.select_turner(-1);
        state.select_turner(3);

        assert_eq!(state.turn_selected, 1);
    }

    #[test]
    fn minutes_stay_between_one_and_the_time_left() {
        let mut state = state_with_turns();
        ends_in(&mut state, 7, 30);
        state.show_turns();

        state.adjust_turn_minutes(10);
        assert_eq!(state.turn_minutes, 7);

        state.adjust_turn_minutes(-10);
        assert_eq!(state.turn_minutes, 1);
    }

    #[test]
    fn showing_the_settings_lowers_the_default_to_the_time_left() {
        let mut state = state_with_turns();
        ends_in(&mut state, 3, 30);

        state.show_turns();

        assert_eq!(state.turn_minutes, 3);
    }

    #[test]
    fn minutes_follow_the_meeting_running_out() {
        let mut state = state_with_turns();
        state.show_turns();
        let now = Local::now().timestamp_millis();

        let sound = state.tick_turns(now, now + 2 * MINUTE_MS + 10_000);

        assert_eq!(sound, TickSound::None);
        assert_eq!(state.turn_minutes, 2);
    }

    #[test]
    fn minutes_cannot_change_outside_the_settings() {
        let mut state = state_with_turns();

        state.adjust_turn_minutes(1);

        assert_eq!(state.turn_minutes, 5);
    }

    #[test]
    fn starting_a_turn_uses_the_chosen_participant_and_minutes() {
        let mut state = state_with_turns();
        state.show_turns();
        state.select_turner(1);
        state.adjust_turn_minutes(-2);

        state.start_turn();

        assert_eq!(state.turn_view, TurnView::Running);
        let turn = state.turn.as_ref().unwrap();
        assert_eq!(turn.name(), "Bob");
        let end_ms = state.target.unwrap().timestamp_millis();
        let left = turn.left_ms(Local::now().timestamp_millis(), end_ms);
        assert!(left > 2 * MINUTE_MS + 50_000 && left <= 3 * MINUTE_MS);
    }

    #[test]
    fn a_turn_cannot_start_from_the_plain_view() {
        let mut state = state_with_turns();

        state.start_turn();

        assert_eq!(state.turn_view, TurnView::Plain);
        assert!(state.turn.is_none());
    }

    #[test]
    fn turn_time_moves_a_minute_at_a_time_within_bounds() {
        let mut state = state_with_turns();
        state.show_turns();
        state.adjust_turn_minutes(-4);
        state.start_turn();
        let end_ms = state.target.unwrap().timestamp_millis();

        state.adjust_turn_time(-1);
        let now_ms = Local::now().timestamp_millis();
        assert!(state.turn.as_ref().unwrap().left_ms(now_ms, end_ms) < 1_000);

        state.adjust_turn_time(60);
        let now_ms = Local::now().timestamp_millis();
        let left = state.turn.as_ref().unwrap().left_ms(now_ms, end_ms);
        assert!(left <= end_ms - now_ms);
        assert!(!state.turn.as_ref().unwrap().can_add(end_ms));
    }

    #[test]
    fn the_beep_plays_once_and_again_after_adding_time() {
        let mut state = state_with_turns();
        state.show_turns();
        state.adjust_turn_minutes(-4);
        state.start_turn();
        let now = Local::now().timestamp_millis();
        let end_ms = state.target.unwrap().timestamp_millis();

        assert_eq!(state.tick_turns(now + 30_000, end_ms), TickSound::None);
        assert_eq!(state.tick_turns(now + 61_000, end_ms), TickSound::TurnEnd);
        assert_eq!(state.tick_turns(now + 62_000, end_ms), TickSound::None);

        state.adjust_turn_time(1);

        assert_eq!(state.tick_turns(now + 63_000, end_ms), TickSound::None);
        assert_eq!(state.tick_turns(now + 121_000, end_ms), TickSound::TurnEnd);
    }

    #[test]
    fn no_beep_when_the_meeting_is_over() {
        let mut state = state_with_turns();
        state.show_turns();
        state.start_turn();
        let end_ms = state.target.unwrap().timestamp_millis();

        assert_eq!(state.tick_turns(end_ms, end_ms), TickSound::None);
    }

    #[test]
    fn ending_a_turn_returns_to_the_settings() {
        let mut state = state_with_turns();
        state.show_turns();
        state.select_turner(2);
        state.start_turn();

        state.end_turn();

        assert_eq!(state.turn_view, TurnView::Settings);
        assert!(state.turn.is_none());
        assert_eq!(state.turn_selected, 2);
    }

    #[test]
    fn stopping_resets_the_turns() {
        let mut state = state_with_turns();
        state.show_turns();
        state.start_turn();

        state.stop(StopType::UserStopped);

        assert_eq!(state.turn_view, TurnView::Plain);
        assert!(state.turn.is_none());
        assert!(state.turn_names.is_empty());
        assert!(!state.turns_available());
    }

    #[test]
    fn the_meeting_ending_resets_the_turns() {
        let mut state = state_with_turns();
        state.show_turns();
        state.start_turn();

        state.stop(StopType::TimerFinished);

        assert_eq!(state.page, Page::TimerFinished);
        assert_eq!(state.turn_view, TurnView::Plain);
        assert!(state.turn.is_none());
    }

    /// State with one meeting of `names` that ends in 10 minutes, listed on the meetings page.
    fn state_with_one_meeting(title: &str, names: &[&str]) -> AppState {
        let now = Local::now().fixed_offset();
        let mut state = sample_state();
        state.on_signed_in(&me(None));
        let mut talk = meeting(
            title,
            now - chrono::Duration::minutes(1),
            now + chrono::Duration::minutes(10),
        );
        talk.attendees = names.iter().map(|name| person(name)).collect();
        state.on_meetings_loaded(vec![talk]);
        state
    }

    fn row(state: &AppState, index: usize) -> MeetingRow {
        state.meetings_model.row_data(index).unwrap()
    }

    fn daily_state() -> AppState {
        let mut state = state_with_one_meeting("Daily PMO", &["Zoe", "Ann", "Bob"]);
        let _ = state.select_meeting(0);
        state
    }

    fn round_of(names: &[&str]) -> DailyRound {
        let order = names.iter().map(|name| name.to_string()).collect();
        DailyRound::new(order, 600_000, 0)
    }

    #[test]
    fn the_switch_starts_on_for_a_daily_title_only() {
        let daily = state_with_one_meeting("Daily PMO", &["Ann"]);
        let other = state_with_one_meeting("Retro", &["Ann"]);

        assert!(row(&daily, 0).daily_on);
        assert!(row(&daily, 0).daily_auto);
        assert!(!row(&other, 0).daily_on);
        assert!(!row(&other, 0).daily_auto);
    }

    #[test]
    fn the_auto_badge_condition_survives_switching_off() {
        let mut state = state_with_one_meeting("Daily PMO", &["Ann"]);

        state.set_daily(0, false);

        assert!(!row(&state, 0).daily_on);
        assert!(row(&state, 0).daily_auto);
    }

    #[test]
    fn the_user_choice_survives_a_refresh_of_the_list() {
        let mut state = state_with_one_meeting("Retro", &["Ann"]);
        state.set_daily(0, true);
        let listed = state.meetings.clone();

        state.on_meetings_loaded(listed);

        assert!(row(&state, 0).daily_on);
    }

    #[test]
    fn the_choice_is_dropped_when_the_event_is_no_longer_listed() {
        let mut state = state_with_one_meeting("Retro", &["Ann"]);
        state.set_daily(0, true);

        state.on_meetings_loaded(Vec::new());

        assert!(state.daily_choices.is_empty());
    }

    #[test]
    fn setting_the_switch_of_an_invalid_row_is_ignored() {
        let mut state = state_with_one_meeting("Retro", &["Ann"]);

        state.set_daily(-1, true);
        state.set_daily(5, true);

        assert!(state.daily_choices.is_empty());
    }

    #[test]
    fn starting_with_the_switch_on_opens_the_daily_screen() {
        let state = daily_state();

        assert_eq!(state.turn_view, TurnView::Daily);
        assert_eq!(state.page, Page::TimerPage);
        assert!(state.daily.is_some());
    }

    #[test]
    fn starting_with_the_switch_on_but_nobody_to_hear_is_a_plain_timer() {
        let mut state = state_with_one_meeting("Daily PMO", &[]);

        let _ = state.select_meeting(0);

        assert_eq!(state.turn_view, TurnView::Plain);
        assert!(state.daily.is_none());
    }

    #[test]
    fn starting_with_the_switch_off_is_a_plain_timer() {
        let mut state = state_with_one_meeting("Daily PMO", &["Ann", "Bob"]);
        state.set_daily(0, false);

        let _ = state.select_meeting(0);

        assert_eq!(state.turn_view, TurnView::Plain);
        assert!(state.daily.is_none());
        assert!(state.turns_available());
    }

    #[test]
    fn the_switch_on_makes_a_non_daily_title_start_the_daily() {
        let mut state = state_with_one_meeting("Retro", &["Ann", "Bob"]);
        state.set_daily(0, true);

        let _ = state.select_meeting(0);

        assert_eq!(state.turn_view, TurnView::Daily);
    }

    #[test]
    fn the_speaking_order_is_a_permutation_of_the_participants() {
        let state = daily_state();
        let round = state.daily.as_ref().unwrap();

        let mut order = vec![round.current().unwrap().to_string()];
        order.extend(round.upcoming().iter().cloned());
        order.sort();

        assert_eq!(order, vec!["Ann", "Bob", "Zoe"]);
    }

    #[test]
    fn pause_and_resume_reach_the_round() {
        let mut state = daily_state();

        state.toggle_daily_pause();
        assert!(state.daily.as_ref().unwrap().is_paused());

        state.toggle_daily_pause();
        assert!(!state.daily.as_ref().unwrap().is_paused());
    }

    #[test]
    fn passing_the_word_moves_to_the_next_speaker() {
        let mut state = daily_state();
        let first = state.daily.as_ref().unwrap().current().unwrap().to_string();

        state.next_daily_speaker();

        let round = state.daily.as_ref().unwrap();
        assert_ne!(round.current().unwrap(), first);
        assert_eq!(round.upcoming().len(), 1);
    }

    #[test]
    fn adjusting_the_time_changes_the_speaker_time() {
        let mut state = daily_state();
        let now = Local::now().timestamp_millis();
        let before = state.daily.as_ref().unwrap().current_left_ms(now);

        state.adjust_daily_time(-1);

        let now = Local::now().timestamp_millis();
        let after = state.daily.as_ref().unwrap().current_left_ms(now);
        assert!(after < before);
    }

    #[test]
    fn the_beep_plays_on_the_automatic_hand_over() {
        let mut state = daily_state();
        let now = Local::now().timestamp_millis();
        let end_ms = state.target.unwrap().timestamp_millis();
        let first = state.daily.as_ref().unwrap().current().unwrap().to_string();

        let sound = state.tick_turns(now + 4 * MINUTE_MS, end_ms);

        assert_eq!(sound, TickSound::TurnEnd);
        assert_ne!(state.daily.as_ref().unwrap().current().unwrap(), first);
    }

    #[test]
    fn no_beep_while_the_speaker_still_has_time() {
        let mut state = daily_state();
        let now = Local::now().timestamp_millis();
        let end_ms = state.target.unwrap().timestamp_millis();

        assert_eq!(state.tick_turns(now + 1_000, end_ms), TickSound::None);
    }

    #[test]
    fn the_view_of_a_running_round_lists_the_upcoming_people() {
        let round = round_of(&["Andrea Zanella", "Giulia Ferri", "Luca Rossi"]);

        let view = daily_view(&round, 0);

        assert_eq!(view.name, "Andrea Zanella");
        assert_eq!(view.chips, vec!["Giulia Ferri ~03:20", "Luca Rossi ~03:20"]);
        assert!(!view.finished);
        assert!(!view.paused);
        assert!(view.can_remove);
    }

    #[test]
    fn the_compact_name_is_abbreviated_and_the_full_one_is_kept() {
        let round = round_of(&["Andrea Zanella Rossi"]);

        let view = daily_view(&round, 0);

        assert_eq!(view.name, "Andrea Zanella Rossi");
        assert_eq!(view.short_name, "Andrea Z. R.");
    }

    #[test]
    fn the_remaining_line_counts_people() {
        let three = daily_view(&round_of(&["a", "b", "c", "d"]), 0);
        let one = daily_view(&round_of(&["a", "b"]), 0);

        assert_eq!(three.remaining, "3 persone ~ 02:30");
        assert_eq!(one.remaining, "1 persona ~ 05:00");
    }

    #[test]
    fn the_last_speaker_sees_ultimo_turno() {
        let view = daily_view(&round_of(&["a"]), 0);

        assert_eq!(view.remaining, "Ultimo turno");
        assert!(view.chips.is_empty());
    }

    #[test]
    fn when_everybody_spoke_the_screen_says_so_and_disables_the_controls() {
        let mut round = round_of(&["a"]);
        round.next(0);

        let view = daily_view(&round, 0);

        assert_eq!(view.name, "Tutti hanno parlato");
        assert_eq!(view.short_name, "Tutti hanno parlato");
        assert!(view.finished);
        assert_eq!(view.left_ms, 0);
        assert!(!view.can_add);
        assert!(!view.can_remove);
        assert_eq!(view.remaining, "");
        assert!(view.chips.is_empty());
    }

    #[test]
    fn a_paused_round_is_reported_paused() {
        let mut round = round_of(&["a", "b"]);
        round.toggle_pause(0);

        assert!(daily_view(&round, 1_000).paused);
    }

    #[test]
    fn estimates_are_formatted_as_minutes_and_seconds() {
        assert_eq!(format_minutes_seconds(0), "00:00");
        assert_eq!(format_minutes_seconds(188_400), "03:08");
        assert_eq!(format_minutes_seconds(3_660_000), "61:00");
        assert_eq!(format_minutes_seconds(-5), "00:00");
    }

    #[test]
    fn stopping_clears_the_daily_state() {
        let mut state = daily_state();

        state.stop(StopType::UserStopped);

        assert!(state.daily.is_none());
        assert_eq!(state.turn_view, TurnView::Plain);
        assert_eq!(state.page, Page::MeetingsPage);
    }

    #[test]
    fn the_meeting_ending_clears_the_daily_state() {
        let mut state = daily_state();

        state.stop(StopType::TimerFinished);

        assert!(state.daily.is_none());
        assert_eq!(state.turn_view, TurnView::Plain);
        assert_eq!(state.page, Page::TimerFinished);
    }

    #[test]
    fn turns_cannot_be_opened_from_the_daily_screen() {
        let mut state = daily_state();

        state.show_turns();

        assert_eq!(state.turn_view, TurnView::Daily);
    }

    #[test]
    fn stop_returns_to_selection_when_not_signed_in() {
        let mut state = sample_state();
        state.target = Some(Local::now());

        state.stop(StopType::UserStopped);

        assert_eq!(state.page, Page::ManualPage);
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
        assert_eq!(local.page, Page::ManualPage);
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

        assert_eq!(state.page, Page::ManualPage);
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
        assert_eq!(state.page, Page::ManualPage);
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
    fn editing_meeting() -> AppState {
        let mut state = state_with_meetings();
        let mut talk = state.shown.first().unwrap().clone();
        talk.attendees = vec![
            Attendee {
                email: "a@x.it".into(),
                name: "A".into(),
                is_self: false,
                optional: false,
                response: ResponseStatus::Accepted,
            },
            Attendee {
                email: "b@x.it".into(),
                name: "B".into(),
                is_self: false,
                optional: false,
                response: ResponseStatus::Accepted,
            },
        ];
        state.on_meetings_loaded(vec![talk]);
        let event_id = state.open_editor(0).unwrap();
        state.on_editor_plan_loaded(&event_id, Err(BackendError::Unreachable));
        state
    }

    #[test]
    fn editor_opens_without_starting_countdown() {
        let mut state = state_with_meetings();
        assert!(state.open_editor(0).is_some());
        assert_eq!(state.page, Page::EditorPage);
        assert!(state.target.is_none());
        assert!(state.editor.as_ref().unwrap().loading);
    }

    #[test]
    fn invalid_editor_row_keeps_meetings_page() {
        let mut state = state_with_meetings();
        assert!(state.open_editor(-1).is_none());
        assert!(state.open_editor(99).is_none());
        assert_eq!(state.page, Page::MeetingsPage);
        assert!(state.editor.is_none());
    }

    #[test]
    fn editor_plan_failure_uses_equal_participant_minutes() {
        let state = editing_meeting();
        let open = state.editor.as_ref().unwrap();
        assert!(!open.loading);
        assert_eq!(open.notice, EDITOR_PLAN_FALLBACK_NOTICE);
        let rows = open.editor.rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].minutes, 60);
        assert_eq!(rows[1].minutes, 60);
    }

    #[test]
    fn editor_start_uses_custom_order_and_excludes_daily() {
        let mut state = editing_meeting();
        state.set_daily(0, true);
        state.editor_move_down(0);
        state.editor_set_minutes(0, 20);
        let target = state
            .editor
            .as_ref()
            .unwrap()
            .editor
            .meeting()
            .end
            .with_timezone(&Local);
        assert_eq!(state.start_from_editor(), TickSound::None);
        assert_eq!(state.target, Some(target));
        assert_eq!(state.turn_view, TurnView::Scheduled);
        assert!(state.daily.is_none());
        assert!(state.editor.is_none());
        assert_eq!(
            state
                .slots
                .as_ref()
                .unwrap()
                .current(&Local::now())
                .unwrap()
                .name,
            "B"
        );
    }

    #[test]
    fn editor_start_waits_for_plan() {
        let mut state = state_with_meetings();
        state.open_editor(0);
        assert_eq!(state.start_from_editor(), TickSound::None);
        assert!(state.target.is_none());
        assert_eq!(state.page, Page::EditorPage);
    }

    #[test]
    fn editor_start_rejects_expired_meeting() {
        let mut state = editing_meeting();
        let expired = Local::now().fixed_offset() - chrono::Duration::seconds(1);
        let mut talk = state.editor.as_ref().unwrap().editor.meeting().clone();
        talk.end = expired;
        let mut editor = SlotEditor::new(talk);
        editor.reset_to_equal();
        state.editor.as_mut().unwrap().editor = editor;
        assert_eq!(state.start_from_editor(), TickSound::None);
        assert!(state.target.is_none());
        assert_eq!(state.editor.as_ref().unwrap().notice, MEETING_OVER_NOTICE);
    }

    #[test]
    fn editor_save_tracks_template_and_ignores_duplicate_result() {
        let mut state = editing_meeting();
        let save = state.start_template_save().unwrap();
        assert!(state.start_template_save().is_none());
        assert!(state.on_template_saved(
            &save.event_id,
            Ok(Template {
                id: "saved".into(),
                meeting_title: save.request.meeting_title,
                entries: save.request.entries
            })
        ));
        assert_eq!(
            state.editor.as_ref().unwrap().editor.template_id(),
            Some("saved")
        );
        assert!(!state.on_template_saved(&save.event_id, Err(BackendError::Unreachable)));
    }

    #[test]
    fn editor_closed_drops_pending_plan_and_save() {
        let mut state = editing_meeting();
        let save = state.start_template_save().unwrap();
        state.close_editor();
        state.on_editor_plan_loaded(&save.event_id, Err(BackendError::Unreachable));
        assert!(!state.on_template_saved(&save.event_id, Err(BackendError::Unreachable)));
        assert!(state.editor.is_none());
        assert_eq!(state.page, Page::MeetingsPage);
    }

    #[test]
    fn last_editor_turn_enters_free_time_and_stop_returns_to_meetings() {
        let mut state = editing_meeting();
        let _ = state.start_from_editor();
        state.next_slot();
        state.next_slot();
        assert!(state.free_time);
        assert!(state.target.is_some());
        state.stop(StopType::UserStopped);
        assert!(state.slots.is_none());
        assert_eq!(state.page, Page::MeetingsPage);
    }

    #[test]
    fn editor_without_participants_falls_back_to_plain_countdown() {
        let mut state = state_with_meetings();
        let event_id = state.open_editor(0).unwrap();
        state.on_editor_plan_loaded(&event_id, Err(BackendError::Unreachable));
        let _ = state.start_from_editor();
        assert!(state.target.is_some());
        assert!(state.slots.is_none());
        assert_eq!(state.turn_view, TurnView::Plain);
    }

    #[test]
    fn editor_slots_advance_with_one_beep() {
        let mut state = editing_meeting();
        let _ = state.start_from_editor();
        let now = Local::now();
        let remaining = state
            .slots
            .as_ref()
            .unwrap()
            .current(&now)
            .unwrap()
            .remaining_ms;
        let next = now + chrono::Duration::milliseconds(remaining + 1);
        assert_eq!(state.update_slot_index(&next), TickSound::TurnEnd);
        assert_eq!(state.update_slot_index(&next), TickSound::None);
    }

    #[test]
    fn editor_meeting_finish_clears_slots_and_keeps_return_origin() {
        let mut state = editing_meeting();
        let _ = state.start_from_editor();
        state.target = Some(Local::now() - chrono::Duration::milliseconds(1));
        assert_eq!(state.tick(), TickSound::MeetingEnd);
        assert_eq!(state.page, Page::TimerFinished);
        assert!(state.slots.is_none());
        state.show_home();
        assert_eq!(state.page, Page::MeetingsPage);
    }
}
