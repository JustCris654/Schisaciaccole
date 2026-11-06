//! Slot plans built from saved templates, and the conversions used by the minutes editor.
//!
//! A template stores weights in seconds per entry. A plan applies a template to the people who
//! are actually in a meeting. The editor works in whole minutes over the nominal meeting
//! duration and shows the time not given to anybody as unassigned. Nothing here reads the clock.

use crate::api::{EntryKind, PlanEntry, TemplateEntry};
use crate::attendee::display_name_from_email;
use crate::slots::{Participant, allocate_ms};
use std::collections::HashSet;

/// Label of the entry that holds the time not assigned to anybody.
pub const FREE_TIME_LABEL: &str = "Tempo libero";

/// Label given to a non-attendee row left without a name.
const DEFAULT_ROW_LABEL: &str = "Voce";

/// One row of the minutes editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEntry {
    /// Kind of the row; never [`EntryKind::FreeTime`], the editor shows unassigned time instead.
    pub kind: EntryKind,
    /// Email of the attendee; `Some` only for [`EntryKind::Attendee`].
    pub email: Option<String>,
    /// Label shown for the row.
    pub label: String,
    /// Assigned minutes; meaningful only when `included` is true.
    pub minutes: u32,
    /// False when the participant is excluded from the meeting slots.
    pub included: bool,
}

/// Builds the key used to compare an email: trimmed and lowercase (ASCII).
fn email_key(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Normalizes a meeting title into the key that identifies its template.
///
/// Trims the title, collapses every run of Unicode whitespace into one ASCII space and lowercases
/// the result with Unicode rules, so `"SmartBIP | Retrospective"` and
/// `"  smartbip |  retrospective "` give the same key.
///
/// Edge cases: an empty or whitespace-only title gives an empty string. Accents are kept; only
/// the case is folded.
pub fn normalize_title(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().collect();
    words.join(" ").to_lowercase()
}

/// Computes the weight given to a participant who has no entry in the template.
///
/// It is the integer mean (floor) of the weights above zero of all the template's attendee
/// entries, including the ones of people who are not in the meeting. When there is none, it is
/// the same mean over all entries of the template; when there is none either, it is 1.
/// The result is never below 1.
fn new_participant_weight(template: &[TemplateEntry]) -> u64 {
    let attendee_mean = mean_of_positive(
        template
            .iter()
            .filter(|entry| entry.kind == EntryKind::Attendee),
    );
    let mean = match attendee_mean {
        Some(mean) => mean,
        None => mean_of_positive(template.iter()).unwrap_or(1),
    };
    mean.max(1)
}

/// Integer mean (floor) of the weights above zero, or `None` when there are none.
fn mean_of_positive<'a>(entries: impl Iterator<Item = &'a TemplateEntry>) -> Option<u64> {
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for entry in entries {
        if entry.weight_secs > 0 {
            sum = sum.saturating_add(u64::from(entry.weight_secs));
            count += 1;
        }
    }
    sum.checked_div(count)
}

