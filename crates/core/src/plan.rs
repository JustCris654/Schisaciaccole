//! Plans built from saved templates, and the conversions used by the participant editor.
//!
//! A template stores a weight per entry. A plan applies a template to the people who are
//! actually in a meeting. The participant editor only tells who takes part: a template saved
//! from it gives every included person the same weight. Nothing here reads the clock.

use crate::api::{EntryKind, PersonKind, PlanEntry, PreferencePerson, TemplateEntry};
use crate::attendee::display_name_from_email;
use crate::slots::Participant;
use std::collections::HashSet;

/// Label of the entry that holds the time not assigned to anybody.
pub const FREE_TIME_LABEL: &str = "Tempo libero";

/// Weight, in seconds, that [`editor_to_template`] gives to every included participant.
pub const PARTICIPANT_WEIGHT_SECS: u32 = 60;

/// One row of the participant editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEntry {
    /// Kind of the row.
    pub kind: PersonKind,
    /// Email of the attendee; `Some` only for [`PersonKind::Attendee`].
    pub email: Option<String>,
    /// Label shown for the row.
    pub label: String,
    /// False when the participant is excluded from the meeting.
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

/// Converts a plan into participant editor rows.
///
/// For every entry that is not free time, the row is `included` when its weight is above 0.
/// Order, kind, email and label are kept. Free time entries are skipped.
///
/// Edge cases: an empty plan gives no rows; a plan of only free time gives no rows.
pub fn plan_to_editor(entries: &[PlanEntry]) -> Vec<EditorEntry> {
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let kind = match entry.kind {
            EntryKind::Attendee => PersonKind::Attendee,
            EntryKind::Manual => PersonKind::Manual,
            EntryKind::FreeTime => continue,
        };
        rows.push(EditorEntry {
            kind,
            email: entry.email.clone(),
            label: entry.label.clone(),
            included: entry.weight > 0,
        });
    }
    rows
}

/// Converts editor rows into template entries to save.
///
/// Rows keep their order. An included row gets [`PARTICIPANT_WEIGHT_SECS`] and an excluded row
/// 0, so the person stays in the template as excluded. The email is kept only for attendees.
/// Labels are trimmed. No free time entry is added.
///
/// Edge cases: a row with an empty label is skipped, except an attendee, which is named from
/// its email with [`display_name_from_email`]; an attendee without email and without a label is
/// skipped too. Nothing here limits the number
/// of entries, and an input with no included row gives a template the server rejects.
pub fn editor_to_template(rows: &[EditorEntry]) -> Vec<TemplateEntry> {
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let label = row_label(row);
        if label.is_empty() {
            continue;
        }
        let (kind, email) = match row.kind {
            PersonKind::Attendee => (EntryKind::Attendee, row.email.clone()),
            PersonKind::Manual => (EntryKind::Manual, None),
        };
        let weight_secs = if row.included {
            PARTICIPANT_WEIGHT_SECS
        } else {
            0
        };
        entries.push(TemplateEntry {
            kind,
            label,
            email,
            weight_secs,
        });
    }
    entries
}

/// Returns the trimmed label of a row. For an attendee with an empty label it is the name built
/// from the email; it is empty when there is nothing to show.
fn row_label(row: &EditorEntry) -> String {
    let label = row.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    if row.kind == PersonKind::Attendee {
        return display_name_from_email(row.email.as_deref().unwrap_or(""));
    }
    String::new()
}

/// Builds the label of a participant row: the preference rename of the first attendee entry with
/// the same email key, else the participant name.
fn row_name(participant: &Participant, preference: Option<&[PreferencePerson]>) -> String {
    let key = email_key(&participant.email);
    if let Some(people) = preference {
        for person in people {
            if person.kind != PersonKind::Attendee {
                continue;
            }
            if person.email.as_deref().map(email_key).as_deref() == Some(key.as_str()) {
                return person.label.clone();
            }
        }
    }
    participant.name.clone()
}

