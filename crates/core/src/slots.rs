//! People who take part in a meeting.
//!
//! Builds the participant list from a meeting's attendees. Nothing here reads the clock.

use crate::api::{Meeting, ResponseStatus};
use crate::attendee::display_name_from_email;

/// A person who takes part in the meeting and gets a speaking slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    /// Display name, never empty unless the email is empty too.
    pub name: String,
    /// Email address of the participant.
    pub email: String,
}

/// Lists the people who should get a slot, sorted by name.
///
/// Keeps attendees whose response is not [`ResponseStatus::Declined`], plus the attendee flagged
/// `is_self` even when declined. The name is the trimmed attendee name; when it is empty the name
/// is built from the email with [`display_name_from_email`]. The order is by lowercase name, ties
/// broken by email.
///
/// Edge cases: a meeting without attendees gives an empty list. Attendees omitted by the calendar
/// (`attendees_omitted`) are not known and not listed. Duplicate emails are not merged.
pub fn participants(meeting: &Meeting) -> Vec<Participant> {
    let mut list = Vec::with_capacity(meeting.attendees.len());
    for attendee in &meeting.attendees {
        if attendee.response == ResponseStatus::Declined && !attendee.is_self {
            continue;
        }
        let trimmed = attendee.name.trim();
        let name = if trimmed.is_empty() {
            display_name_from_email(&attendee.email)
        } else {
            trimmed.to_string()
        };
        list.push(Participant {
            name,
            email: attendee.email.clone(),
        });
    }
    list.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.email.cmp(&b.email))
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Attendee;
    use chrono::{DateTime, FixedOffset, NaiveDate, TimeDelta, Utc};

    fn utc(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 3, 10)
            .unwrap()
            .and_hms_opt(hour, minute, second)
            .unwrap()
            .and_utc()
    }

    fn attendee(name: &str, email: &str, response: ResponseStatus, is_self: bool) -> Attendee {
        Attendee {
            email: email.to_string(),
            name: name.to_string(),
            is_self,
            optional: false,
            response,
        }
    }

    fn meeting(attendees: Vec<Attendee>) -> Meeting {
        let start = utc(10, 0, 0).with_timezone(&FixedOffset::east_opt(0).unwrap());
        Meeting {
            event_id: "e".to_string(),
            title: "t".to_string(),
            start,
            end: start + TimeDelta::hours(1),
            attendees,
            attendees_omitted: false,
            participant_resolution: Default::default(),
            recurring_event_id: None,
            has_template: false,
            preference: None,
        }
    }

    fn names(list: &[Participant]) -> Vec<&str> {
        list.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn excludes_declined_attendee() {
        let m = meeting(vec![
            attendee("Ann", "ann@x.it", ResponseStatus::Accepted, false),
            attendee("Bob", "bob@x.it", ResponseStatus::Declined, false),
        ]);

        assert_eq!(names(&participants(&m)), vec!["Ann"]);
    }

    #[test]
    fn includes_self_even_when_declined() {
        let m = meeting(vec![
            attendee("Me", "me@x.it", ResponseStatus::Declined, true),
            attendee("Ann", "ann@x.it", ResponseStatus::Tentative, false),
        ]);

        assert_eq!(names(&participants(&m)), vec!["Ann", "Me"]);
    }

    #[test]
    fn empty_name_falls_back_to_email_name() {
        let m = meeting(vec![attendee(
            "",
            "mario.rossi@x.it",
            ResponseStatus::NeedsAction,
            false,
        )]);

        assert_eq!(names(&participants(&m)), vec!["Mario Rossi"]);
    }

    #[test]
    fn whitespace_only_name_falls_back_to_email_name() {
        let m = meeting(vec![attendee(
            "   ",
            "mario.rossi@x.it",
            ResponseStatus::Accepted,
            false,
        )]);

        assert_eq!(names(&participants(&m)), vec!["Mario Rossi"]);
    }

    #[test]
    fn trims_name() {
        let m = meeting(vec![attendee(
            "  Ann ",
            "ann@x.it",
            ResponseStatus::Accepted,
            false,
        )]);

        assert_eq!(names(&participants(&m)), vec!["Ann"]);
    }

    #[test]
    fn sorts_by_name_ignoring_case() {
        let m = meeting(vec![
            attendee("bob", "b@x.it", ResponseStatus::Accepted, false),
            attendee("Carl", "c@x.it", ResponseStatus::Accepted, false),
            attendee("Ann", "a@x.it", ResponseStatus::Accepted, false),
        ]);

        assert_eq!(names(&participants(&m)), vec!["Ann", "bob", "Carl"]);
    }

    #[test]
    fn breaks_name_ties_by_email() {
        let m = meeting(vec![
            attendee("Ann", "z@x.it", ResponseStatus::Accepted, false),
            attendee("ann", "a@x.it", ResponseStatus::Accepted, false),
        ]);

        let emails: Vec<String> = participants(&m).into_iter().map(|p| p.email).collect();

        assert_eq!(emails, vec!["a@x.it", "z@x.it"]);
    }
}