/// Builds the slot plan of a meeting.
///
/// Without a template, every participant gets one attendee entry with weight 1, in the given
/// order, labelled with the participant name.
///
/// With a template (entries already in saved position order):
/// - an attendee entry whose email matches a participant (trimmed, ASCII case-insensitive) is
///   kept in place with its saved label and weight, a weight of 0 included (excluded person);
/// - when the same email appears more than once, only the first entry is used;
/// - an attendee entry without email, or without a matching participant, is dropped;
/// - manual and free time entries are always kept;
/// - participants without an entry are added in participant order after the last entry that is
///   not free time, so free time stays at the end. They are labelled with the participant name
///   and get the mean weight of the template's attendees, see `new_participant_weight`.
///
/// Edge cases: no participants and no template, or an empty template without participants, give
/// an empty plan. Two participants with the same email both count as matched by one entry.
/// Weights are not checked: a plan whose weights are all 0 is returned as is.
pub fn build_plan(
    participants: &[Participant],
    template: Option<&[TemplateEntry]>,
) -> Vec<PlanEntry> {
    let Some(template) = template else {
        return participants.iter().map(equal_entry).collect();
    };

    let participant_keys: HashSet<String> = participants
        .iter()
        .map(|participant| email_key(&participant.email))
        .collect();
    let mut used_keys: HashSet<String> = HashSet::new();
    let mut plan: Vec<PlanEntry> = Vec::with_capacity(template.len() + participants.len());

    for entry in template {
        if entry.kind == EntryKind::Attendee {
            let Some(email) = entry.email.as_deref() else {
                continue;
            };
            let key = email_key(email);
            if !participant_keys.contains(&key) || !used_keys.insert(key) {
                continue;
            }
        }
        plan.push(template_entry_to_plan(entry));
    }

    let weight = new_participant_weight(template);
    let mut insert_at = plan
        .iter()
        .rposition(|entry| entry.kind != EntryKind::FreeTime)
        .map_or(0, |last| last + 1);
    for participant in participants {
        if used_keys.contains(&email_key(&participant.email)) {
            continue;
        }
        plan.insert(
            insert_at,
            PlanEntry {
                kind: EntryKind::Attendee,
                email: Some(participant.email.clone()),
                label: participant.name.clone(),
                weight,
            },
        );
        insert_at += 1;
    }
    plan
}

/// Entry of an equal plan: weight 1 for the participant.
fn equal_entry(participant: &Participant) -> PlanEntry {
    PlanEntry {
        kind: EntryKind::Attendee,
        email: Some(participant.email.clone()),
        label: participant.name.clone(),
        weight: 1,
    }
}

/// Converts one saved entry into a plan entry.
fn template_entry_to_plan(entry: &TemplateEntry) -> PlanEntry {
    PlanEntry {
        kind: entry.kind,
        email: entry.email.clone(),
        label: entry.label.clone(),
        weight: u64::from(entry.weight_secs),
    }
}

/// Converts template entries into plan entries one to one, keeping order and weights.
///
/// Used to start a timer from editor rows without saving them. It does not match entries against
/// participants and does not drop anything.
pub fn template_to_plan(entries: &[TemplateEntry]) -> Vec<PlanEntry> {
    entries.iter().map(template_entry_to_plan).collect()
}

/// Rounds seconds to the nearest whole minute, halves going up, as `u32` minutes.
fn rounded_minutes(secs: i64) -> u32 {
    let minutes = secs.saturating_add(30) / 60;
    u32::try_from(minutes).unwrap_or(0)
}

/// Converts a plan into editor rows over a meeting of `duration_secs` seconds.
///
/// The whole duration is split among all entries by weight with [`allocate_ms`] (used on
/// seconds). For every entry that is not free time, the row is `included` when its weight is
/// above 0 and then gets its share rounded to the nearest minute (half up); an excluded row gets
/// 0 minutes. Free time entries are skipped: their time shows up as unassigned.
///
/// Edge cases: when the split is impossible (`duration_secs <= 0`, no entries or all weights 0)
/// every row gets 0 minutes and `included` still follows the weight. Rounding can make the sum
/// of minutes differ from the duration by a few minutes, see [`unassigned_minutes`].
pub fn plan_to_editor(entries: &[PlanEntry], duration_secs: i64) -> Vec<EditorEntry> {
    let weights: Vec<u64> = entries.iter().map(|entry| entry.weight).collect();
    let shares = allocate_ms(&weights, duration_secs);

    let mut rows = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        if entry.kind == EntryKind::FreeTime {
            continue;
        }
        let included = entry.weight > 0;
        let share = shares
            .as_ref()
            .and_then(|shares| shares.get(position))
            .copied()
            .unwrap_or(0);
        let minutes = if included { rounded_minutes(share) } else { 0 };
        rows.push(EditorEntry {
            kind: entry.kind,
            email: entry.email.clone(),
            label: entry.label.clone(),
            minutes,
            included,
        });
    }
    rows
}