/// Builds the editor rows of a meeting from its participants and the saved preference.
///
/// One included attendee row per participant, in the given order. A repeated email (trimmed, ASCII
/// case-insensitive) keeps only the first. The label is the one of the first attendee entry of the
/// preference with the same email, else the participant name. Then one included manual row per
/// manual entry of the preference, in saved order, with the saved label.
///
/// Edge cases: preference attendee entries that match no participant, or have no email, are
/// ignored. No participants and no preference give no rows. Labels are not trimmed or checked.
pub fn editor_rows(
    participants: &[Participant],
    preference: Option<&[PreferencePerson]>,
) -> Vec<EditorEntry> {
    let mut rows = attendee_rows(participants, &[], preference);
    if let Some(people) = preference {
        for person in people {
            if person.kind == PersonKind::Manual {
                rows.push(manual_editor_row(person.label.clone(), true));
            }
        }
    }
    rows
}

/// Builds an editor row for a manual person.
fn manual_editor_row(label: String, included: bool) -> EditorEntry {
    EditorEntry {
        kind: PersonKind::Manual,
        email: None,
        label,
        included,
    }
}

/// Builds the attendee section in participant order, deduplicated by email key. A participant
/// matching an existing attendee row reuses that row unchanged; others get a new included row.
fn attendee_rows(
    participants: &[Participant],
    existing: &[EditorEntry],
    preference: Option<&[PreferencePerson]>,
) -> Vec<EditorEntry> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut rows = Vec::with_capacity(participants.len());
    for participant in participants {
        let key = email_key(&participant.email);
        if !seen.insert(key.clone()) {
            continue;
        }
        let found = existing.iter().find(|row| {
            row.kind == PersonKind::Attendee
                && row.email.as_deref().map(email_key).as_deref() == Some(key.as_str())
        });
        match found {
            Some(row) => rows.push(row.clone()),
            None => rows.push(EditorEntry {
                kind: PersonKind::Attendee,
                email: Some(participant.email.clone()),
                label: row_name(participant, preference),
                included: true,
            }),
        }
    }
    rows
}

/// Merges an existing draft with freshly read participants.
///
/// The attendee section is rebuilt in participant order, deduplicated by email key (first wins).
/// A participant matching an existing attendee row (first match) keeps that row unchanged, with
/// its `included` and `label`. Any other participant gets a new included row, labelled as in
/// [`editor_rows`]. Existing attendee rows that match no participant are dropped, even when
/// renamed. Then every existing manual row follows, in order and unchanged.
///
/// Edge cases: the preference only names new participants. A draft already in participant order
/// with the same participants is returned equal. Attendee rows without email never match.
pub fn merge_rows(
    rows: &[EditorEntry],
    participants: &[Participant],
    preference: Option<&[PreferencePerson]>,
) -> Vec<EditorEntry> {
    let mut merged = attendee_rows(participants, rows, preference);
    for row in rows {
        if row.kind == PersonKind::Manual {
            merged.push(row.clone());
        }
    }
    merged
}

