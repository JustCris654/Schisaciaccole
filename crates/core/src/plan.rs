//! Plans built from saved templates, and the conversions used by the participant editor.
//!
//! A template stores a weight per entry. A plan applies a template to the people who are
//! actually in a meeting. The participant editor only tells who takes part: a template saved
//! from it gives every included person the same weight. Nothing here reads the clock.

use crate::api::{EntryKind, PlanEntry, TemplateEntry};
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
    /// Kind of the row; never [`EntryKind::FreeTime`], the editor has no such rows.
    pub kind: EntryKind,
    /// Email of the attendee; `Some` only for [`EntryKind::Attendee`].
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
        if entry.kind == EntryKind::FreeTime {
            continue;
        }
        rows.push(EditorEntry {
            kind: entry.kind,
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
/// skipped too. A row of kind [`EntryKind::FreeTime`] is skipped. Nothing here limits the number
/// of entries, and an input with no included row gives a template the server rejects.
pub fn editor_to_template(rows: &[EditorEntry]) -> Vec<TemplateEntry> {
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        if row.kind == EntryKind::FreeTime {
            continue;
        }
        let label = row_label(row);
        if label.is_empty() {
            continue;
        }
        let email = match row.kind {
            EntryKind::Attendee => row.email.clone(),
            _ => None,
        };
        let weight_secs = if row.included {
            PARTICIPANT_WEIGHT_SECS
        } else {
            0
        };
        entries.push(TemplateEntry {
            kind: row.kind,
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
    if row.kind == EntryKind::Attendee {
        return display_name_from_email(row.email.as_deref().unwrap_or(""));
    }
    String::new()
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
            kind: EntryKind::Attendee,
            email: Some(email.to_string()),
            label: label.to_string(),
            included,
        }
    }

    fn manual_row(label: &str, included: bool) -> EditorEntry {
        EditorEntry {
            kind: EntryKind::Manual,
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
        assert_eq!(rows[0].kind, EntryKind::Manual);
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
    fn editor_to_template_skips_free_time_rows() {
        let mut free_row = manual_row("Tempo libero", true);
        free_row.kind = EntryKind::FreeTime;

        let entries = editor_to_template(&[free_row]);

        assert!(entries.is_empty());
    }

    #[test]
    fn editor_to_template_of_nothing_is_empty() {
        assert!(editor_to_template(&[]).is_empty());
    }
}