/// Returns the minutes of the meeting not assigned to any included row.
///
/// It is `duration_secs / 60` (floor) minus the sum of the minutes of the included rows.
/// A negative result means the rows take more time than the meeting has.
///
/// Edge cases: a negative duration gives a negative result; sums saturate instead of
/// overflowing.
pub fn unassigned_minutes(rows: &[EditorEntry], duration_secs: i64) -> i64 {
    let mut assigned: i64 = 0;
    for row in rows {
        if row.included {
            assigned = assigned.saturating_add(i64::from(row.minutes));
        }
    }
    (duration_secs / 60).saturating_sub(assigned)
}

/// Converts editor rows into template entries to save.
///
/// Every row becomes one entry in order, with `weight_secs = minutes * 60` when included and 0
/// otherwise. The email is kept only for attendees. Labels are trimmed; an empty label becomes
/// the name built from the email with [`display_name_from_email`] for attendees, or `"Voce"`
/// otherwise (also for an attendee without email). When `duration_secs` minus the assigned
/// seconds is above 0, a free time entry labelled [`FREE_TIME_LABEL`] with that difference as
/// weight is added at the end.
///
/// Edge cases: over-assigned rows or a non-positive duration add no free time entry. Seconds
/// saturate at `u32::MAX` instead of overflowing.
pub fn editor_to_template(rows: &[EditorEntry], duration_secs: i64) -> Vec<TemplateEntry> {
    let mut entries = Vec::with_capacity(rows.len() + 1);
    let mut assigned: i64 = 0;
    for row in rows {
        let weight_secs = if row.included {
            row.minutes.saturating_mul(60)
        } else {
            0
        };
        assigned = assigned.saturating_add(i64::from(weight_secs));
        let email = match row.kind {
            EntryKind::Attendee => row.email.clone(),
            _ => None,
        };
        entries.push(TemplateEntry {
            kind: row.kind,
            label: row_label(row),
            email,
            weight_secs,
        });
    }

    let free_secs = duration_secs.saturating_sub(assigned);
    if free_secs > 0 {
        entries.push(TemplateEntry {
            kind: EntryKind::FreeTime,
            email: None,
            label: FREE_TIME_LABEL.to_string(),
            weight_secs: u32::try_from(free_secs).unwrap_or(u32::MAX),
        });
    }
    entries
}

