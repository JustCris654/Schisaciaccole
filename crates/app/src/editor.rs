//! Slot editor: the minutes rows of one meeting and the operations the editor page offers.
//!
//! Plain data without Slint types: [`crate::state::AppState`] owns a [`SlotEditor`] while the
//! editor page is open and turns its rows into the UI model. Row indices come from the UI, so
//! every operation ignores an index out of range instead of failing.

use log::debug;
use schisaciaccole_core::api::{EntryKind, Meeting, PutTemplateRequest, SlotPlan};
use schisaciaccole_core::{
    EditorEntry, build_plan, editor_to_template, participants, plan_to_editor, template_to_plan,
    unassigned_minutes,
};

use crate::state::weighted_slots;

/// Most rows the editor holds: adding a row beyond it does nothing.
pub const MAX_ROWS: usize = 99;
/// Most minutes a row can take; larger values are clamped to it.
pub const MAX_MINUTES: u32 = 600;
/// Label of a row added by hand.
pub const NEW_ROW_LABEL: &str = "Voce";
/// Minutes of a row added by hand.
pub const NEW_ROW_MINUTES: u32 = 5;

/// Rows of the minutes editor for one meeting, with the template they were loaded from.
#[derive(Debug, Clone)]
pub struct SlotEditor {
    meeting: Meeting,
    /// Nominal duration of the meeting (end minus start), never negative.
    duration_secs: i64,
    rows: Vec<EditorEntry>,
    /// Id of the saved template of the meeting title, when one is known.
    template_id: Option<String>,
}

impl SlotEditor {
    /// Creates an editor for `meeting` with no rows and no known template.
    ///
    /// Edge cases: a meeting that ends before it starts gets a duration of 0 seconds.
    pub fn new(meeting: Meeting) -> Self {
        let duration_secs = meeting
            .end
            .signed_duration_since(meeting.start)
            .num_seconds()
            .max(0);
        Self {
            meeting,
            duration_secs,
            rows: Vec::new(),
            template_id: None,
        }
    }

    /// Meeting being edited.
    pub fn meeting(&self) -> &Meeting {
        &self.meeting
    }

    /// Calendar event id of the meeting being edited.
    pub fn event_id(&self) -> &str {
        &self.meeting.event_id
    }

    /// Rows in display order.
    pub fn rows(&self) -> &[EditorEntry] {
        &self.rows
    }

    /// Id of the saved template, when one is known.
    pub fn template_id(&self) -> Option<&str> {
        self.template_id.as_deref()
    }

    /// Replaces the known template id.
    pub fn set_template_id(&mut self, template_id: Option<String>) {
        self.template_id = template_id;
    }

    /// Nominal duration of the meeting in whole minutes (rounded down).
    pub fn duration_minutes(&self) -> i64 {
        self.duration_secs / 60
    }

    /// Replaces the rows with those of `plan` over the nominal duration and remembers its
    /// template id (`None` for an equal plan). Fresh participants replace the meeting snapshot
    /// used by equal reset. Legacy plans without participants preserve that snapshot and merge
    /// warning flags conservatively.
    pub fn load_plan(&mut self, plan: &SlotPlan) {
        if let Some(attendees) = &plan.attendees {
            self.meeting.attendees = attendees.clone();
            self.meeting.attendees_omitted = plan.attendees_omitted;
            self.meeting.participant_resolution = plan.participant_resolution;
        } else {
            self.meeting.attendees_omitted |= plan.attendees_omitted;
            self.meeting.participant_resolution.incomplete |=
                plan.participant_resolution.incomplete;
            self.meeting.participant_resolution.reauthorization_required |=
                plan.participant_resolution.reauthorization_required;
        }
        self.rows = plan_to_editor(&plan.entries, self.duration_secs);
        self.template_id = plan.template_id.clone();
    }

    /// Replaces the rows with the equal plan of the meeting participants. The template id is
    /// kept.
    ///
    /// Edge cases: a meeting without participants gives no rows.
    pub fn reset_to_equal(&mut self) {
        let equal = build_plan(&participants(&self.meeting), None);
        self.rows = plan_to_editor(&equal, self.duration_secs);
    }

