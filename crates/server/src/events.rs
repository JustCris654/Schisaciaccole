//! Mapping of Google Calendar events to the [`Meeting`]s of the API.

use chrono::{DateTime, FixedOffset};
use schisaciaccole_core::api::{Attendee, Meeting, ResponseStatus};
use schisaciaccole_core::attendee::display_name_from_email;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

/// Title of a meeting whose event has no summary.
pub const UNTITLED: &str = "(senza titolo)";

/// The fields of a Google Calendar event used to build a [`Meeting`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleEvent {
    id: String,
    status: Option<String>,
    summary: Option<String>,
    start: Option<EventTime>,
    end: Option<EventTime>,
    #[serde(default)]
    attendees: Vec<GoogleAttendee>,
    #[serde(default)]
    attendees_omitted: bool,
    recurring_event_id: Option<String>,
}

/// Start or end of an event. All-day events have a `date` and no
/// `dateTime`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventTime {
    date_time: Option<DateTime<FixedOffset>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleAttendee {
    email: Option<String>,
    display_name: Option<String>,
    #[serde(rename = "self", default)]
    is_self: bool,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    resource: bool,
    response_status: Option<String>,
}

/// Turns the `items` of Google Calendar event lists into meetings, keeping
/// their order.
///
/// Each item is mapped with [`map_event`], which lists the events left out
/// and the rules for titles and attendees. Duplicate events and events
/// outside the requested range are not filtered.
pub fn map_events(items: &[Value]) -> Vec<Meeting> {
    items.iter().filter_map(map_event).collect()
}

/// Turns one Google Calendar event into a meeting.
///
/// Returns `None` when the event is skipped:
/// - its `status` is `cancelled`;
/// - its start or end has no `dateTime` (all-day events);
/// - the attendee marked `self` has declined it;
/// - it cannot be parsed (missing `id`, wrong types, a `dateTime` that is
///   not RFC 3339); a warning without event data is logged.
///
/// In the meeting the title falls back to [`UNTITLED`] and rooms
/// (`resource` attendees) and attendees without email are left out. An
/// attendee's name is its `displayName`, or one derived from the email when
/// the display name is missing or blank. A missing or unknown
/// `responseStatus` becomes [`ResponseStatus::NeedsAction`]. `has_template`
/// is always `false` and `preference` is always `None`: the caller sets
/// `preference`.
pub fn map_event(item: &Value) -> Option<Meeting> {
    match GoogleEvent::deserialize(item) {
        Ok(event) => to_meeting(event),
        Err(_) => {
            warn!("skipping unparsable Google Calendar event");
            None
        }
    }
}

/// Builds the meeting of one event; `None` when [`map_event`] skips it.
fn to_meeting(event: GoogleEvent) -> Option<Meeting> {
    if event.status.as_deref() == Some("cancelled") {
        return None;
    }
    let start = event.start.and_then(|time| time.date_time)?;
    let end = event.end.and_then(|time| time.date_time)?;
    if is_declined_by_self(&event.attendees) {
        return None;
    }
    let attendees = event
        .attendees
        .into_iter()
        .filter_map(to_attendee)
        .collect();
    Some(Meeting {
        event_id: event.id,
        title: event.summary.unwrap_or_else(|| UNTITLED.to_string()),
        start,
        end,
        attendees,
        attendees_omitted: event.attendees_omitted,
        participant_resolution: Default::default(),
        recurring_event_id: event.recurring_event_id,
        has_template: false,
        preference: None,
    })
}

/// True when the attendee marked `self` answered `declined`.
fn is_declined_by_self(attendees: &[GoogleAttendee]) -> bool {
    attendees
        .iter()
        .any(|attendee| attendee.is_self && attendee.response_status.as_deref() == Some("declined"))
}

/// Builds an attendee; `None` for rooms and attendees without email.
fn to_attendee(attendee: GoogleAttendee) -> Option<Attendee> {
    if attendee.resource {
        return None;
    }
    let email = attendee.email?;
    let name = match attendee.display_name {
        Some(name) if !name.trim().is_empty() => name,
        _ => display_name_from_email(&email),
    };
    Some(Attendee {
        name,
        email,
        is_self: attendee.is_self,
        optional: attendee.optional,
        response: response_status(attendee.response_status.as_deref()),
    })
}