/// Returns the trimmed label of a row, or a fallback name when it is empty.
fn row_label(row: &EditorEntry) -> String {
    let label = row.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    if row.kind == EntryKind::Attendee {
        let name = display_name_from_email(row.email.as_deref().unwrap_or(""));
        if !name.is_empty() {
            return name;
        }
    }
    DEFAULT_ROW_LABEL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, email: &str) -> Participant {
        Participant {
            name: name.to_string(),
            email: email.to_string(),
        }
    }

    fn attendee(label: &str, email: &str, secs: u32) -> TemplateEntry {
        TemplateEntry {
            kind: EntryKind::Attendee,
            email: Some(email.to_string()),
            label: label.to_string(),
            weight_secs: secs,
        }
    }

    fn manual(label: &str, secs: u32) -> TemplateEntry {
        TemplateEntry {
            kind: EntryKind::Manual,
            email: None,
            label: label.to_string(),
            weight_secs: secs,
        }
    }

    fn free(secs: u32) -> TemplateEntry {
        TemplateEntry {
            kind: EntryKind::FreeTime,
            email: None,
            label: FREE_TIME_LABEL.to_string(),
            weight_secs: secs,
        }
    }

    fn row(label: &str, email: &str, minutes: u32, included: bool) -> EditorEntry {
        EditorEntry {
            kind: EntryKind::Attendee,
            email: Some(email.to_string()),
            label: label.to_string(),
            minutes,
            included,
        }
    }

    fn four_people() -> Vec<Participant> {
        vec![
            person("Anna", "anna@x.it"),
            person("Bruno", "bruno@x.it"),
            person("Carla", "carla@x.it"),
            person("Dario", "dario@x.it"),
        ]
    }

    fn design_doc_template() -> Vec<TemplateEntry> {
        vec![
            attendee("Anna", "anna@x.it", 1500),
            attendee("Bruno", "bruno@x.it", 300),
            attendee("Carla", "carla@x.it", 300),
            attendee("Dario", "dario@x.it", 300),
            free(1200),
        ]
    }

    fn weights(plan: &[PlanEntry]) -> Vec<u64> {
        plan.iter().map(|entry| entry.weight).collect()
    }

    fn labels(plan: &[PlanEntry]) -> Vec<&str> {
        plan.iter().map(|entry| entry.label.as_str()).collect()
    }

    #[test]
    fn normalize_title_ignores_case_and_spacing() {
        let a = normalize_title("SmartBIP | Retrospective");
        let b = normalize_title("  smartbip |  retrospective ");
        assert_eq!(a, "smartbip | retrospective");
        assert_eq!(a, b);
    }

    #[test]
    fn normalize_title_collapses_tabs_and_newlines() {
        assert_eq!(
            normalize_title("Daily\t\n Team\u{a0}sync"),
            "daily team sync"
        );
    }

    #[test]
    fn normalize_title_lowercases_non_ascii() {
        assert_eq!(normalize_title("ÉQUIPE"), "équipe");
    }

    #[test]
    fn normalize_title_of_blank_is_empty() {
        assert_eq!(normalize_title(""), "");
        assert_eq!(normalize_title(" \t\n "), "");
    }

    #[test]
    fn build_plan_without_template_is_equal_in_participant_order() {
        let plan = build_plan(&four_people(), None);

        assert_eq!(labels(&plan), ["Anna", "Bruno", "Carla", "Dario"]);
        assert_eq!(weights(&plan), [1, 1, 1, 1]);
        assert_eq!(plan[0].kind, EntryKind::Attendee);
        assert_eq!(plan[0].email.as_deref(), Some("anna@x.it"));
    }

    #[test]
    fn build_plan_of_nothing_is_empty() {
        assert!(build_plan(&[], None).is_empty());
        assert!(build_plan(&[], Some(&[])).is_empty());
    }

    #[test]
    fn build_plan_keeps_saved_order_labels_and_weights() {
        let template = vec![
            attendee("Dario S.", "dario@x.it", 600),
            attendee("Anna R.", "anna@x.it", 120),
        ];
        let participants = vec![person("Anna", "anna@x.it"), person("Dario", "dario@x.it")];

        let plan = build_plan(&participants, Some(&template));

        assert_eq!(labels(&plan), ["Dario S.", "Anna R."]);
        assert_eq!(weights(&plan), [600, 120]);
    }

    #[test]
    fn build_plan_matches_email_ignoring_case_and_spaces() {
        let template = vec![attendee("Anna", "Anna@X.IT", 300)];
        let participants = vec![person("Anna", " anna@x.it ")];

        let plan = build_plan(&participants, Some(&template));

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].weight, 300);
    }

    #[test]
    fn build_plan_uses_only_the_first_entry_of_a_duplicate_email() {
        let template = vec![
            attendee("Anna", "anna@x.it", 300),
            attendee("Anna bis", "ANNA@x.it", 900),
        ];

        let plan = build_plan(&[person("Anna", "anna@x.it")], Some(&template));

        assert_eq!(labels(&plan), ["Anna"]);
        assert_eq!(weights(&plan), [300]);
    }

    #[test]
    fn build_plan_drops_attendee_without_email() {
        let template = vec![TemplateEntry {
            kind: EntryKind::Attendee,
            email: None,
            label: "Ghost".to_string(),
            weight_secs: 300,
        }];

        let plan = build_plan(&[], Some(&template));

        assert!(plan.is_empty());
    }

    #[test]
    fn build_plan_drops_removed_participant() {
        let template = design_doc_template();
        let participants = vec![person("Anna", "anna@x.it"), person("Bruno", "bruno@x.it")];

        let plan = build_plan(&participants, Some(&template));

        assert_eq!(labels(&plan), ["Anna", "Bruno", FREE_TIME_LABEL]);
        assert_eq!(weights(&plan), [1500, 300, 1200]);
    }

    #[test]
    fn build_plan_keeps_excluded_participant_excluded() {
        let template = vec![
            attendee("Anna", "anna@x.it", 300),
            attendee("Bruno", "bruno@x.it", 0),
        ];
        let participants = vec![person("Anna", "anna@x.it"), person("Bruno", "bruno@x.it")];

        let plan = build_plan(&participants, Some(&template));

        assert_eq!(weights(&plan), [300, 0]);
    }

    #[test]
    fn build_plan_keeps_manual_and_free_time_entries() {
        let template = vec![
            manual("Demo", 600),
            attendee("Anna", "anna@x.it", 300),
            free(60),
        ];

        let plan = build_plan(&[], Some(&template));

        assert_eq!(labels(&plan), ["Demo", FREE_TIME_LABEL]);
        assert_eq!(plan[0].kind, EntryKind::Manual);
        assert_eq!(plan[1].kind, EntryKind::FreeTime);
    }

    #[test]
    fn build_plan_puts_new_participant_before_free_time_with_mean_weight() {
        let template = design_doc_template();
        let mut participants = four_people();
        participants.push(person("Elena", "elena@x.it"));

        let plan = build_plan(&participants, Some(&template));

        assert_eq!(
            labels(&plan),
            ["Anna", "Bruno", "Carla", "Dario", "Elena", FREE_TIME_LABEL]
        );
        // (1500 + 300 + 300 + 300) / 4 = 600
        assert_eq!(weights(&plan), [1500, 300, 300, 300, 600, 1200]);
        assert_eq!(plan[4].email.as_deref(), Some("elena@x.it"));
    }

    #[test]
    fn build_plan_new_participants_keep_participant_order() {
        let template = vec![attendee("Anna", "anna@x.it", 300), free(60)];
        let participants = vec![
            person("Zoe", "zoe@x.it"),
            person("Anna", "anna@x.it"),
            person("Bea", "bea@x.it"),
        ];

        let plan = build_plan(&participants, Some(&template));

        assert_eq!(labels(&plan), ["Anna", "Zoe", "Bea", FREE_TIME_LABEL]);
    }

    #[test]
    fn build_plan_mean_counts_attendees_absent_from_the_meeting() {
        let template = vec![
            attendee("Anna", "anna@x.it", 1000),
            attendee("Bruno", "bruno@x.it", 0),
            attendee("Carla", "carla@x.it", 501),
        ];
        let participants = vec![person("Dario", "dario@x.it")];

        let plan = build_plan(&participants, Some(&template));

        // floor((1000 + 501) / 2) = 750
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].weight, 750);
    }

    #[test]
    fn build_plan_mean_falls_back_to_all_entries_without_attendees() {
        let template = vec![manual("Demo", 600), manual("Q&A", 301), free(0)];

        let plan = build_plan(&[person("Anna", "anna@x.it")], Some(&template));

        // floor((600 + 301) / 2) = 450
        assert_eq!(plan[2].label, "Anna");
        assert_eq!(plan[2].weight, 450);
    }

    #[test]
    fn build_plan_mean_falls_back_to_one_without_positive_weights() {
        let template = vec![attendee("Bruno", "bruno@x.it", 0)];

        let plan = build_plan(&[person("Anna", "anna@x.it")], Some(&template));

        assert_eq!(weights(&plan), [1]);
    }

    #[test]
    fn build_plan_places_new_participant_first_when_template_is_only_free_time() {
        let template = vec![free(600)];

        let plan = build_plan(&[person("Anna", "anna@x.it")], Some(&template));

        assert_eq!(labels(&plan), ["Anna", FREE_TIME_LABEL]);
        assert_eq!(weights(&plan), [600, 600]);
    }

    #[test]
    fn template_to_plan_converts_one_to_one() {
        let entries = vec![
            attendee("Anna", "anna@x.it", 300),
            manual("Demo", 0),
            free(60),
        ];

        let plan = template_to_plan(&entries);

        assert_eq!(labels(&plan), ["Anna", "Demo", FREE_TIME_LABEL]);
        assert_eq!(weights(&plan), [300, 0, 60]);
        assert_eq!(plan[0].email.as_deref(), Some("anna@x.it"));
        assert_eq!(plan[2].kind, EntryKind::FreeTime);
    }

    #[test]
    fn plan_to_editor_rebuilds_the_design_doc_minutes() {
        let plan = template_to_plan(&design_doc_template());

        let rows = plan_to_editor(&plan, 3600);

        let minutes: Vec<u32> = rows.iter().map(|row| row.minutes).collect();
        assert_eq!(minutes, [25, 5, 5, 5]);
        assert!(rows.iter().all(|row| row.included));
        assert_eq!(unassigned_minutes(&rows, 3600), 20);
    }

    #[test]
    fn plan_to_editor_scales_to_a_shorter_meeting_rounding_half_up() {
        let plan = template_to_plan(&design_doc_template());

        let rows = plan_to_editor(&plan, 1800);

        // 750 s = 12.5 min -> 13; 150 s = 2.5 min -> 3
        let minutes: Vec<u32> = rows.iter().map(|row| row.minutes).collect();
        assert_eq!(minutes, [13, 3, 3, 3]);
        assert_eq!(unassigned_minutes(&rows, 1800), 30 - 22);
    }

    #[test]
    fn plan_to_editor_gives_excluded_rows_zero_minutes() {
        let plan = template_to_plan(&[
            attendee("Anna", "anna@x.it", 300),
            attendee("Bruno", "bruno@x.it", 0),
        ]);

        let rows = plan_to_editor(&plan, 600);

        assert_eq!(rows[0].minutes, 10);
        assert!(rows[0].included);
        assert_eq!(rows[1].minutes, 0);
        assert!(!rows[1].included);
    }

    #[test]
    fn plan_to_editor_skips_free_time_entries() {
        let plan = template_to_plan(&[manual("Demo", 300), free(300)]);

        let rows = plan_to_editor(&plan, 600);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, EntryKind::Manual);
        assert_eq!(rows[0].minutes, 5);
    }

    #[test]
    fn plan_to_editor_gives_zero_minutes_for_a_zero_duration() {
        let plan = template_to_plan(&[attendee("Anna", "anna@x.it", 300)]);

        let rows = plan_to_editor(&plan, 0);

        assert_eq!(rows[0].minutes, 0);
        assert!(rows[0].included);
    }

    #[test]
    fn plan_to_editor_gives_zero_minutes_when_all_weights_are_zero() {
        let plan = template_to_plan(&[attendee("Anna", "anna@x.it", 0)]);

        let rows = plan_to_editor(&plan, 600);

        assert_eq!(rows[0].minutes, 0);
        assert!(!rows[0].included);
    }

    #[test]
    fn plan_to_editor_of_nothing_is_empty() {
        assert!(plan_to_editor(&[], 600).is_empty());
    }

    #[test]
    fn unassigned_minutes_ignores_excluded_rows() {
        let rows = vec![
            row("Anna", "a@x.it", 10, true),
            row("Bruno", "b@x.it", 7, false),
        ];

        assert_eq!(unassigned_minutes(&rows, 1800), 20);
    }

    #[test]
    fn unassigned_minutes_is_negative_when_over_assigned() {
        let rows = vec![row("Anna", "a@x.it", 40, true)];

        assert_eq!(unassigned_minutes(&rows, 1800), -10);
    }

    #[test]
    fn unassigned_minutes_floors_the_duration() {
        assert_eq!(unassigned_minutes(&[], 119), 1);
    }

    #[test]
    fn editor_to_template_adds_free_time_for_unassigned_seconds() {
        let rows = vec![
            row("Anna", "anna@x.it", 25, true),
            row("Bruno", "bruno@x.it", 5, true),
            row("Carla", "carla@x.it", 5, true),
            row("Dario", "dario@x.it", 5, true),
        ];

        let entries = editor_to_template(&rows, 3600);

        assert_eq!(entries, design_doc_template());
    }

    #[test]
    fn editor_to_template_has_no_free_time_when_fully_assigned() {
        let rows = vec![row("Anna", "anna@x.it", 30, true)];

        let entries = editor_to_template(&rows, 1800);

        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn editor_to_template_has_no_free_time_when_over_assigned() {
        let rows = vec![row("Anna", "anna@x.it", 40, true)];

        let entries = editor_to_template(&rows, 1800);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].weight_secs, 2400);
    }

    #[test]
    fn editor_to_template_saves_excluded_rows_with_weight_zero() {
        let rows = vec![row("Anna", "anna@x.it", 10, false)];

        let entries = editor_to_template(&rows, 600);

        assert_eq!(entries[0].weight_secs, 0);
        assert_eq!(entries[1].kind, EntryKind::FreeTime);
        assert_eq!(entries[1].weight_secs, 600);
    }

    #[test]
    fn editor_to_template_keeps_email_only_for_attendees() {
        let mut manual_row = row("Demo", "stale@x.it", 5, true);
        manual_row.kind = EntryKind::Manual;
        let rows = vec![row("Anna", "anna@x.it", 5, true), manual_row];

        let entries = editor_to_template(&rows, 600);

        assert_eq!(entries[0].email.as_deref(), Some("anna@x.it"));
        assert_eq!(entries[1].email, None);
    }

    #[test]
    fn editor_to_template_trims_labels() {
        let rows = vec![row("  Anna  ", "anna@x.it", 5, true)];

        let entries = editor_to_template(&rows, 300);

        assert_eq!(entries[0].label, "Anna");
    }

    #[test]
    fn editor_to_template_names_an_empty_attendee_label_from_the_email() {
        let rows = vec![row("  ", "anna.rossi@x.it", 5, true)];

        let entries = editor_to_template(&rows, 300);

        assert_eq!(entries[0].label, "Anna Rossi");
    }

    #[test]
    fn editor_to_template_names_an_empty_manual_label_voce() {
        let mut manual_row = row("", "", 5, true);
        manual_row.kind = EntryKind::Manual;
        manual_row.email = None;

        let entries = editor_to_template(&[manual_row], 300);

        assert_eq!(entries[0].label, "Voce");
    }

    #[test]
    fn editor_to_template_names_an_empty_attendee_without_email_voce() {
        let mut attendee_row = row("", "", 5, true);
        attendee_row.email = None;

        let entries = editor_to_template(&[attendee_row], 300);

        assert_eq!(entries[0].label, "Voce");
    }

    #[test]
    fn editor_to_template_saturates_instead_of_overflowing() {
        let rows = vec![row("Anna", "anna@x.it", u32::MAX, true)];

        let entries = editor_to_template(&rows, i64::MAX);

        assert_eq!(entries[0].weight_secs, u32::MAX);
        assert_eq!(entries[1].kind, EntryKind::FreeTime);
        assert_eq!(entries[1].weight_secs, u32::MAX);
    }

    #[test]
    fn editor_to_template_of_nothing_is_only_free_time() {
        let entries = editor_to_template(&[], 600);

        assert_eq!(entries, vec![free(600)]);
    }
}