    /// Sets the label of row `index` as typed; trimming happens when the rows are saved.
    ///
    /// Edge cases: an index out of range is logged and ignored.
    pub fn set_label(&mut self, index: usize, label: &str) {
        match self.rows.get_mut(index) {
            Some(row) => row.label = label.to_string(),
            None => debug!("set_label: ignoring out of range row {index}"),
        }
    }

    /// Sets the minutes of row `index`, clamped to `0..=MAX_MINUTES`.
    ///
    /// Edge cases: a negative value becomes 0; an index out of range is logged and ignored.
    pub fn set_minutes(&mut self, index: usize, minutes: i32) {
        let minutes = u32::try_from(minutes).unwrap_or(0).min(MAX_MINUTES);
        match self.rows.get_mut(index) {
            Some(row) => row.minutes = minutes,
            None => debug!("set_minutes: ignoring out of range row {index}"),
        }
    }

    /// Includes or excludes the attendee of row `index` from the slots.
    ///
    /// Edge cases: a row that is not an attendee is always included, so it is left unchanged;
    /// an index out of range is logged and ignored.
    pub fn set_included(&mut self, index: usize, included: bool) {
        match self.rows.get_mut(index) {
            Some(row) if row.kind == EntryKind::Attendee => row.included = included,
            Some(_) => debug!("set_included: row {index} is not an attendee"),
            None => debug!("set_included: ignoring out of range row {index}"),
        }
    }

    /// Swaps row `index` with the one above it.
    ///
    /// Edge cases: the first row and an index out of range are left unchanged.
    pub fn move_up(&mut self, index: usize) {
        if index == 0 || index >= self.rows.len() {
            debug!("move_up: ignoring row {index}");
            return;
        }
        self.rows.swap(index - 1, index);
    }

    /// Swaps row `index` with the one below it.
    ///
    /// Edge cases: the last row and an index out of range are left unchanged.
    pub fn move_down(&mut self, index: usize) {
        if index.saturating_add(1) >= self.rows.len() {
            debug!("move_down: ignoring row {index}");
            return;
        }
        self.rows.swap(index, index + 1);
    }

    /// Appends an included manual row labelled [`NEW_ROW_LABEL`] with [`NEW_ROW_MINUTES`].
    ///
    /// Edge cases: nothing is added when the editor already holds [`MAX_ROWS`] rows.
    pub fn add_manual_row(&mut self) {
        if self.rows.len() >= MAX_ROWS {
            debug!("add_manual_row: already {MAX_ROWS} rows");
            return;
        }
        self.rows.push(EditorEntry {
            kind: EntryKind::Manual,
            email: None,
            label: NEW_ROW_LABEL.to_string(),
            minutes: NEW_ROW_MINUTES,
            included: true,
        });
    }

    /// Removes row `index` when it was added by hand.
    ///
    /// Edge cases: attendee rows (they can only be excluded) and an index out of range are left
    /// unchanged.
    pub fn remove_row(&mut self, index: usize) {
        match self.rows.get(index) {
            Some(row) if row.kind == EntryKind::Manual => {
                self.rows.remove(index);
            }
            Some(_) => debug!("remove_row: row {index} is not a manual row"),
            None => debug!("remove_row: ignoring out of range row {index}"),
        }
    }

    /// Minutes of the nominal duration not given to any included row; negative when the rows
    /// take more than the meeting.
    pub fn unassigned_minutes(&self) -> i64 {
        unassigned_minutes(&self.rows, self.duration_secs)
    }

    /// True when the included rows take more minutes than the meeting has.
    pub fn is_over(&self) -> bool {
        self.unassigned_minutes() < 0
    }

    /// Footer line about the unassigned time: `"Non assegnati: N min"`, or
    /// `"Oltre la durata di N min"` when the rows take more than the meeting.
    pub fn unassigned_text(&self) -> String {
        let minutes = self.unassigned_minutes();
        if minutes < 0 {
            format!("Oltre la durata di {} min", minutes.unsigned_abs())
        } else {
            format!("Non assegnati: {minutes} min")
        }
    }