/// Builds the people to save in a meeting preference.
///
/// Output order: attendee renames from the rows, then carried-over saved renames, then manual
/// people. An attendee row with an email is saved as a rename when its trimmed label is not empty
/// and differs from the calendar name of the participant with the same email key; when no
/// participant matches, it is saved when the trimmed label is not empty. `included` is ignored.
/// One entry per email key, the first wins. Every `saved` attendee entry with an email whose key
/// matches no attendee row of `rows` is appended unchanged, so renames of people not invited
/// today are kept. Every manual row with a non-empty trimmed label is saved, included or not,
/// in row order, with the label trimmed and no email.
///
/// Edge cases: rows with blank labels produce nothing. Attendee rows without email are ignored.
/// The number of people is not limited here; the server rejects more than 100.
pub fn preference_people(
    rows: &[EditorEntry],
    participants: &[Participant],
    saved: Option<&[PreferencePerson]>,
) -> Vec<PreferencePerson> {
    let mut people: Vec<PreferencePerson> = Vec::with_capacity(rows.len());
    let mut seen: HashSet<String> = HashSet::new();
    let mut row_keys: HashSet<String> = HashSet::new();

    for row in rows {
        if row.kind != PersonKind::Attendee {
            continue;
        }
        let Some(email) = row.email.as_deref() else {
            continue;
        };
        let key = email_key(email);
        row_keys.insert(key.clone());
        let label = row.label.trim();
        if label.is_empty() || seen.contains(&key) {
            continue;
        }
        let calendar_name = participants
            .iter()
            .find(|participant| email_key(&participant.email) == key)
            .map(|participant| participant.name.as_str());
        if calendar_name == Some(label) {
            continue;
        }
        seen.insert(key);
        people.push(PreferencePerson {
            kind: PersonKind::Attendee,
            email: Some(email.trim().to_string()),
            label: label.to_string(),
        });
    }

    if let Some(saved) = saved {
        for person in saved {
            if person.kind != PersonKind::Attendee {
                continue;
            }
            let Some(email) = person.email.as_deref() else {
                continue;
            };
            let key = email_key(email);
            if row_keys.contains(&key) || !seen.insert(key) {
                continue;
            }
            people.push(person.clone());
        }
    }

    for row in rows {
        if row.kind != PersonKind::Manual {
            continue;
        }
        let label = row.label.trim();
        if !label.is_empty() {
            people.push(PreferencePerson {
                kind: PersonKind::Manual,
                email: None,
                label: label.to_string(),
            });
        }
    }
    people
}

