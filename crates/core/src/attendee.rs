//! Helpers for attendee names.

use std::collections::HashMap;

use crate::api::{Attendee, ParticipantResolution, ResponseStatus};

/// A leaf member discovered while resolving an invited group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    /// Email address supplied by the group directory.
    pub email: String,
    /// Display name, when the directory provides one.
    pub name: Option<String>,
}

/// Atomic result of resolving one original group invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupExpansionOutcome {
    /// All known leaf members; an empty list removes the group placeholder.
    Complete(Vec<GroupMember>),
    /// Resolution failed, so the original invitation must be retained.
    Incomplete {
        /// True when the user's OAuth grant lacks directory access.
        reauthorization_required: bool,
    },
}

/// Resolution outcome associated with an original invited email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupExpansion {
    /// Original group address, matched without ASCII case sensitivity.
    pub email: String,
    /// Complete member list or atomic fallback.
    pub outcome: GroupExpansionOutcome,
}

/// Deduplicated attendees and completeness metadata after group expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttendees {
    /// Direct invitations followed by unique derived members.
    pub attendees: Vec<Attendee>,
    /// Completeness and consent requirements of matched group outcomes.
    pub resolution: ParticipantResolution,
}

/// Merges direct invitations with atomic group resolution outcomes.
///
/// Complete outcomes replace their original invitation, including empty groups. Incomplete
/// outcomes retain the whole original attendee and expose their resolution status. Invitations
/// without an outcome remain direct. Direct attendees always win over derived duplicates with
/// every field unchanged; the first direct duplicate wins. Among derived duplicates, the first
/// member's email and name win, while any required root makes attendance required. Derived
/// members always have `NeedsAction`, and `is_self` comes only from `authenticated_email`.
///
/// Edge cases: comparisons ignore ASCII email case; the first outcome for an address wins and
/// outcomes without a matching invitation are ignored. Blank member names fall back to the email
/// name helper. Addresses are not validated, and this function does not traverse groups or accept
/// partial member lists: the caller must turn incomplete traversals into `Incomplete` outcomes.
pub fn merge_group_attendees(
    attendees: &[Attendee],
    expansions: &[GroupExpansion],
    authenticated_email: &str,
) -> ResolvedAttendees {
    let mut outcomes = HashMap::with_capacity(expansions.len());
    for expansion in expansions {
        outcomes
            .entry(expansion.email.to_ascii_lowercase())
            .or_insert(&expansion.outcome);
    }

    let mut merged = Vec::with_capacity(attendees.len());
    let mut direct_indices = HashMap::with_capacity(attendees.len());
    let mut resolution = ParticipantResolution::default();
    for attendee in attendees {
        let key = attendee.email.to_ascii_lowercase();
        match outcomes.get(&key) {
            Some(GroupExpansionOutcome::Complete(_)) => continue,
            Some(GroupExpansionOutcome::Incomplete {
                reauthorization_required,
            }) => {
                resolution.incomplete = true;
                resolution.reauthorization_required |= reauthorization_required;
            }
            None => {}
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = direct_indices.entry(key) {
            entry.insert(merged.len());
            merged.push(attendee.clone());
        }
    }

    let mut derived_indices: HashMap<String, usize> = HashMap::new();
    for attendee in attendees {
        let key = attendee.email.to_ascii_lowercase();
        let Some(GroupExpansionOutcome::Complete(members)) = outcomes.get(&key) else {
            continue;
        };
        for member in members {
            let key = member.email.to_ascii_lowercase();
            if direct_indices.contains_key(&key) {
                continue;
            }
            if let Some(index) = derived_indices.get(&key) {
                if let Some(derived) = merged.get_mut(*index) {
                    derived.optional &= attendee.optional;
                }
                continue;
            }
            let name = match member.name.as_deref() {
                Some(name) if !name.trim().is_empty() => name.to_string(),
                _ => display_name_from_email(&member.email),
            };
            derived_indices.insert(key, merged.len());
            merged.push(Attendee {
                email: member.email.clone(),
                name,
                is_self: member.email.eq_ignore_ascii_case(authenticated_email),
                optional: attendee.optional,
                response: ResponseStatus::NeedsAction,
            });
        }
    }
    ResolvedAttendees {
        attendees: merged,
        resolution,
    }
}

/// Builds a readable name from an email address.
///
/// Takes the part before the first `@` (the whole string when there is none), splits it on `.`,
/// `_` and `-`, drops empty parts, uppercases the first character of each part and joins the
/// parts with a space. The rest of each part is left unchanged.
///
/// Edge cases: when no part is left (for example `@x` or an empty string) the original email is
/// returned. It does not validate the email and does not strip `+tag` suffixes.
pub fn display_name_from_email(email: &str) -> String {
    let local = match email.split_once('@') {
        Some((local, _)) => local,
        None => email,
    };

    let mut name = String::with_capacity(local.len());
    for part in local.split(['.', '_', '-']) {
        let mut chars = part.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        if !name.is_empty() {
            name.push(' ');
        }
        name.extend(first.to_uppercase());
        name.push_str(chars.as_str());
    }

    if name.is_empty() {
        email.to_string()
    } else {
        name
    }
}

/// Shortens a person's name to fit in a small space.
///
/// Level 0 returns the trimmed name. Level 1 keeps the first word and turns the others into
/// initials: `Andrea Zanella Rossi` becomes `Andrea Z. R.`. Level 2 or more turns every word into
/// an initial with no spaces: `A.Z.R.`. Words are separated by whitespace and initials are the
/// first `char` of each word, so non-ASCII names are safe.
///
/// Edge cases: a single-word name is returned trimmed and unchanged at every level. An empty or
/// blank name gives an empty string.
pub fn abbreviate_name(name: &str, level: u8) -> String {
    let trimmed = name.trim();
    if level == 0 {
        return trimmed.to_string();
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() < 2 {
        return trimmed.to_string();
    }

    let mut out = String::with_capacity(trimmed.len());
    for (position, word) in words.iter().enumerate() {
        if level == 1 {
            if position == 0 {
                out.push_str(word);
                continue;
            }
            out.push(' ');
        }
        if let Some(initial) = word.chars().next() {
            out.push(initial);
            out.push('.');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invitation(email: &str) -> Attendee {
        Attendee {
            email: email.to_string(),
            name: format!("Original {email}"),
            is_self: false,
            optional: false,
            response: ResponseStatus::Accepted,
        }
    }

    fn expanded(email: &str, members: &[&str]) -> GroupExpansion {
        GroupExpansion {
            email: email.to_string(),
            outcome: GroupExpansionOutcome::Complete(
                members
                    .iter()
                    .map(|email| GroupMember {
                        email: (*email).to_string(),
                        name: None,
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn empty_input_has_no_warning() {
        let result = merge_group_attendees(&[], &[], "me@example.com");
        assert!(result.attendees.is_empty());
        assert_eq!(result.resolution, ParticipantResolution::default());
    }

    #[test]
    fn complete_group_replaces_original_with_needs_action_members() {
        let root = invitation("team@example.com");
        let expansion = expanded("TEAM@example.com", &["anna.rossi@example.com"]);

        let result = merge_group_attendees(&[root], &[expansion], "someone@example.com");

        assert_eq!(
            result.attendees,
            vec![Attendee {
                email: "anna.rossi@example.com".to_string(),
                name: "Anna Rossi".to_string(),
                is_self: false,
                optional: false,
                response: ResponseStatus::NeedsAction,
            }]
        );
        assert_eq!(result.resolution, ParticipantResolution::default());
    }

    #[test]
    fn complete_empty_group_removes_original_invitation() {
        let root = invitation("team@example.com");
        let expansion = expanded(&root.email, &[]);

        let result = merge_group_attendees(&[root], &[expansion], "me@example.com");

        assert!(result.attendees.is_empty());
        assert!(!result.resolution.incomplete);
    }

    #[test]
    fn direct_metadata_wins_before_or_after_group_invitation() {
        let root = invitation("team@example.com");
        let direct = Attendee {
            email: "PERSON@example.com".to_string(),
            name: "Custom Calendar Name".to_string(),
            is_self: true,
            optional: true,
            response: ResponseStatus::Declined,
        };
        let expansion = expanded(&root.email, &["person@example.com"]);
        for invitations in [
            vec![root.clone(), direct.clone()],
            vec![direct.clone(), root],
        ] {
            let result = merge_group_attendees(
                &invitations,
                std::slice::from_ref(&expansion),
                "different@example.com",
            );
            assert_eq!(result.attendees, vec![direct.clone()]);
        }
    }

    #[test]
    fn first_direct_duplicate_preserves_its_whole_metadata() {
        let first = invitation("PERSON@example.com");
        let mut duplicate = invitation("person@example.com");
        duplicate.name = "Other name".to_string();
        duplicate.response = ResponseStatus::Tentative;
        duplicate.optional = true;

        let result = merge_group_attendees(&[first.clone(), duplicate], &[], "me@example.com");

        assert_eq!(result.attendees, vec![first]);
    }

    #[test]
    fn required_derived_duplicate_wins_in_either_root_order() {
        let required = invitation("required@example.com");
        let mut optional = invitation("optional@example.com");
        optional.optional = true;
        let expansions = [
            expanded(&required.email, &["PERSON@example.com"]),
            expanded(&optional.email, &["person@example.com"]),
        ];
        for invitations in [
            vec![optional.clone(), required.clone()],
            vec![required, optional],
        ] {
            let result = merge_group_attendees(&invitations, &expansions, "me@example.com");
            assert_eq!(result.attendees.len(), 1);
            assert!(!result.attendees.first().unwrap().optional);
        }
    }

    #[test]
    fn optional_group_members_remain_optional() {
        let mut root = invitation("optional@example.com");
        root.optional = true;
        let expansion = expanded(&root.email, &["person@example.com"]);

        let result = merge_group_attendees(&[root], &[expansion], "me@example.com");

        assert!(result.attendees.first().unwrap().optional);
    }

    #[test]
    fn derived_self_comes_from_authenticated_email_ignoring_group_self_flag() {
        let mut root = invitation("team@example.com");
        root.is_self = true;
        let expansion = expanded(&root.email, &["ME@example.com", "other@example.com"]);

        let result = merge_group_attendees(&[root], &[expansion], "me@EXAMPLE.COM");

        assert!(result.attendees.first().unwrap().is_self);
        assert!(!result.attendees.get(1).unwrap().is_self);
    }

    #[test]
    fn repeated_members_keep_first_name_and_email() {
        let root = invitation("team@example.com");
        let expansion = GroupExpansion {
            email: root.email.clone(),
            outcome: GroupExpansionOutcome::Complete(vec![
                GroupMember {
                    email: "PERSON@example.com".to_string(),
                    name: Some("Directory Name".to_string()),
                },
                GroupMember {
                    email: "person@example.com".to_string(),
                    name: Some("Different Name".to_string()),
                },
            ]),
        };

        let result = merge_group_attendees(&[root], &[expansion], "me@example.com");

        assert_eq!(result.attendees.len(), 1);
        assert_eq!(result.attendees.first().unwrap().name, "Directory Name");
        assert_eq!(
            result.attendees.first().unwrap().email,
            "PERSON@example.com"
        );
    }

    #[test]
    fn blank_derived_name_falls_back_to_email_name() {
        let root = invitation("team@example.com");
        let expansion = GroupExpansion {
            email: root.email.clone(),
            outcome: GroupExpansionOutcome::Complete(vec![GroupMember {
                email: "anna.rossi@example.com".to_string(),
                name: Some("  \t".to_string()),
            }]),
        };

        let result = merge_group_attendees(&[root], &[expansion], "me@example.com");

        assert_eq!(result.attendees.first().unwrap().name, "Anna Rossi");
    }

    #[test]
    fn incomplete_root_preserves_original_and_reports_reauthorization() {
        let root = invitation("team@example.com");
        let expansion = GroupExpansion {
            email: root.email.clone(),
            outcome: GroupExpansionOutcome::Incomplete {
                reauthorization_required: true,
            },
        };

        let result =
            merge_group_attendees(std::slice::from_ref(&root), &[expansion], "me@example.com");

        assert_eq!(result.attendees, vec![root]);
        assert!(result.resolution.incomplete);
        assert!(result.resolution.reauthorization_required);
    }

    #[test]
    fn incomplete_root_does_not_drop_independent_successes_or_direct_attendees() {
        let failed = invitation("failed@example.com");
        let direct = invitation("person@example.com");
        let successful = invitation("success@example.com");
        let expansions = [
            GroupExpansion {
                email: failed.email.clone(),
                outcome: GroupExpansionOutcome::Incomplete {
                    reauthorization_required: false,
                },
            },
            expanded(
                &successful.email,
                &["new@example.com", "person@example.com"],
            ),
        ];

        let result = merge_group_attendees(
            &[failed.clone(), successful, direct.clone()],
            &expansions,
            "me@example.com",
        );

        assert_eq!(result.attendees.len(), 3);
        assert_eq!(result.attendees.first(), Some(&failed));
        assert_eq!(result.attendees.get(1), Some(&direct));
        assert_eq!(result.attendees.get(2).unwrap().email, "new@example.com");
        assert!(result.resolution.incomplete);
        assert!(!result.resolution.reauthorization_required);
    }

    #[test]
    fn unmatched_outcomes_do_not_change_attendees_or_status() {
        let direct = invitation("person@example.com");
        let expansion = GroupExpansion {
            email: "missing@example.com".to_string(),
            outcome: GroupExpansionOutcome::Incomplete {
                reauthorization_required: true,
            },
        };

        let result = merge_group_attendees(
            std::slice::from_ref(&direct),
            &[expansion],
            "me@example.com",
        );

        assert_eq!(result.attendees, vec![direct]);
        assert_eq!(result.resolution, ParticipantResolution::default());
    }

    #[test]
    fn first_outcome_for_an_address_wins() {
        let root = invitation("team@example.com");
        let expansions = [
            expanded(&root.email, &[]),
            GroupExpansion {
                email: "TEAM@example.com".to_string(),
                outcome: GroupExpansionOutcome::Incomplete {
                    reauthorization_required: true,
                },
            },
        ];

        let result = merge_group_attendees(&[root], &expansions, "me@example.com");

        assert!(result.attendees.is_empty());
        assert_eq!(result.resolution, ParticipantResolution::default());
    }

    #[test]
    fn abbreviate_level_zero_returns_trimmed_name() {
        assert_eq!(abbreviate_name("  Andrea Zanella ", 0), "Andrea Zanella");
    }

    #[test]
    fn abbreviate_level_one_keeps_first_word() {
        assert_eq!(abbreviate_name("Andrea Zanella Rossi", 1), "Andrea Z. R.");
    }

    #[test]
    fn abbreviate_level_two_uses_only_initials() {
        assert_eq!(abbreviate_name("Andrea Zanella Rossi", 2), "A.Z.R.");
    }

    #[test]
    fn abbreviate_level_above_two_behaves_like_two() {
        assert_eq!(abbreviate_name("Andrea Zanella Rossi", 9), "A.Z.R.");
    }

    #[test]
    fn abbreviate_single_word_is_unchanged_at_every_level() {
        assert_eq!(abbreviate_name("Andrea", 0), "Andrea");
        assert_eq!(abbreviate_name("Andrea", 1), "Andrea");
        assert_eq!(abbreviate_name("Andrea", 2), "Andrea");
    }

    #[test]
    fn abbreviate_empty_gives_empty() {
        assert_eq!(abbreviate_name("", 0), "");
        assert_eq!(abbreviate_name("   ", 1), "");
        assert_eq!(abbreviate_name("", 2), "");
    }

    #[test]
    fn abbreviate_collapses_repeated_spaces() {
        assert_eq!(abbreviate_name("Anna   Maria", 1), "Anna M.");
    }

    #[test]
    fn abbreviate_is_unicode_safe() {
        assert_eq!(abbreviate_name("Émile Ångström", 1), "Émile Å.");
        assert_eq!(abbreviate_name("Émile Ångström", 2), "É.Å.");
    }

    #[test]
    fn splits_dotted_local_part() {
        assert_eq!(
            display_name_from_email("anna.rossi@openmove.com"),
            "Anna Rossi"
        );
    }

    #[test]
    fn splits_on_underscore_and_dash() {
        assert_eq!(
            display_name_from_email("mario_de-luca@x.it"),
            "Mario De Luca"
        );
    }

    #[test]
    fn returns_email_when_local_part_is_empty() {
        assert_eq!(display_name_from_email("@x"), "@x");
    }

    #[test]
    fn keeps_existing_uppercase() {
        assert_eq!(display_name_from_email("ANNA@x"), "ANNA");
    }

    #[test]
    fn works_without_at_sign() {
        assert_eq!(display_name_from_email("anna.rossi"), "Anna Rossi");
    }

    #[test]
    fn collapses_repeated_separators() {
        assert_eq!(display_name_from_email("..anna__-rossi.@x"), "Anna Rossi");
    }

    #[test]
    fn returns_email_when_only_separators() {
        assert_eq!(display_name_from_email("._-@x"), "._-@x");
    }

    #[test]
    fn capitalizes_non_ascii_initial() {
        assert_eq!(display_name_from_email("émile.zola@x.fr"), "Émile Zola");
    }

    #[test]
    fn uses_only_text_before_first_at() {
        assert_eq!(display_name_from_email("a.b@c@d"), "A B");
    }
}