    /// Request that saves the rows as the template of the meeting title.
    ///
    /// The entries come from [`editor_to_template`]: excluded rows weigh 0 and unassigned time
    /// becomes a trailing free time entry.
    pub fn save_request(&self) -> PutTemplateRequest {
        PutTemplateRequest {
            meeting_title: self.meeting.title.clone(),
            entries: editor_to_template(&self.rows, self.duration_secs),
        }
    }

    /// `(label, weight)` slots to start the timer with: the saved form of the rows, unassigned
    /// time included as a last free time slot, without the entries that weigh 0 (excluded rows,
    /// rows with 0 minutes).
    ///
    /// Edge cases: no rows and no unassigned time give an empty list.
    pub fn start_slots(&self) -> Vec<(String, u64)> {
        let entries = editor_to_template(&self.rows, self.duration_secs);
        weighted_slots(&template_to_plan(&entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, FixedOffset, TimeZone};
    use schisaciaccole_core::api::{Attendee, PlanEntry, PlanSource, ResponseStatus};
    use schisaciaccole_core::{FREE_TIME_LABEL, TemplateEntry};

    fn at(hour: u32, minute: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 3, 10, hour, minute, 0)
            .unwrap()
    }

    fn attendee(name: &str) -> Attendee {
        Attendee {
            email: format!("{}@x.it", name.to_lowercase()),
            name: name.to_string(),
            is_self: false,
            optional: false,
            response: ResponseStatus::Accepted,
        }
    }

    /// One hour meeting "Daily" with the given attendees.
    fn meeting(names: &[&str]) -> Meeting {
        Meeting {
            event_id: "ev1".to_string(),
            title: "Daily".to_string(),
            start: at(9, 0),
            end: at(10, 0),
            attendees: names.iter().map(|name| attendee(name)).collect(),
            attendees_omitted: false,
            participant_resolution: Default::default(),
            recurring_event_id: None,
            has_template: false,
        }
    }

    /// Editor on the equal plan of Ann, Bob and Carl: 20 minutes each.
    fn equal_editor() -> SlotEditor {
        let mut editor = SlotEditor::new(meeting(&["Carl", "Ann", "Bob"]));
        editor.reset_to_equal();
        editor
    }

    fn labels(editor: &SlotEditor) -> Vec<&str> {
        editor.rows().iter().map(|row| row.label.as_str()).collect()
    }

    fn plan_entry(kind: EntryKind, label: &str, weight: u64) -> PlanEntry {
        PlanEntry {
            kind,
            email: match kind {
                EntryKind::Attendee => Some(format!("{}@x.it", label.to_lowercase())),
                _ => None,
            },
            label: label.to_string(),
            weight,
        }
    }

    #[test]
    fn new_editor_is_empty_with_the_nominal_duration() {
        let editor = SlotEditor::new(meeting(&["Ann"]));

        assert!(editor.rows().is_empty());
        assert_eq!(editor.template_id(), None);
        assert_eq!(editor.duration_minutes(), 60);
        assert_eq!(editor.event_id(), "ev1");
    }

    #[test]
    fn meeting_ending_before_its_start_has_no_duration() {
        let mut backwards = meeting(&["Ann"]);
        backwards.end = at(8, 0);

        let editor = SlotEditor::new(backwards);

        assert_eq!(editor.duration_minutes(), 0);
    }

    #[test]
    fn duration_rounds_partial_minutes_down() {
        let mut odd = meeting(&["Ann"]);
        odd.end = at(9, 30) + chrono::Duration::seconds(59);

        let editor = SlotEditor::new(odd);

        assert_eq!(editor.duration_minutes(), 30);
    }

    #[test]
    fn equal_reset_splits_the_meeting_among_participants() {
        let editor = equal_editor();

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
        assert!(editor.rows().iter().all(|row| row.minutes == 20));
        assert!(editor.rows().iter().all(|row| row.included));
    }

    #[test]
    fn equal_reset_keeps_the_template_id() {
        let mut editor = equal_editor();
        editor.set_template_id(Some("t1".to_string()));

        editor.reset_to_equal();

        assert_eq!(editor.template_id(), Some("t1"));
    }

    #[test]
    fn loading_a_plan_fills_rows_and_template_id() {
        let mut editor = SlotEditor::new(meeting(&["Ann", "Bob"]));
        let plan = SlotPlan {
            attendees: None,
            attendees_omitted: false,
            participant_resolution: Default::default(),
            source: PlanSource::Template,
            template_id: Some("t1".to_string()),
            entries: vec![
                plan_entry(EntryKind::Attendee, "Ann", 2),
                plan_entry(EntryKind::Attendee, "Bob", 0),
                plan_entry(EntryKind::FreeTime, FREE_TIME_LABEL, 1),
            ],
        };

        editor.load_plan(&plan);

        assert_eq!(labels(&editor), vec!["Ann", "Bob"]);
        assert_eq!(editor.rows()[0].minutes, 40);
        assert!(!editor.rows()[1].included);
        assert_eq!(editor.template_id(), Some("t1"));
    }

    #[test]
    fn set_label_keeps_the_text_as_typed() {
        let mut editor = equal_editor();

        editor.set_label(1, " Roberto ");

        assert_eq!(editor.rows()[1].label, " Roberto ");
    }

    #[test]
    fn set_label_out_of_range_changes_nothing() {
        let mut editor = equal_editor();

        editor.set_label(3, "X");

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn set_minutes_stores_the_value() {
        let mut editor = equal_editor();

        editor.set_minutes(0, 7);

        assert_eq!(editor.rows()[0].minutes, 7);
    }

    #[test]
    fn set_minutes_clamps_to_the_maximum() {
        let mut editor = equal_editor();

        editor.set_minutes(0, 601);

        assert_eq!(editor.rows()[0].minutes, MAX_MINUTES);
    }

    #[test]
    fn set_minutes_turns_negative_into_zero() {
        let mut editor = equal_editor();

        editor.set_minutes(0, -5);

        assert_eq!(editor.rows()[0].minutes, 0);
    }

    #[test]
    fn set_minutes_out_of_range_changes_nothing() {
        let mut editor = equal_editor();

        editor.set_minutes(9, 1);

        assert!(editor.rows().iter().all(|row| row.minutes == 20));
    }

    #[test]
    fn attendee_can_be_excluded_and_included_again() {
        let mut editor = equal_editor();

        editor.set_included(1, false);
        assert!(!editor.rows()[1].included);

        editor.set_included(1, true);
        assert!(editor.rows()[1].included);
    }

    #[test]
    fn manual_row_cannot_be_excluded() {
        let mut editor = equal_editor();
        editor.add_manual_row();

        editor.set_included(3, false);

        assert!(editor.rows()[3].included);
    }

    #[test]
    fn set_included_out_of_range_changes_nothing() {
        let mut editor = equal_editor();

        editor.set_included(3, false);

        assert!(editor.rows().iter().all(|row| row.included));
    }

    #[test]
    fn move_up_swaps_with_the_previous_row() {
        let mut editor = equal_editor();

        editor.move_up(2);

        assert_eq!(labels(&editor), vec!["Ann", "Carl", "Bob"]);
    }

    #[test]
    fn move_up_of_the_first_row_changes_nothing() {
        let mut editor = equal_editor();

        editor.move_up(0);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn move_up_out_of_range_changes_nothing() {
        let mut editor = equal_editor();

        editor.move_up(3);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn move_down_swaps_with_the_next_row() {
        let mut editor = equal_editor();

        editor.move_down(0);

        assert_eq!(labels(&editor), vec!["Bob", "Ann", "Carl"]);
    }

    #[test]
    fn move_down_of_the_last_row_changes_nothing() {
        let mut editor = equal_editor();

        editor.move_down(2);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn move_down_out_of_range_changes_nothing() {
        let mut editor = equal_editor();

        editor.move_down(usize::MAX);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn added_row_is_an_included_manual_row() {
        let mut editor = equal_editor();

        editor.add_manual_row();

        let added = editor.rows().last().unwrap();
        assert_eq!(added.kind, EntryKind::Manual);
        assert_eq!(added.label, NEW_ROW_LABEL);
        assert_eq!(added.minutes, NEW_ROW_MINUTES);
        assert!(added.included);
        assert_eq!(added.email, None);
    }

    #[test]
    fn rows_stop_at_the_limit() {
        let mut editor = equal_editor();

        for _ in 0..MAX_ROWS {
            editor.add_manual_row();
        }

        assert_eq!(editor.rows().len(), MAX_ROWS);
    }

    #[test]
    fn manual_row_can_be_removed() {
        let mut editor = equal_editor();
        editor.add_manual_row();

        editor.remove_row(3);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn attendee_row_cannot_be_removed() {
        let mut editor = equal_editor();

        editor.remove_row(0);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn remove_out_of_range_changes_nothing() {
        let mut editor = equal_editor();

        editor.remove_row(3);

        assert_eq!(editor.rows().len(), 3);
    }

    #[test]
    fn unassigned_text_shows_the_free_minutes() {
        let mut editor = equal_editor();
        editor.set_minutes(0, 5);

        assert_eq!(editor.unassigned_text(), "Non assegnati: 15 min");
        assert!(!editor.is_over());
    }

    #[test]
    fn unassigned_text_ignores_excluded_rows() {
        let mut editor = equal_editor();

        editor.set_included(2, false);

        assert_eq!(editor.unassigned_text(), "Non assegnati: 20 min");
    }

    #[test]
    fn unassigned_text_shows_the_excess_when_over() {
        let mut editor = equal_editor();
        editor.set_minutes(0, 32);

        assert_eq!(editor.unassigned_text(), "Oltre la durata di 12 min");
        assert!(editor.is_over());
    }

    #[test]
    fn save_request_has_the_title_and_trailing_free_time() {
        let mut editor = equal_editor();
        editor.set_included(1, false);

        let request = editor.save_request();

        assert_eq!(request.meeting_title, "Daily");
        let weights: Vec<u32> = request.entries.iter().map(|e| e.weight_secs).collect();
        assert_eq!(weights, vec![1200, 0, 1200, 1200]);
        let free = request.entries.last().unwrap();
        assert_eq!(free.kind, EntryKind::FreeTime);
        assert_eq!(free.label, FREE_TIME_LABEL);
    }

    #[test]
    fn save_request_without_unassigned_time_has_no_free_time() {
        let editor = equal_editor();

        let request = editor.save_request();

        assert_eq!(request.entries.len(), 3);
        assert!(
            request
                .entries
                .iter()
                .all(|entry: &TemplateEntry| entry.kind == EntryKind::Attendee)
        );
    }

    #[test]
    fn start_slots_skip_excluded_rows_and_keep_free_time() {
        let mut editor = equal_editor();
        editor.set_included(1, false);

        let slots = editor.start_slots();

        assert_eq!(
            slots,
            vec![
                ("Ann".to_string(), 1200),
                ("Carl".to_string(), 1200),
                (FREE_TIME_LABEL.to_string(), 1200),
            ]
        );
    }

    #[test]
    fn start_slots_skip_rows_with_zero_minutes() {
        let mut editor = equal_editor();
        editor.set_minutes(0, 0);
        editor.set_minutes(1, 30);
        editor.set_minutes(2, 30);

        let slots = editor.start_slots();

        assert_eq!(
            slots,
            vec![("Bob".to_string(), 1800), ("Carl".to_string(), 1800)]
        );
    }

    #[test]
    fn fresh_empty_group_stays_empty_on_equal_reset() {
        let mut editor = equal_editor();
        let plan = SlotPlan {
            source: PlanSource::Equal,
            template_id: None,
            entries: Vec::new(),
            attendees: Some(Vec::new()),
            attendees_omitted: false,
            participant_resolution: Default::default(),
        };

        editor.load_plan(&plan);
        editor.reset_to_equal();

        assert!(editor.rows().is_empty());
        assert!(editor.meeting().attendees.is_empty());
    }

    #[test]
    fn legacy_plan_preserves_participants_and_prior_warning() {
        let mut meeting = meeting(&["Ann"]);
        meeting.attendees_omitted = true;
        let mut editor = SlotEditor::new(meeting);
        let plan = SlotPlan {
            source: PlanSource::Equal,
            template_id: None,
            entries: Vec::new(),
            attendees: None,
            attendees_omitted: false,
            participant_resolution: Default::default(),
        };

        editor.load_plan(&plan);
        editor.reset_to_equal();

        assert_eq!(labels(&editor), vec!["Ann"]);
        assert!(editor.meeting().attendees_omitted);
    }
}