/// Returns the names to start a meeting with: the trimmed labels of the included rows, in row
/// order.
///
/// Edge cases: an included row whose trimmed label is empty is skipped; excluded rows are never
/// returned; no rows give no names.
pub fn start_names(rows: &[EditorEntry]) -> Vec<String> {
    let mut names = Vec::with_capacity(rows.len());
    for row in rows {
        let label = row.label.trim();
        if row.included && !label.is_empty() {
            names.push(label.to_string());
        }
    }
    names
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

    fn row(label: &str, email: &str, included: bool) -> EditorEntry {
        EditorEntry {
            kind: PersonKind::Attendee,
            email: Some(email.to_string()),
            label: label.to_string(),
            included,
        }
    }

    fn manual_row(label: &str, included: bool) -> EditorEntry {
        EditorEntry {
            kind: PersonKind::Manual,
            email: None,
            label: label.to_string(),
            included,
        }
    }

    fn plan_of(entries: &[TemplateEntry]) -> Vec<PlanEntry> {
        entries.iter().map(template_entry_to_plan).collect()
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
    fn plan_to_editor_includes_entries_with_a_positive_weight() {
        let plan = plan_of(&design_doc_template());

        let rows = plan_to_editor(&plan);

        let names: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(names, ["Anna", "Bruno", "Carla", "Dario"]);
        assert!(rows.iter().all(|row| row.included));
        assert_eq!(rows[0].email.as_deref(), Some("anna@x.it"));
    }

    #[test]
    fn plan_to_editor_excludes_an_entry_with_weight_zero() {
        let plan = plan_of(&[
            attendee("Anna", "anna@x.it", 300),
            attendee("Bruno", "bruno@x.it", 0),
        ]);

        let rows = plan_to_editor(&plan);

        assert!(rows[0].included);
        assert!(!rows[1].included);
    }

    #[test]
    fn plan_to_editor_skips_free_time_entries() {
        let plan = plan_of(&[manual("Demo", 300), free(300)]);

        let rows = plan_to_editor(&plan);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, PersonKind::Manual);
    }

    #[test]
    fn plan_to_editor_keeps_the_plan_order() {
        let plan = plan_of(&[
            attendee("Dario", "dario@x.it", 60),
            manual("Demo", 60),
            attendee("Anna", "anna@x.it", 60),
        ]);

        let rows = plan_to_editor(&plan);

        let names: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(names, ["Dario", "Demo", "Anna"]);
    }

    #[test]
    fn plan_to_editor_of_nothing_is_empty() {
        assert!(plan_to_editor(&[]).is_empty());
    }

    #[test]
    fn editor_to_template_gives_included_rows_the_participant_weight() {
        let rows = vec![
            row("Anna", "anna@x.it", true),
            row("Bruno", "bruno@x.it", true),
        ];

        let entries = editor_to_template(&rows);

        assert_eq!(
            entries,
            vec![
                attendee("Anna", "anna@x.it", PARTICIPANT_WEIGHT_SECS),
                attendee("Bruno", "bruno@x.it", PARTICIPANT_WEIGHT_SECS),
            ]
        );
    }

    #[test]
    fn editor_to_template_saves_excluded_rows_with_weight_zero() {
        let rows = vec![row("Anna", "anna@x.it", false)];

        let entries = editor_to_template(&rows);

        assert_eq!(entries, vec![attendee("Anna", "anna@x.it", 0)]);
    }

    #[test]
    fn editor_to_template_adds_no_free_time_entry() {
        let rows = vec![row("Anna", "anna@x.it", true)];

        let entries = editor_to_template(&rows);

        assert!(
            entries
                .iter()
                .all(|entry| entry.kind != EntryKind::FreeTime)
        );
    }

    #[test]
    fn editor_to_template_keeps_the_row_order() {
        let rows = vec![
            row("Dario", "dario@x.it", true),
            manual_row("Demo", true),
            row("Anna", "anna@x.it", false),
        ];

        let entries = editor_to_template(&rows);

        let labels: Vec<&str> = entries.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(labels, ["Dario", "Demo", "Anna"]);
    }

    #[test]
    fn editor_to_template_keeps_email_only_for_attendees() {
        let mut stale = manual_row("Demo", true);
        stale.email = Some("stale@x.it".to_string());
        let rows = vec![row("Anna", "anna@x.it", true), stale];

        let entries = editor_to_template(&rows);

        assert_eq!(entries[0].email.as_deref(), Some("anna@x.it"));
        assert_eq!(entries[1].email, None);
    }

    #[test]
    fn editor_to_template_trims_labels() {
        let rows = vec![row("  Anna  ", "anna@x.it", true)];

        let entries = editor_to_template(&rows);

        assert_eq!(entries[0].label, "Anna");
    }

    #[test]
    fn editor_to_template_names_an_empty_attendee_label_from_the_email() {
        let rows = vec![row("  ", "anna.rossi@x.it", true)];

        let entries = editor_to_template(&rows);

        assert_eq!(entries[0].label, "Anna Rossi");
    }

    #[test]
    fn editor_to_template_skips_a_blank_manual_row() {
        let rows = vec![manual_row("   ", true), row("Anna", "anna@x.it", true)];

        let entries = editor_to_template(&rows);

        assert_eq!(
            entries,
            vec![attendee("Anna", "anna@x.it", PARTICIPANT_WEIGHT_SECS)]
        );
    }

    #[test]
    fn editor_to_template_skips_a_nameless_attendee_without_email() {
        let mut nameless = row("", "", true);
        nameless.email = None;

        let entries = editor_to_template(&[nameless]);

        assert!(entries.is_empty());
    }

    #[test]
    fn editor_to_template_of_nothing_is_empty() {
        assert!(editor_to_template(&[]).is_empty());
    }

    fn pref_attendee(label: &str, email: &str) -> PreferencePerson {
        PreferencePerson {
            kind: PersonKind::Attendee,
            email: Some(email.to_string()),
            label: label.to_string(),
        }
    }

    fn pref_manual(label: &str) -> PreferencePerson {
        PreferencePerson {
            kind: PersonKind::Manual,
            email: None,
            label: label.to_string(),
        }
    }

    fn row_labels(rows: &[EditorEntry]) -> Vec<&str> {
        rows.iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn editor_rows_without_preference_use_participant_names() {
        let rows = editor_rows(&four_people(), None);

        assert_eq!(row_labels(&rows), ["Anna", "Bruno", "Carla", "Dario"]);
        assert!(rows.iter().all(|row| row.included));
        assert_eq!(rows[0].email.as_deref(), Some("anna@x.it"));
        assert_eq!(rows[0].kind, PersonKind::Attendee);
    }

    #[test]
    fn editor_rows_apply_the_preference_rename() {
        let preference = vec![pref_attendee("Anna R.", "anna@x.it")];

        let rows = editor_rows(&four_people(), Some(&preference));

        assert_eq!(row_labels(&rows), ["Anna R.", "Bruno", "Carla", "Dario"]);
    }

    #[test]
    fn editor_rows_match_the_rename_ignoring_case_and_spaces() {
        let preference = vec![pref_attendee("Anna R.", " ANNA@x.IT ")];

        let rows = editor_rows(&[person("Anna", "anna@x.it")], Some(&preference));

        assert_eq!(row_labels(&rows), ["Anna R."]);
    }

    #[test]
    fn editor_rows_use_the_first_rename_of_a_repeated_email() {
        let preference = vec![
            pref_attendee("First", "anna@x.it"),
            pref_attendee("Second", "anna@x.it"),
        ];

        let rows = editor_rows(&[person("Anna", "anna@x.it")], Some(&preference));

        assert_eq!(row_labels(&rows), ["First"]);
    }

    #[test]
    fn editor_rows_ignore_renames_of_people_not_invited() {
        let preference = vec![pref_attendee("Zed", "zed@x.it")];

        let rows = editor_rows(&[person("Anna", "anna@x.it")], Some(&preference));

        assert_eq!(row_labels(&rows), ["Anna"]);
    }

    #[test]
    fn editor_rows_ignore_preference_attendee_without_email() {
        let preference = vec![PreferencePerson {
            kind: PersonKind::Attendee,
            email: None,
            label: "Ghost".to_string(),
        }];

        let rows = editor_rows(&[person("Anna", "anna@x.it")], Some(&preference));

        assert_eq!(row_labels(&rows), ["Anna"]);
    }

    #[test]
    fn editor_rows_keep_only_the_first_of_a_repeated_participant_email() {
        let participants = vec![
            person("Anna", "anna@x.it"),
            person("Anna bis", " ANNA@x.it"),
            person("Bruno", "bruno@x.it"),
        ];

        let rows = editor_rows(&participants, None);

        assert_eq!(row_labels(&rows), ["Anna", "Bruno"]);
    }

    #[test]
    fn editor_rows_append_manual_people_in_saved_order() {
        let preference = vec![
            pref_manual("Demo"),
            pref_attendee("Anna R.", "anna@x.it"),
            pref_manual("Q&A"),
        ];

        let rows = editor_rows(&[person("Anna", "anna@x.it")], Some(&preference));

        assert_eq!(row_labels(&rows), ["Anna R.", "Demo", "Q&A"]);
        assert_eq!(rows[1].kind, PersonKind::Manual);
        assert_eq!(rows[1].email, None);
        assert!(rows[1].included);
    }

    #[test]
    fn editor_rows_of_nothing_is_empty() {
        assert!(editor_rows(&[], None).is_empty());
    }

    #[test]
    fn merge_rows_returns_an_unchanged_draft_in_participant_order() {
        let participants = vec![person("Anna", "anna@x.it"), person("Bruno", "bruno@x.it")];
        let mut draft = editor_rows(&participants, None);
        draft.push(manual_row("Demo", false));

        let merged = merge_rows(&draft, &participants, None);

        assert_eq!(merged, draft);
    }

    #[test]
    fn merge_rows_replaces_a_group_placeholder_with_members() {
        let draft = vec![row("Team", "team@x.it", true), manual_row("Demo", true)];
        let members = vec![person("Anna", "anna@x.it"), person("Bruno", "bruno@x.it")];

        let merged = merge_rows(&draft, &members, None);

        assert_eq!(row_labels(&merged), ["Anna", "Bruno", "Demo"]);
    }

    #[test]
    fn merge_rows_keeps_a_member_exclusion() {
        let draft = vec![row("Anna", "anna@x.it", false)];
        let participants = vec![person("Anna", "anna@x.it"), person("Bruno", "bruno@x.it")];

        let merged = merge_rows(&draft, &participants, None);

        assert!(!merged[0].included);
        assert!(merged[1].included);
    }

    #[test]
    fn merge_rows_keeps_a_rename_of_a_remaining_participant() {
        let draft = vec![row("Anna R.", "ANNA@x.it", true)];

        let merged = merge_rows(&draft, &[person("Anna", "anna@x.it")], None);

        assert_eq!(merged, draft);
    }

    #[test]
    fn merge_rows_includes_a_new_participant_named_by_the_preference() {
        let draft = vec![row("Anna", "anna@x.it", true)];
        let participants = vec![person("Anna", "anna@x.it"), person("Bruno", "bruno@x.it")];
        let preference = vec![pref_attendee("Bru", "bruno@x.it")];

        let merged = merge_rows(&draft, &participants, Some(&preference));

        assert_eq!(row_labels(&merged), ["Anna", "Bru"]);
        assert!(merged[1].included);
    }

    #[test]
    fn merge_rows_does_not_apply_the_preference_to_existing_rows() {
        let draft = vec![row("Anna", "anna@x.it", true)];
        let preference = vec![pref_attendee("Anna R.", "anna@x.it")];

        let merged = merge_rows(&draft, &[person("Anna", "anna@x.it")], Some(&preference));

        assert_eq!(row_labels(&merged), ["Anna"]);
    }

    #[test]
    fn merge_rows_drops_a_removed_participant_even_when_renamed() {
        let draft = vec![
            row("Anna", "anna@x.it", true),
            row("Bru", "bruno@x.it", true),
        ];

        let merged = merge_rows(&draft, &[person("Anna", "anna@x.it")], None);

        assert_eq!(row_labels(&merged), ["Anna"]);
    }

    #[test]
    fn merge_rows_follows_the_participant_order() {
        let draft = vec![
            row("Anna", "anna@x.it", true),
            row("Bruno", "bruno@x.it", true),
        ];
        let participants = vec![person("Bruno", "bruno@x.it"), person("Anna", "anna@x.it")];

        let merged = merge_rows(&draft, &participants, None);

        assert_eq!(row_labels(&merged), ["Bruno", "Anna"]);
    }

    #[test]
    fn merge_rows_keeps_manual_rows_untouched_after_attendees() {
        let draft = vec![
            manual_row("Demo", false),
            row("Anna", "anna@x.it", true),
            manual_row("  Q&A ", true),
        ];

        let merged = merge_rows(&draft, &[person("Anna", "anna@x.it")], None);

        assert_eq!(
            merged,
            vec![
                row("Anna", "anna@x.it", true),
                manual_row("Demo", false),
                manual_row("  Q&A ", true),
            ]
        );
    }

    #[test]
    fn merge_rows_dedupes_repeated_participant_emails() {
        let participants = vec![person("Anna", "anna@x.it"), person("Anna", "ANNA@x.it")];

        let merged = merge_rows(&[], &participants, None);

        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn preference_people_saves_a_rename() {
        let rows = vec![row("Anna R.", "anna@x.it", true)];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert_eq!(people, vec![pref_attendee("Anna R.", "anna@x.it")]);
    }

    #[test]
    fn preference_people_skips_a_label_equal_to_the_calendar_name() {
        let rows = vec![row("  Anna ", "anna@x.it", true)];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert!(people.is_empty());
    }

    #[test]
    fn preference_people_saves_the_rename_of_an_excluded_attendee() {
        let rows = vec![row("Anna R.", "anna@x.it", false)];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert_eq!(people, vec![pref_attendee("Anna R.", "anna@x.it")]);
    }

    #[test]
    fn preference_people_matches_the_calendar_name_ignoring_email_case() {
        let rows = vec![row("Anna", "ANNA@x.it", true)];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert!(people.is_empty());
    }

    #[test]
    fn preference_people_trims_label_and_email() {
        let rows = vec![row("  Anna R.  ", " anna@x.it ", true)];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert_eq!(people, vec![pref_attendee("Anna R.", "anna@x.it")]);
    }

    #[test]
    fn preference_people_keeps_a_rename_of_a_row_with_no_participant() {
        let rows = vec![row("Zed Z.", "zed@x.it", true)];

        let people = preference_people(&rows, &[], None);

        assert_eq!(people, vec![pref_attendee("Zed Z.", "zed@x.it")]);
    }

    #[test]
    fn preference_people_skips_a_blank_attendee_label() {
        let rows = vec![row("   ", "anna@x.it", true)];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert!(people.is_empty());
    }

    #[test]
    fn preference_people_keeps_one_entry_per_email() {
        let rows = vec![
            row("First", "anna@x.it", true),
            row("Second", "ANNA@x.it", true),
        ];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert_eq!(people, vec![pref_attendee("First", "anna@x.it")]);
    }

    #[test]
    fn preference_people_carries_over_renames_of_people_not_in_rows() {
        let rows = vec![row("Anna R.", "anna@x.it", true)];
        let saved = vec![pref_attendee("Bru", "bruno@x.it")];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], Some(&saved));

        assert_eq!(
            people,
            vec![
                pref_attendee("Anna R.", "anna@x.it"),
                pref_attendee("Bru", "bruno@x.it"),
            ]
        );
    }

    #[test]
    fn preference_people_drops_the_saved_rename_when_the_row_reverts_to_the_calendar_name() {
        let rows = vec![row("Anna", "anna@x.it", true)];
        let saved = vec![pref_attendee("Anna R.", "anna@x.it")];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], Some(&saved));

        assert!(people.is_empty());
    }

    #[test]
    fn preference_people_carry_over_matches_ignoring_case() {
        let rows = vec![row("Anna R.", "anna@x.it", true)];
        let saved = vec![pref_attendee("Old", "ANNA@x.it")];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], Some(&saved));

        assert_eq!(people, vec![pref_attendee("Anna R.", "anna@x.it")]);
    }

    #[test]
    fn preference_people_carry_over_keeps_one_entry_per_email() {
        let saved = vec![
            pref_attendee("One", "bruno@x.it"),
            pref_attendee("Two", "BRUNO@x.it"),
        ];

        let people = preference_people(&[], &[], Some(&saved));

        assert_eq!(people, vec![pref_attendee("One", "bruno@x.it")]);
    }

    #[test]
    fn preference_people_saves_manual_people_after_attendees_in_row_order() {
        let rows = vec![
            manual_row(" Demo ", true),
            row("Anna R.", "anna@x.it", true),
            manual_row("Q&A", false),
        ];

        let people = preference_people(&rows, &[person("Anna", "anna@x.it")], None);

        assert_eq!(
            people,
            vec![
                pref_attendee("Anna R.", "anna@x.it"),
                pref_manual("Demo"),
                pref_manual("Q&A"),
            ]
        );
    }

    #[test]
    fn preference_people_skips_a_blank_manual_row() {
        let rows = vec![manual_row("   ", true)];

        assert!(preference_people(&rows, &[], None).is_empty());
    }

    #[test]
    fn preference_people_of_nothing_is_empty() {
        assert!(preference_people(&[], &[], None).is_empty());
    }

    #[test]
    fn start_names_lists_included_trimmed_labels_in_order() {
        let rows = vec![
            row(" Anna ", "anna@x.it", true),
            row("Bruno", "bruno@x.it", false),
            manual_row("Demo", true),
        ];

        assert_eq!(start_names(&rows), ["Anna", "Demo"]);
    }

    #[test]
    fn start_names_skips_included_rows_with_a_blank_label() {
        let rows = vec![manual_row("  ", true), row("Anna", "anna@x.it", true)];

        assert_eq!(start_names(&rows), ["Anna"]);
    }

    #[test]
    fn start_names_of_nothing_is_empty() {
        assert!(start_names(&[]).is_empty());
    }
}
