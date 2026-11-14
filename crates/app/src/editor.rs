//! Participant editor: who takes part in one meeting and the operations the editor page offers.
//!
//! Plain data without Slint types. A [`Draft`] holds the rows of one meeting and survives the
//! editor page: [`crate::state::AppState`] keeps one per calendar event and applies every editor
//! operation to it. A [`ParticipantEditor`] is the meeting snapshot the open page shows. Row
//! indices come from the UI, so every operation ignores an index out of range instead of failing.

use log::debug;
use schisaciaccole_core::api::{Meeting, MeetingPreference, PersonKind, PreferencePerson};
use schisaciaccole_core::{
    EditorEntry, editor_rows, merge_rows, participants, preference_people, start_names,
};

/// Most rows the editor holds: adding a row beyond it does nothing.
pub const MAX_ROWS: usize = 99;

/// True when Calendar omitted attendees or Google Groups could not be fully resolved.
///
/// Complete empty groups have no warning; reauthorization alone also requires attention.
pub fn participants_incomplete(meeting: &Meeting) -> bool {
    meeting.attendees_omitted
        || meeting.participant_resolution.incomplete
        || meeting.participant_resolution.reauthorization_required
}

/// Preference people of `meeting`, when it has a saved preference.
fn saved_people(meeting: &Meeting) -> Option<&[PreferencePerson]> {
    meeting
        .preference
        .as_ref()
        .map(|preference| preference.people.as_slice())
}

/// Editor rows of `meeting` when nobody edited them: the Calendar participants and the people of
/// the saved preference, all included (see [`editor_rows`]).
///
/// Edge cases: a meeting without participants and without preference gives no rows.
pub fn default_rows(meeting: &Meeting) -> Vec<EditorEntry> {
    editor_rows(&participants(meeting), saved_people(meeting))
}

/// Meeting shown by the open editor page.
#[derive(Debug, Clone)]
pub struct ParticipantEditor {
    meeting: Meeting,
    /// Nominal duration of the meeting (end minus start), never negative.
    duration_secs: i64,
}

impl ParticipantEditor {
    /// Creates an editor for `meeting`.
    ///
    /// Edge cases: a meeting that ends before it starts gets a duration of 0 seconds.
    pub fn new(meeting: Meeting) -> Self {
        let duration_secs = nominal_secs(&meeting);
        Self {
            meeting,
            duration_secs,
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

    /// Nominal duration of the meeting in whole minutes (rounded down).
    pub fn duration_minutes(&self) -> i64 {
        self.duration_secs / 60
    }

    /// Replaces the meeting snapshot and the duration derived from it.
    ///
    /// Edge cases: the event id is not checked; the caller passes the same event.
    pub fn replace_meeting(&mut self, meeting: Meeting) {
        self.duration_secs = nominal_secs(&meeting);
        self.meeting = meeting;
    }

    /// Replaces the preference of the snapshot, `None` when it was deleted.
    pub fn set_preference(&mut self, preference: Option<MeetingPreference>) {
        self.meeting.preference = preference;
    }
}

/// Nominal duration of `meeting` in seconds, never negative.
fn nominal_secs(meeting: &Meeting) -> i64 {
    meeting
        .end
        .signed_duration_since(meeting.start)
        .num_seconds()
        .max(0)
}

/// Rows of one meeting kept in memory while the app runs.
///
/// `complete` tells whether the participant list the rows come from was complete (see
/// [`participants_incomplete`]); [`Draft::merge_fresh`] needs it to avoid losing exclusions
/// during a temporary group expansion failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    rows: Vec<EditorEntry>,
    complete: bool,
}

impl Draft {
    /// Creates the draft of `meeting`: [`default_rows`], complete when the participant list is.
    pub fn from_meeting(meeting: &Meeting) -> Self {
        Self {
            rows: default_rows(meeting),
            complete: !participants_incomplete(meeting),
        }
    }

    /// Rows in display order.
    pub fn rows(&self) -> &[EditorEntry] {
        &self.rows
    }

