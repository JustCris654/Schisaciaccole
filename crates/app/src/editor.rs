//! Participant editor: who takes part in one meeting and the operations the editor page offers.
//!
//! Plain data without Slint types: [`crate::state::AppState`] owns a [`ParticipantEditor`] while
//! the editor page is open and turns its rows into the UI model. Row indices come from the UI,
//! so every operation ignores an index out of range instead of failing.

use log::debug;
use schisaciaccole_core::api::{Meeting, PersonKind, PutTemplateRequest, SlotPlan};
use schisaciaccole_core::{
    EditorEntry, build_plan, editor_to_template, participants, plan_to_editor,
};

/// Most rows the editor holds: adding a row beyond it does nothing.
pub const MAX_ROWS: usize = 99;

/// Participants of one meeting, with the template they were loaded from.
#[derive(Debug, Clone)]
pub struct ParticipantEditor {
    meeting: Meeting,
    /// Nominal duration of the meeting (end minus start), never negative.
    duration_secs: i64,
    rows: Vec<EditorEntry>,
    /// Id of the saved template of the meeting title, when one is known.
    template_id: Option<String>,
}

impl ParticipantEditor {
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

    /// Replaces the rows with those of `plan` and remembers its template id (`None` for an equal
    /// plan). Fresh participants replace the meeting snapshot used by the participant reset.
    /// Legacy plans without participants preserve that snapshot and merge warning flags
    /// conservatively.
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
        self.rows = plan_to_editor(&plan.entries);
        self.template_id = plan.template_id.clone();
    }

    /// Replaces the rows with all the calendar participants of the meeting, every one included.
    /// Rows added by hand are dropped and the template id is kept.
    ///
    /// Edge cases: a meeting without participants gives no rows.
    pub fn reset_participants(&mut self) {
        let equal = build_plan(&participants(&self.meeting), None);
        self.rows = plan_to_editor(&equal);
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

    /// Includes or excludes the attendee of row `index` from the meeting.
    ///
    /// Edge cases: a row that is not an attendee is always included, so it is left unchanged;
    /// an index out of range is logged and ignored.
    pub fn set_included(&mut self, index: usize, included: bool) {
        match self.rows.get_mut(index) {
            Some(row) if row.kind == PersonKind::Attendee => row.included = included,
            Some(_) => debug!("set_included: row {index} is not an attendee"),
            None => debug!("set_included: ignoring out of range row {index}"),
        }
    }

    /// Appends an included manual row with an empty label, for the user to type a name.
    ///
    /// Edge cases: nothing is added when the editor already holds [`MAX_ROWS`] rows.
    pub fn add_manual_row(&mut self) {
        if self.rows.len() >= MAX_ROWS {
            debug!("add_manual_row: already {MAX_ROWS} rows");
            return;
        }
        self.rows.push(EditorEntry {
            kind: PersonKind::Manual,
            email: None,
            label: String::new(),
            included: true,
        });
    }

    /// Removes row `index` when it was added by hand.
    ///
    /// Edge cases: attendee rows (they can only be excluded) and an index out of range are left
    /// unchanged.
    pub fn remove_row(&mut self, index: usize) {
        match self.rows.get(index) {
            Some(row) if row.kind == PersonKind::Manual => {
                self.rows.remove(index);
            }
            Some(_) => debug!("remove_row: row {index} is not a manual row"),
            None => debug!("remove_row: ignoring out of range row {index}"),
        }
    }

    /// Request that saves the rows as the template of the meeting title.
    ///
    /// The entries come from [`editor_to_template`]: included rows share one weight, excluded
    /// rows weigh 0 and blank manual rows are left out.
    pub fn save_request(&self) -> PutTemplateRequest {
        PutTemplateRequest {
            meeting_title: self.meeting.title.clone(),
            entries: editor_to_template(&self.rows),
        }
    }

    /// Names to start the meeting with: the labels of the included rows, trimmed, in row order.
    ///
    /// Edge cases: rows with a blank label are skipped, so a meeting without usable rows gives
    /// an empty list. Two rows with the same name give the name twice.
    pub fn start_names(&self) -> Vec<String> {
        let mut names = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let name = row.label.trim();
            if row.included && !name.is_empty() {
                names.push(name.to_string());
            }
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, FixedOffset, TimeZone};
    use schisaciaccole_core::api::{Attendee, EntryKind, PlanEntry, PlanSource, ResponseStatus};
    use schisaciaccole_core::{FREE_TIME_LABEL, PARTICIPANT_WEIGHT_SECS};

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
            preference: None,
        }
    }

    /// Editor on the calendar participants Ann, Bob and Carl, all included.
    fn calendar_editor() -> ParticipantEditor {
        let mut editor = ParticipantEditor::new(meeting(&["Carl", "Ann", "Bob"]));
        editor.reset_participants();
        editor
    }

    fn labels(editor: &ParticipantEditor) -> Vec<&str> {
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

    fn plan_of(entries: Vec<PlanEntry>) -> SlotPlan {
        SlotPlan {
            attendees: None,
            attendees_omitted: false,
            participant_resolution: Default::default(),
            source: PlanSource::Template,
            template_id: Some("t1".to_string()),
            entries,
        }
    }

    #[test]
    fn new_editor_is_empty_with_the_nominal_duration() {
        let editor = ParticipantEditor::new(meeting(&["Ann"]));

        assert!(editor.rows().is_empty());
        assert_eq!(editor.template_id(), None);
        assert_eq!(editor.duration_minutes(), 60);
        assert_eq!(editor.event_id(), "ev1");
    }

    #[test]
    fn meeting_ending_before_its_start_has_no_duration() {
        let mut backwards = meeting(&["Ann"]);
        backwards.end = at(8, 0);

        let editor = ParticipantEditor::new(backwards);

        assert_eq!(editor.duration_minutes(), 0);
    }

    #[test]
    fn duration_rounds_partial_minutes_down() {
        let mut odd = meeting(&["Ann"]);
        odd.end = at(9, 30) + chrono::Duration::seconds(59);

        let editor = ParticipantEditor::new(odd);

        assert_eq!(editor.duration_minutes(), 30);
    }

    #[test]
    fn reset_lists_every_calendar_participant_included() {
        let editor = calendar_editor();

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
        assert!(editor.rows().iter().all(|row| row.included));
    }

    #[test]
    fn reset_restores_excluded_participants() {
        let mut editor = calendar_editor();
        editor.set_included(1, false);

        editor.reset_participants();

        assert!(editor.rows().iter().all(|row| row.included));
    }

    #[test]
    fn reset_drops_rows_added_by_hand() {
        let mut editor = calendar_editor();
        editor.add_manual_row();

        editor.reset_participants();

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn reset_keeps_the_template_id() {
        let mut editor = calendar_editor();
        editor.set_template_id(Some("t1".to_string()));

        editor.reset_participants();

        assert_eq!(editor.template_id(), Some("t1"));
    }

    #[test]
    fn loading_a_plan_fills_rows_and_template_id() {
        let mut editor = ParticipantEditor::new(meeting(&["Ann", "Bob"]));
        let plan = plan_of(vec![
            plan_entry(EntryKind::Attendee, "Ann", 2),
            plan_entry(EntryKind::Attendee, "Bob", 0),
        ]);

        editor.load_plan(&plan);

        assert_eq!(labels(&editor), vec!["Ann", "Bob"]);
        assert!(editor.rows()[0].included);
        assert!(!editor.rows()[1].included);
        assert_eq!(editor.template_id(), Some("t1"));
    }

    #[test]
    fn loading_a_plan_skips_free_time() {
        let mut editor = ParticipantEditor::new(meeting(&["Ann"]));
        let plan = plan_of(vec![
            plan_entry(EntryKind::Attendee, "Ann", 2),
            plan_entry(EntryKind::FreeTime, FREE_TIME_LABEL, 1),
        ]);

        editor.load_plan(&plan);

        assert_eq!(labels(&editor), vec!["Ann"]);
    }

    #[test]
    fn set_label_keeps_the_text_as_typed() {
        let mut editor = calendar_editor();

        editor.set_label(1, " Roberto ");

        assert_eq!(editor.rows()[1].label, " Roberto ");
    }

    #[test]
    fn set_label_out_of_range_changes_nothing() {
        let mut editor = calendar_editor();

        editor.set_label(3, "X");

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn attendee_can_be_excluded_and_included_again() {
        let mut editor = calendar_editor();

        editor.set_included(1, false);
        assert!(!editor.rows()[1].included);

        editor.set_included(1, true);
        assert!(editor.rows()[1].included);
    }

    #[test]
    fn manual_row_cannot_be_excluded() {
        let mut editor = calendar_editor();
        editor.add_manual_row();

        editor.set_included(3, false);

        assert!(editor.rows()[3].included);
    }

    #[test]
    fn set_included_out_of_range_changes_nothing() {
        let mut editor = calendar_editor();

        editor.set_included(3, false);

        assert!(editor.rows().iter().all(|row| row.included));
    }

    #[test]
    fn added_row_is_an_included_manual_row_with_an_empty_name() {
        let mut editor = calendar_editor();

        editor.add_manual_row();

        let added = editor.rows().last().unwrap();
        assert_eq!(added.kind, PersonKind::Manual);
        assert_eq!(added.label, "");
        assert!(added.included);
        assert_eq!(added.email, None);
    }

    #[test]
    fn rows_stop_at_the_limit() {
        let mut editor = calendar_editor();

        for _ in 0..MAX_ROWS {
            editor.add_manual_row();
        }

        assert_eq!(editor.rows().len(), MAX_ROWS);
    }

    #[test]
    fn manual_row_can_be_removed() {
        let mut editor = calendar_editor();
        editor.add_manual_row();

        editor.remove_row(3);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn attendee_row_cannot_be_removed() {
        let mut editor = calendar_editor();

        editor.remove_row(0);

        assert_eq!(labels(&editor), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn remove_out_of_range_changes_nothing() {
        let mut editor = calendar_editor();

        editor.remove_row(3);

        assert_eq!(editor.rows().len(), 3);
    }

    #[test]
    fn save_request_has_the_title_and_a_weight_per_row() {
        let mut editor = calendar_editor();
        editor.set_included(1, false);

        let request = editor.save_request();

        assert_eq!(request.meeting_title, "Daily");
        let weights: Vec<u32> = request.entries.iter().map(|e| e.weight_secs).collect();
        assert_eq!(
            weights,
            vec![PARTICIPANT_WEIGHT_SECS, 0, PARTICIPANT_WEIGHT_SECS]
        );
    }

    #[test]
    fn save_request_has_no_free_time_entry() {
        let editor = calendar_editor();

        let request = editor.save_request();

        assert!(
            request
                .entries
                .iter()
                .all(|entry| entry.kind == EntryKind::Attendee)
        );
    }

    #[test]
    fn save_request_skips_a_blank_manual_row() {
        let mut editor = calendar_editor();
        editor.add_manual_row();

        let request = editor.save_request();

        assert_eq!(request.entries.len(), 3);
    }

    #[test]
    fn save_request_keeps_a_named_manual_row() {
        let mut editor = calendar_editor();
        editor.add_manual_row();
        editor.set_label(3, " Dario ");

        let request = editor.save_request();

        let last = request.entries.last().unwrap();
        assert_eq!(last.kind, EntryKind::Manual);
        assert_eq!(last.label, "Dario");
    }

    #[test]
    fn start_names_are_the_included_rows_in_order() {
        let editor = calendar_editor();

        assert_eq!(editor.start_names(), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn start_names_skip_excluded_rows() {
        let mut editor = calendar_editor();
        editor.set_included(1, false);

        assert_eq!(editor.start_names(), vec!["Ann", "Carl"]);
    }

    #[test]
    fn start_names_skip_blank_rows() {
        let mut editor = calendar_editor();
        editor.add_manual_row();
        editor.add_manual_row();
        editor.set_label(4, "   ");

        assert_eq!(editor.start_names(), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn start_names_are_trimmed_and_follow_the_row_order() {
        let mut editor = calendar_editor();
        editor.add_manual_row();
        editor.set_label(3, " Dario ");
        editor.set_label(0, "Anna");

        assert_eq!(editor.start_names(), vec!["Anna", "Bob", "Carl", "Dario"]);
    }

    #[test]
    fn start_names_of_an_empty_editor_are_empty() {
        let editor = ParticipantEditor::new(meeting(&["Ann"]));

        assert!(editor.start_names().is_empty());
    }

    #[test]
    fn fresh_empty_group_stays_empty_on_reset() {
        let mut editor = calendar_editor();
        let plan = SlotPlan {
            source: PlanSource::Equal,
            template_id: None,
            entries: Vec::new(),
            attendees: Some(Vec::new()),
            attendees_omitted: false,
            participant_resolution: Default::default(),
        };

        editor.load_plan(&plan);
        editor.reset_participants();

        assert!(editor.rows().is_empty());
        assert!(editor.meeting().attendees.is_empty());
    }

    #[test]
    fn legacy_plan_preserves_participants_and_prior_warning() {
        let mut meeting = meeting(&["Ann"]);
        meeting.attendees_omitted = true;
        let mut editor = ParticipantEditor::new(meeting);
        let plan = SlotPlan {
            source: PlanSource::Equal,
            template_id: None,
            entries: Vec::new(),
            attendees: None,
            attendees_omitted: false,
            participant_resolution: Default::default(),
        };

        editor.load_plan(&plan);
        editor.reset_participants();

        assert_eq!(labels(&editor), vec!["Ann"]);
        assert!(editor.meeting().attendees_omitted);
    }
}