/// Maps Google's `responseStatus`; missing or unknown values are
/// [`ResponseStatus::NeedsAction`].
fn response_status(value: Option<&str>) -> ResponseStatus {
    match value {
        Some("accepted") => ResponseStatus::Accepted,
        Some("declined") => ResponseStatus::Declined,
        Some("tentative") => ResponseStatus::Tentative,
        _ => ResponseStatus::NeedsAction,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(id: &str) -> Value {
        json!({
            "id": id,
            "status": "confirmed",
            "summary": "Daily",
            "start": { "dateTime": "2026-11-05T09:30:00+01:00" },
            "end": { "dateTime": "2026-11-05T10:00:00+01:00" },
        })
    }

    fn with_attendees(mut event: Value, attendees: Value) -> Value {
        event["attendees"] = attendees;
        event
    }

    fn single(item: Value) -> Meeting {
        let meetings = map_events(&[item]);
        assert_eq!(meetings.len(), 1);
        meetings.into_iter().next().unwrap()
    }

    #[test]
    fn map_event_skips_cancelled_event() {
        let mut item = event("e1");
        item["status"] = json!("cancelled");

        assert!(map_event(&item).is_none());
    }

    #[test]
    fn map_event_skips_unparsable_event() {
        assert!(map_event(&json!({ "summary": "no id" })).is_none());
    }

    fn ids(meetings: &[Meeting]) -> Vec<&str> {
        meetings
            .iter()
            .map(|meeting| meeting.event_id.as_str())
            .collect()
    }

    #[test]
    fn maps_basic_event() {
        let meeting = single(event("e1"));

        assert_eq!(meeting.event_id, "e1");
        assert_eq!(meeting.title, "Daily");
        assert_eq!(
            meeting.start,
            DateTime::parse_from_rfc3339("2026-11-05T09:30:00+01:00").unwrap()
        );
        assert_eq!(meeting.start.offset().local_minus_utc(), 3600);
        assert_eq!(
            meeting.end,
            DateTime::parse_from_rfc3339("2026-11-05T10:00:00+01:00").unwrap()
        );
        assert!(meeting.attendees.is_empty());
        assert!(!meeting.attendees_omitted);
        assert_eq!(meeting.recurring_event_id, None);
    }

    #[test]
    fn keeps_google_order() {
        let meetings = map_events(&[event("b"), event("a"), event("c")]);

        assert_eq!(ids(&meetings), ["b", "a", "c"]);
    }

    #[test]
    fn skips_cancelled_event() {
        let mut cancelled = event("e1");
        cancelled["status"] = json!("cancelled");

        let meetings = map_events(&[cancelled, event("e2")]);

        assert_eq!(ids(&meetings), ["e2"]);
    }

    #[test]
    fn keeps_tentative_event() {
        let mut tentative = event("e1");
        tentative["status"] = json!("tentative");

        assert_eq!(single(tentative).event_id, "e1");
    }

    #[test]
    fn keeps_event_without_status() {
        let mut item = event("e1");
        item.as_object_mut().unwrap().remove("status");

        assert_eq!(single(item).event_id, "e1");
    }

    #[test]
    fn skips_all_day_event() {
        let mut all_day = event("e1");
        all_day["start"] = json!({ "date": "2026-11-05" });
        all_day["end"] = json!({ "date": "2026-11-06" });

        assert!(map_events(&[all_day]).is_empty());
    }

    #[test]
    fn skips_event_without_end_date_time() {
        let mut item = event("e1");
        item["end"] = json!({ "date": "2026-11-06" });

        assert!(map_events(&[item]).is_empty());
    }

    #[test]
    fn skips_event_without_start() {
        let mut item = event("e1");
        item.as_object_mut().unwrap().remove("start");

        assert!(map_events(&[item]).is_empty());
    }

    #[test]
    fn skips_event_declined_by_self() {
        let declined = with_attendees(
            event("e1"),
            json!([
                { "email": "ada@example.com", "self": true, "responseStatus": "declined" },
                { "email": "bob@example.com", "responseStatus": "accepted" },
            ]),
        );

        assert!(map_events(&[declined]).is_empty());
    }

    #[test]
    fn keeps_event_declined_by_someone_else() {
        let item = with_attendees(
            event("e1"),
            json!([
                { "email": "ada@example.com", "self": true, "responseStatus": "accepted" },
                { "email": "bob@example.com", "responseStatus": "declined" },
            ]),
        );

        assert_eq!(single(item).attendees.len(), 2);
    }

    #[test]
    fn untitled_event_gets_placeholder_title() {
        let mut item = event("e1");
        item.as_object_mut().unwrap().remove("summary");

        assert_eq!(single(item).title, "(senza titolo)");
    }

    #[test]
    fn drops_resource_attendees() {
        let item = with_attendees(
            event("e1"),
            json!([
                { "email": "room@resource.calendar.google.com", "resource": true },
                { "email": "bob@example.com" },
            ]),
        );

        let meeting = single(item);

        assert_eq!(meeting.attendees.len(), 1);
        assert_eq!(meeting.attendees[0].email, "bob@example.com");
    }

    #[test]
    fn drops_attendee_without_email() {
        let item = with_attendees(
            event("e1"),
            json!([
                { "displayName": "Nobody" },
                { "email": "bob@example.com" },
            ]),
        );

        let meeting = single(item);

        assert_eq!(meeting.attendees.len(), 1);
        assert_eq!(meeting.attendees[0].email, "bob@example.com");
    }

    #[test]
    fn attendee_name_is_display_name() {
        let item = with_attendees(
            event("e1"),
            json!([{ "email": "bob@example.com", "displayName": "Roberto B." }]),
        );

        assert_eq!(single(item).attendees[0].name, "Roberto B.");
    }

    #[test]
    fn attendee_name_falls_back_to_email() {
        let item = with_attendees(event("e1"), json!([{ "email": "anna.rossi@example.com" }]));

        assert_eq!(single(item).attendees[0].name, "Anna Rossi");
    }

    #[test]
    fn blank_display_name_falls_back_to_email() {
        let item = with_attendees(
            event("e1"),
            json!([{ "email": "anna.rossi@example.com", "displayName": "  " }]),
        );

        assert_eq!(single(item).attendees[0].name, "Anna Rossi");
    }

    #[test]
    fn attendee_flags_default_to_false_and_needs_action() {
        let item = with_attendees(event("e1"), json!([{ "email": "bob@example.com" }]));

        let attendee = &single(item).attendees[0];

        assert!(!attendee.is_self);
        assert!(!attendee.optional);
        assert_eq!(attendee.response, ResponseStatus::NeedsAction);
    }

    #[test]
    fn attendee_flags_are_copied() {
        let item = with_attendees(
            event("e1"),
            json!([{ "email": "ada@example.com", "self": true, "optional": true }]),
        );

        let attendee = &single(item).attendees[0];

        assert!(attendee.is_self);
        assert!(attendee.optional);
    }

    #[test]
    fn known_response_statuses_are_mapped() {
        let item = with_attendees(
            event("e1"),
            json!([
                { "email": "a@example.com", "responseStatus": "needsAction" },
                { "email": "b@example.com", "responseStatus": "declined" },
                { "email": "c@example.com", "responseStatus": "tentative" },
                { "email": "d@example.com", "responseStatus": "accepted" },
            ]),
        );

        let responses: Vec<ResponseStatus> = single(item)
            .attendees
            .iter()
            .map(|attendee| attendee.response)
            .collect();

        assert_eq!(
            responses,
            [
                ResponseStatus::NeedsAction,
                ResponseStatus::Declined,
                ResponseStatus::Tentative,
                ResponseStatus::Accepted,
            ]
        );
    }

    #[test]
    fn unknown_response_status_is_needs_action() {
        let item = with_attendees(
            event("e1"),
            json!([{ "email": "bob@example.com", "responseStatus": "maybe-later" }]),
        );

        assert_eq!(
            single(item).attendees[0].response,
            ResponseStatus::NeedsAction
        );
    }

    #[test]
    fn copies_attendees_omitted_and_recurring_event_id() {
        let mut item = event("e1_20261105T083000Z");
        item["attendeesOmitted"] = json!(true);
        item["recurringEventId"] = json!("e1");

        let meeting = single(item);

        assert!(meeting.attendees_omitted);
        assert_eq!(meeting.recurring_event_id.as_deref(), Some("e1"));
    }

    #[test]
    fn skips_event_with_unparsable_date_time() {
        let mut broken = event("e1");
        broken["start"] = json!({ "dateTime": "tomorrow morning" });

        let meetings = map_events(&[broken, event("e2")]);

        assert_eq!(ids(&meetings), ["e2"]);
    }

    #[test]
    fn skips_event_without_id() {
        let mut broken = event("e1");
        broken.as_object_mut().unwrap().remove("id");

        let meetings = map_events(&[broken, event("e2")]);

        assert_eq!(ids(&meetings), ["e2"]);
    }

    #[test]
    fn skips_event_with_wrongly_typed_attendees() {
        let broken = with_attendees(event("e1"), json!("not a list"));

        let meetings = map_events(&[broken, event("e2")]);

        assert_eq!(ids(&meetings), ["e2"]);
    }

    #[test]
    fn skips_item_that_is_not_an_object() {
        let meetings = map_events(&[json!(42), event("e2")]);

        assert_eq!(ids(&meetings), ["e2"]);
    }
}