    /// True when the participant list of the last creation, reset or merge was complete.
    #[cfg(test)]
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Rebuilds the draft from `meeting` as [`Draft::from_meeting`] does, dropping every edit.
    pub fn reset(&mut self, meeting: &Meeting) {
        *self = Self::from_meeting(meeting);
    }

    /// Merges the participants of `fresh`, the meeting just read, into the rows.
    ///
    /// A complete draft is left unchanged when the fresh list is incomplete. Otherwise the rows
    /// go through [`merge_rows`] (the saved preference only names new attendees) and the draft
    /// takes the completeness of `fresh`.
    ///
    /// Edge cases: nothing is merged for an incomplete fresh list over a complete draft, even if
    /// attendees were added; they enter on the first complete list.
    pub fn merge_fresh(&mut self, fresh: &Meeting) {
        let fresh_complete = !participants_incomplete(fresh);
        if self.complete && !fresh_complete {
            debug!("merge_fresh: fresh list incomplete, keeping the draft");
            return;
        }
        self.rows = merge_rows(&self.rows, &participants(fresh), saved_people(fresh));
        self.complete = fresh_complete;
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

    /// Includes or excludes row `index` from the meeting, whatever its kind.
    ///
    /// Edge cases: an index out of range is logged and ignored.
    pub fn set_included(&mut self, index: usize, included: bool) {
        match self.rows.get_mut(index) {
            Some(row) => row.included = included,
            None => debug!("set_included: ignoring out of range row {index}"),
        }
    }

    /// Appends an included manual row with an empty label, for the user to type a name.
    ///
    /// Edge cases: nothing is added when the draft already holds [`MAX_ROWS`] rows.
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

    /// People to save as the preference of `meeting`: see [`preference_people`], with the
    /// preference already saved on `meeting` as the carried-over renames.
    ///
    /// Edge cases: an empty result means there is nothing to save; the server rejects it.
    pub fn preference_people(&self, meeting: &Meeting) -> Vec<PreferencePerson> {
        preference_people(&self.rows, &participants(meeting), saved_people(meeting))
    }

    /// Names to start the meeting with: see [`start_names`].
    ///
    /// Edge cases: rows with a blank label are skipped, so a draft without usable rows gives an
    /// empty list. Two rows with the same name give the name twice.
    pub fn start_names(&self) -> Vec<String> {
        start_names(&self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::blank_meeting;
    use chrono::{DateTime, FixedOffset, TimeZone};
    use schisaciaccole_core::api::{Attendee, ResponseStatus};

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
        let mut meeting = blank_meeting("ev1", "Daily", at(9, 0), at(10, 0));
        meeting.attendees = names.iter().map(|name| attendee(name)).collect();
        meeting
    }

    fn preference(people: Vec<PreferencePerson>) -> MeetingPreference {
        MeetingPreference {
            id: "p1".to_string(),
            meeting_title: "Daily".to_string(),
            people,
        }
    }

    fn rename(email: &str, label: &str) -> PreferencePerson {
        PreferencePerson {
            kind: PersonKind::Attendee,
            email: Some(email.to_string()),
            label: label.to_string(),
        }
    }

    fn manual(label: &str) -> PreferencePerson {
        PreferencePerson {
            kind: PersonKind::Manual,
            email: None,
            label: label.to_string(),
        }
    }

    /// Draft on the calendar participants Ann, Bob and Carl, all included.
    fn calendar_draft() -> Draft {
        Draft::from_meeting(&meeting(&["Carl", "Ann", "Bob"]))
    }

    fn labels(draft: &Draft) -> Vec<&str> {
        draft.rows().iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn new_editor_has_the_nominal_duration() {
        let editor = ParticipantEditor::new(meeting(&["Ann"]));

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
    fn replacing_the_meeting_updates_the_duration() {
        let mut editor = ParticipantEditor::new(meeting(&["Ann"]));
        let mut longer = meeting(&["Ann"]);
        longer.end = at(11, 0);

        editor.replace_meeting(longer);

        assert_eq!(editor.duration_minutes(), 120);
    }

    #[test]
    fn draft_lists_every_calendar_participant_included() {
        let draft = calendar_draft();

        assert_eq!(labels(&draft), vec!["Ann", "Bob", "Carl"]);
        assert!(draft.rows().iter().all(|row| row.included));
    }

    #[test]
    fn draft_applies_the_preference_renames_and_manual_people() {
        let mut talk = meeting(&["Ann", "Bob"]);
        talk.preference = Some(preference(vec![
            rename("bob@x.it", "Roberto"),
            manual("Marco"),
        ]));

        let draft = Draft::from_meeting(&talk);

        assert_eq!(labels(&draft), vec!["Ann", "Roberto", "Marco"]);
        assert!(draft.rows().iter().all(|row| row.included));
    }

    #[test]
    fn draft_of_a_complete_list_is_complete() {
        assert!(calendar_draft().is_complete());
    }

    #[test]
    fn draft_of_an_incomplete_list_is_not_complete() {
        let mut talk = meeting(&["Ann"]);
        talk.participant_resolution.incomplete = true;

        assert!(!Draft::from_meeting(&talk).is_complete());
    }

    #[test]
    fn reset_restores_excluded_participants() {
        let talk = meeting(&["Carl", "Ann", "Bob"]);
        let mut draft = Draft::from_meeting(&talk);
        draft.set_included(1, false);

        draft.reset(&talk);

        assert!(draft.rows().iter().all(|row| row.included));
    }

    #[test]
    fn reset_drops_rows_added_by_hand() {
        let talk = meeting(&["Carl", "Ann", "Bob"]);
        let mut draft = Draft::from_meeting(&talk);
        draft.add_manual_row();

        draft.reset(&talk);

        assert_eq!(labels(&draft), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn reset_takes_the_completeness_of_the_meeting() {
        let mut talk = meeting(&["Ann"]);
        let mut draft = Draft::from_meeting(&talk);
        talk.attendees_omitted = true;

        draft.reset(&talk);

        assert!(!draft.is_complete());
    }

    #[test]
    fn merge_adds_a_new_attendee_included_and_drops_a_removed_one() {
        let mut draft = Draft::from_meeting(&meeting(&["Ann", "Bob", "Carl"]));
        draft.set_included(1, false);
        draft.add_manual_row();
        draft.set_label(3, "Marco");

        draft.merge_fresh(&meeting(&["Ann", "Bob", "Dario"]));

        assert_eq!(labels(&draft), vec!["Ann", "Bob", "Dario", "Marco"]);
        let included: Vec<bool> = draft.rows().iter().map(|row| row.included).collect();
        assert_eq!(included, vec![true, false, true, true]);
    }

    #[test]
    fn merge_of_an_incomplete_list_keeps_a_complete_draft() {
        let mut draft = Draft::from_meeting(&meeting(&["Ann", "Bob"]));
        draft.set_included(0, false);
        let before = draft.clone();
        let mut fresh = meeting(&["Dario"]);
        fresh.participant_resolution.incomplete = true;

        draft.merge_fresh(&fresh);

        assert_eq!(draft, before);
    }

    #[test]
    fn merge_of_a_complete_list_replaces_an_incomplete_draft() {
        let mut stale = meeting(&["Group"]);
        stale.participant_resolution.incomplete = true;
        let mut draft = Draft::from_meeting(&stale);

        draft.merge_fresh(&meeting(&["Ann", "Bob"]));

        assert_eq!(labels(&draft), vec!["Ann", "Bob"]);
        assert!(draft.is_complete());
    }

    #[test]
    fn merge_of_an_incomplete_list_merges_into_an_incomplete_draft() {
        let mut stale = meeting(&["Group"]);
        stale.participant_resolution.incomplete = true;
        let mut draft = Draft::from_meeting(&stale);
        let mut fresh = meeting(&["Other"]);
        fresh.attendees_omitted = true;

        draft.merge_fresh(&fresh);

        assert_eq!(labels(&draft), vec!["Other"]);
        assert!(!draft.is_complete());
    }

    #[test]
    fn set_label_keeps_the_text_as_typed() {
        let mut draft = calendar_draft();

        draft.set_label(1, " Roberto ");

        assert_eq!(draft.rows()[1].label, " Roberto ");
    }

    #[test]
    fn set_label_out_of_range_changes_nothing() {
        let mut draft = calendar_draft();

        draft.set_label(3, "X");

        assert_eq!(labels(&draft), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn attendee_can_be_excluded_and_included_again() {
        let mut draft = calendar_draft();

        draft.set_included(1, false);
        assert!(!draft.rows()[1].included);

        draft.set_included(1, true);
        assert!(draft.rows()[1].included);
    }

    #[test]
    fn manual_row_can_be_excluded() {
        let mut draft = calendar_draft();
        draft.add_manual_row();

        draft.set_included(3, false);

        assert!(!draft.rows()[3].included);
    }

    #[test]
    fn set_included_out_of_range_changes_nothing() {
        let mut draft = calendar_draft();

        draft.set_included(3, false);

        assert!(draft.rows().iter().all(|row| row.included));
    }

    #[test]
    fn added_row_is_an_included_manual_row_with_an_empty_name() {
        let mut draft = calendar_draft();

        draft.add_manual_row();

        let added = draft.rows().last().unwrap();
        assert_eq!(added.kind, PersonKind::Manual);
        assert_eq!(added.label, "");
        assert!(added.included);
        assert_eq!(added.email, None);
    }

    #[test]
    fn rows_stop_at_the_limit() {
        let mut draft = calendar_draft();

        for _ in 0..MAX_ROWS {
            draft.add_manual_row();
        }

        assert_eq!(draft.rows().len(), MAX_ROWS);
    }

    #[test]
    fn manual_row_can_be_removed() {
        let mut draft = calendar_draft();
        draft.add_manual_row();

        draft.remove_row(3);

        assert_eq!(labels(&draft), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn attendee_row_cannot_be_removed() {
        let mut draft = calendar_draft();

        draft.remove_row(0);

        assert_eq!(labels(&draft), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn remove_out_of_range_changes_nothing() {
        let mut draft = calendar_draft();

        draft.remove_row(3);

        assert_eq!(draft.rows().len(), 3);
    }

    #[test]
    fn preference_people_has_renames_and_named_manual_rows_not_exclusions() {
        let talk = meeting(&["Carl", "Ann", "Bob"]);
        let mut draft = Draft::from_meeting(&talk);
        draft.set_label(1, "Roberto");
        draft.set_included(0, false);
        draft.add_manual_row();
        draft.set_label(3, " Dario ");
        draft.add_manual_row();

        let people = draft.preference_people(&talk);

        assert_eq!(people, vec![rename("bob@x.it", "Roberto"), manual("Dario")]);
    }

    #[test]
    fn preference_people_of_untouched_calendar_rows_is_empty() {
        let talk = meeting(&["Ann", "Bob"]);

        assert!(
            Draft::from_meeting(&talk)
                .preference_people(&talk)
                .is_empty()
        );
    }

    #[test]
    fn start_names_are_the_included_rows_in_order() {
        assert_eq!(calendar_draft().start_names(), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn start_names_skip_excluded_rows_manual_ones_included() {
        let mut draft = calendar_draft();
        draft.add_manual_row();
        draft.set_label(3, "Dario");
        draft.set_included(1, false);
        draft.set_included(3, false);

        assert_eq!(draft.start_names(), vec!["Ann", "Carl"]);
    }

    #[test]
    fn start_names_skip_blank_rows() {
        let mut draft = calendar_draft();
        draft.add_manual_row();
        draft.set_label(3, "   ");

        assert_eq!(draft.start_names(), vec!["Ann", "Bob", "Carl"]);
    }

    #[test]
    fn start_names_are_trimmed_and_follow_the_row_order() {
        let mut draft = calendar_draft();
        draft.add_manual_row();
        draft.set_label(3, " Dario ");
        draft.set_label(0, "Anna");

        assert_eq!(draft.start_names(), vec!["Anna", "Bob", "Carl", "Dario"]);
    }

    #[test]
    fn default_rows_of_a_meeting_without_participants_are_empty() {
        assert!(default_rows(&meeting(&[])).is_empty());
    }
}
