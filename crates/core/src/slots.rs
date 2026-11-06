//! Speaking slots for a meeting countdown.
//!
//! Splits the time left in a meeting among its participants. Slot ends are absolute instants,
//! so a suspended laptop does not stall the schedule. Every slot end lies a whole number of
//! seconds before the meeting end, so the slot countdown and the meeting countdown change digit
//! together. Nothing here reads the clock.

use crate::api::{Meeting, ResponseStatus};
use crate::attendee::display_name_from_email;
use chrono::{DateTime, TimeDelta, TimeZone};

/// Shortest slot, in milliseconds, accepted when a schedule is created.
pub const MIN_SLOT_MS: i64 = 10_000;

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

/// Splits `total_ms` among `weights` proportionally.
///
/// Each share is `floor(total_ms * weight / sum_of_weights)`; the rounding remainder is added to
/// the last share, so the shares always add up to `total_ms`. The math is done in 128 bits and
/// cannot overflow.
///
/// Edge cases: returns `None` for empty `weights`, a zero sum of weights or `total_ms <= 0`.
/// A zero weight gets a zero share, plus the remainder when it is the last entry.
pub fn allocate_ms(weights: &[u64], total_ms: i64) -> Option<Vec<i64>> {
    if weights.is_empty() || total_ms <= 0 {
        return None;
    }
    let sum: u128 = weights.iter().map(|&weight| u128::from(weight)).sum();
    if sum == 0 {
        return None;
    }
    let total = u128::try_from(total_ms).ok()?;

    let mut shares = Vec::with_capacity(weights.len());
    let mut assigned: i64 = 0;
    for &weight in weights {
        let share = i64::try_from(total * u128::from(weight) / sum).ok()?;
        assigned = assigned.checked_add(share)?;
        shares.push(share);
    }
    let remainder = total_ms.checked_sub(assigned)?;
    let last = shares.last_mut()?;
    *last = last.checked_add(remainder)?;
    Some(shares)
}

/// Splits `total_ms` among `weights` in whole seconds, with the sub-second part on the first share.
///
/// The whole seconds of `total_ms` are split with [`allocate_ms`] (so the seconds remainder goes
/// to the last share) and converted to milliseconds; the `total_ms % 1000` leftover is added to
/// the first share. The shares add up to `total_ms` and, summed from the end, every partial sum
/// is a whole number of seconds.
///
/// Edge cases: returns `None` for empty `weights`, a zero sum of weights or less than one whole
/// second in `total_ms` (zero or negative included).
fn allocate_whole_seconds(weights: &[u64], total_ms: i64) -> Option<Vec<i64>> {
    let whole_secs = total_ms / 1000;
    let extra_ms = total_ms % 1000;
    let secs = allocate_ms(weights, whole_secs)?;

    let mut shares = Vec::with_capacity(secs.len());
    for sec in secs {
        shares.push(sec.checked_mul(1000)?);
    }
    let first = shares.first_mut()?;
    *first = first.checked_add(extra_ms)?;
    Some(shares)
}

/// Builds cumulative slot ends starting at `from`; the last end is exactly `meeting_end`.
///
/// Returns `None` when an instant is out of the representable range.
fn cumulative_ends<Tz: TimeZone>(
    from: &DateTime<Tz>,
    shares: &[i64],
    meeting_end: &DateTime<Tz>,
) -> Option<Vec<DateTime<Tz>>> {
    let mut ends = Vec::with_capacity(shares.len());
    let mut elapsed_ms: i64 = 0;
    for (position, share) in shares.iter().enumerate() {
        if position + 1 == shares.len() {
            ends.push(meeting_end.clone());
        } else {
            elapsed_ms = elapsed_ms.checked_add(*share)?;
            let delta = TimeDelta::try_milliseconds(elapsed_ms)?;
            ends.push(from.clone().checked_add_signed(delta)?);
        }
    }
    Some(ends)
}

/// One speaking slot of a schedule.
#[derive(Debug, Clone)]
struct Slot<Tz: TimeZone> {
    name: String,
    weight: u64,
    end: DateTime<Tz>,
}

/// What is happening now in a [`SlotSchedule`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotView {
    /// Position of the current slot in the schedule.
    pub index: usize,
    /// Name of the person speaking.
    pub name: String,
    /// Milliseconds left in the current slot; always at least 1.
    pub remaining_ms: i64,
    /// Name of the following speaker, `None` on the last slot.
    pub next_name: Option<String>,
}

/// Ordered speaking slots that fill the time up to the end of a meeting.
#[derive(Debug, Clone)]
pub struct SlotSchedule<Tz: TimeZone> {
    slots: Vec<Slot<Tz>>,
    meeting_end: DateTime<Tz>,
}

impl<Tz: TimeZone> SlotSchedule<Tz> {
    /// Splits the time from `now` to `meeting_end` among `slots`, given as `(name, weight)`.
    ///
    /// Slot ends are cumulative from `now` and the last one equals `meeting_end` exactly. The
    /// whole seconds are split by weight; the sub-second part of `meeting_end - now` goes to the
    /// first slot, so every slot end is a whole number of seconds before `meeting_end`.
    ///
    /// Edge cases: returns `None` (the caller should run a plain timer) with fewer than 2 slots,
    /// when `meeting_end` is not at least one second after `now`, when all weights are zero, or when the shortest
    /// slot would be shorter than [`MIN_SLOT_MS`]. A zero weight gives a zero-length slot, which
    /// also makes the schedule fail the minimum check.
    pub fn new(
        slots: Vec<(String, u64)>,
        now: &DateTime<Tz>,
        meeting_end: &DateTime<Tz>,
    ) -> Option<Self> {
        if slots.len() < 2 {
            return None;
        }
        let total_ms = meeting_end
            .clone()
            .signed_duration_since(now.clone())
            .num_milliseconds();
        let weights: Vec<u64> = slots.iter().map(|(_, weight)| *weight).collect();
        let shares = allocate_whole_seconds(&weights, total_ms)?;
        if shares.iter().any(|share| *share < MIN_SLOT_MS) {
            return None;
        }
        let ends = cumulative_ends(now, &shares, meeting_end)?;

        let slots = slots
            .into_iter()
            .zip(ends)
            .map(|((name, weight), end)| Slot { name, weight, end })
            .collect();
        Some(Self {
            slots,
            meeting_end: meeting_end.clone(),
        })
    }

    /// Number of slots in the schedule.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// True when the schedule has no slots; never the case for one built by [`Self::new`].
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn current_index(&self, now: &DateTime<Tz>) -> Option<usize> {
        self.slots.iter().position(|slot| slot.end > *now)
    }

    /// Returns the slot running at `now`: the first one whose end is after `now`.
    ///
    /// Edge cases: at exactly a slot end the next slot is current. Returns `None` once every
    /// slot is over (`now` at or after the meeting end). If `now` is far ahead of the schedule
    /// (a suspended laptop) the slot that actually covers `now` is returned, not the one that
    /// was running before.
    pub fn current(&self, now: &DateTime<Tz>) -> Option<SlotView> {
        let index = self.current_index(now)?;
        let slot = self.slots.get(index)?;
        let next_name = self.slots.get(index + 1).map(|next| next.name.clone());
        Some(SlotView {
            index,
            name: slot.name.clone(),
            remaining_ms: slot
                .end
                .clone()
                .signed_duration_since(now.clone())
                .num_milliseconds(),
            next_name,
        })
    }

    /// Ends the current slot at `now` and shares the time left among the following slots.
    ///
    /// The following slots are split over `meeting_end - now` by their weights, with cumulative
    /// ends from `now` and the last end equal to the meeting end. The whole seconds are split by
    /// weight and the sub-second part goes to the first following slot, so every end is a whole
    /// number of seconds before the meeting end. There is no minimum-length check, so slots never
    /// disappear mid-meeting.
    ///
    /// Edge cases: returns `false` and changes nothing when there is no current slot, when the
    /// current slot is the last one, when less than one second is left, or when the time left
    /// cannot be split (all remaining weights zero).
    pub fn skip(&mut self, now: &DateTime<Tz>) -> bool {
        let Some(index) = self.current_index(now) else {
            return false;
        };
        let Some(rest) = self.slots.get(index + 1..) else {
            return false;
        };
        if rest.is_empty() {
            return false;
        }
        let weights: Vec<u64> = rest.iter().map(|slot| slot.weight).collect();
        let total_ms = self
            .meeting_end
            .clone()
            .signed_duration_since(now.clone())
            .num_milliseconds();
        let Some(shares) = allocate_whole_seconds(&weights, total_ms) else {
            return false;
        };
        let Some(ends) = cumulative_ends(now, &shares, &self.meeting_end) else {
            return false;
        };

        if let Some(current) = self.slots.get_mut(index) {
            current.end = now.clone();
        }
        for (slot, end) in self.slots.iter_mut().skip(index + 1).zip(ends) {
            slot.end = end;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Attendee;
    use chrono::{FixedOffset, NaiveDate, Utc};

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
            recurring_event_id: None,
            has_template: false,
        }
    }

    fn names(list: &[Participant]) -> Vec<&str> {
        list.iter().map(|p| p.name.as_str()).collect()
    }

    fn named(count: usize) -> Vec<(String, u64)> {
        (0..count).map(|i| (format!("p{i}"), 1)).collect()
    }

    fn ends(schedule: &SlotSchedule<Utc>) -> Vec<DateTime<Utc>> {
        schedule.slots.iter().map(|slot| slot.end).collect()
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

    #[test]
    fn allocates_exact_split() {
        assert_eq!(allocate_ms(&[1, 1, 1], 30_000), Some(vec![10_000; 3]));
    }

    #[test]
    fn allocates_by_weight() {
        assert_eq!(allocate_ms(&[1, 3], 40_000), Some(vec![10_000, 30_000]));
    }

    #[test]
    fn remainder_goes_to_last_share() {
        assert_eq!(allocate_ms(&[1, 1, 1], 10), Some(vec![3, 3, 4]));
    }

    #[test]
    fn zero_weight_gets_zero_share() {
        assert_eq!(allocate_ms(&[0, 1, 1], 10), Some(vec![0, 5, 5]));
    }

    #[test]
    fn allocate_rejects_zero_total() {
        assert_eq!(allocate_ms(&[1, 1], 0), None);
    }

    #[test]
    fn allocate_rejects_negative_total() {
        assert_eq!(allocate_ms(&[1, 1], -5), None);
    }

    #[test]
    fn allocate_rejects_empty_weights() {
        assert_eq!(allocate_ms(&[], 1_000), None);
    }

    #[test]
    fn allocate_rejects_zero_sum() {
        assert_eq!(allocate_ms(&[0, 0], 1_000), None);
    }

    #[test]
    fn allocate_handles_large_values_without_overflow() {
        let shares = allocate_ms(&[u64::MAX, u64::MAX], i64::MAX).unwrap();

        assert_eq!(shares.iter().sum::<i64>(), i64::MAX);
        assert_eq!(shares.first(), Some(&(i64::MAX / 2)));
    }

    #[test]
    fn new_rejects_single_slot() {
        let now = utc(10, 0, 0);

        assert!(SlotSchedule::new(named(1), &now, &utc(11, 0, 0)).is_none());
    }

    #[test]
    fn new_rejects_meeting_already_over() {
        let now = utc(10, 0, 0);

        assert!(SlotSchedule::new(named(2), &now, &now).is_none());
        assert!(SlotSchedule::new(named(2), &now, &utc(9, 0, 0)).is_none());
    }

    #[test]
    fn new_rejects_slot_shorter_than_minimum() {
        let now = utc(10, 0, 0);

        // 19 s over 2 slots gives 9.5 s each.
        assert!(SlotSchedule::new(named(2), &now, &utc(10, 0, 19)).is_none());
    }

    #[test]
    fn new_accepts_slot_of_exactly_minimum() {
        let now = utc(10, 0, 0);

        assert!(SlotSchedule::new(named(2), &now, &utc(10, 0, 20)).is_some());
    }

    #[test]
    fn new_rejects_zero_weight_slot() {
        let now = utc(10, 0, 0);
        let slots = vec![("a".to_string(), 0), ("b".to_string(), 1)];

        assert!(SlotSchedule::new(slots, &now, &utc(11, 0, 0)).is_none());
    }

    #[test]
    fn new_splits_equally_and_ends_at_meeting_end() {
        let end = utc(11, 0, 0);

        let schedule = SlotSchedule::new(named(4), &utc(10, 0, 0), &end).unwrap();

        assert_eq!(schedule.len(), 4);
        assert!(!schedule.is_empty());
        assert_eq!(
            ends(&schedule),
            vec![utc(10, 15, 0), utc(10, 30, 0), utc(10, 45, 0), end]
        );
    }

    #[test]
    fn late_start_rescales_from_now() {
        let end = utc(11, 0, 0);

        let schedule = SlotSchedule::new(named(3), &utc(10, 30, 0), &end).unwrap();

        assert_eq!(ends(&schedule), vec![utc(10, 40, 0), utc(10, 50, 0), end]);
    }

    #[test]
    fn new_puts_subsecond_remainder_on_first_slot() {
        let end = utc(10, 30, 0);
        let now = utc(10, 0, 0) + TimeDelta::milliseconds(437);

        let schedule = SlotSchedule::new(named(2), &now, &end).unwrap();

        // 1799.563 s: 899 s + 563 ms for the first slot, 900 s for the second.
        assert_eq!(ends(&schedule), vec![utc(10, 15, 0), end]);
        assert_eq!(schedule.current(&now).unwrap().remaining_ms, 899_563);
    }

    #[test]
    fn new_ends_are_whole_seconds_before_meeting_end() {
        let end = utc(10, 30, 0);
        let now = utc(10, 0, 0) + TimeDelta::milliseconds(437);

        let schedule = SlotSchedule::new(named(7), &now, &end).unwrap();

        for slot_end in ends(&schedule) {
            let distance = end.signed_duration_since(slot_end).num_milliseconds();
            assert_eq!(distance % 1000, 0);
        }
    }

    #[test]
    fn new_gives_seconds_remainder_to_last_slot() {
        let now = utc(10, 0, 0);

        let schedule = SlotSchedule::new(named(3), &now, &utc(10, 1, 40)).unwrap();

        // 100 s over 3 equal weights: 33 s, 33 s, 34 s.
        assert_eq!(
            ends(&schedule),
            vec![utc(10, 0, 33), utc(10, 1, 6), utc(10, 1, 40)]
        );
    }

    #[test]
    fn new_rejects_less_than_one_second_left() {
        let now = utc(10, 0, 0) + TimeDelta::milliseconds(1);

        assert!(SlotSchedule::new(named(2), &now, &utc(10, 0, 1)).is_none());
    }

    #[test]
    fn current_is_first_slot_at_start() {
        let now = utc(10, 0, 0);
        let schedule = SlotSchedule::new(named(2), &now, &utc(11, 0, 0)).unwrap();

        let view = schedule.current(&now).unwrap();

        assert_eq!(
            view,
            SlotView {
                index: 0,
                name: "p0".to_string(),
                remaining_ms: 1_800_000,
                next_name: Some("p1".to_string()),
            }
        );
    }

    #[test]
    fn current_at_exact_boundary_is_next_slot() {
        let schedule = SlotSchedule::new(named(2), &utc(10, 0, 0), &utc(11, 0, 0)).unwrap();

        let view = schedule.current(&utc(10, 30, 0)).unwrap();

        assert_eq!(view.index, 1);
        assert_eq!(view.remaining_ms, 1_800_000);
    }

    #[test]
    fn current_one_ms_before_boundary_is_still_previous_slot() {
        let schedule = SlotSchedule::new(named(2), &utc(10, 0, 0), &utc(11, 0, 0)).unwrap();

        let view = schedule
            .current(&(utc(10, 30, 0) - TimeDelta::milliseconds(1)))
            .unwrap();

        assert_eq!(view.index, 0);
        assert_eq!(view.remaining_ms, 1);
    }

    #[test]
    fn current_is_none_after_all_ends() {
        let end = utc(11, 0, 0);
        let schedule = SlotSchedule::new(named(2), &utc(10, 0, 0), &end).unwrap();

        assert!(schedule.current(&end).is_none());
        assert!(schedule.current(&utc(12, 0, 0)).is_none());
    }

    #[test]
    fn last_slot_has_no_next_name() {
        let schedule = SlotSchedule::new(named(2), &utc(10, 0, 0), &utc(11, 0, 0)).unwrap();

        let view = schedule.current(&utc(10, 45, 0)).unwrap();

        assert_eq!(view.index, 1);
        assert_eq!(view.next_name, None);
    }

    #[test]
    fn skip_redistributes_remaining_time_equally() {
        let end = utc(11, 0, 0);
        let mut schedule = SlotSchedule::new(named(4), &utc(10, 0, 0), &end).unwrap();

        let skipped = schedule.skip(&utc(10, 5, 0));

        assert!(skipped);
        assert_eq!(
            ends(&schedule),
            vec![utc(10, 5, 0), utc(10, 23, 20), utc(10, 41, 40), end]
        );
        assert_eq!(schedule.current(&utc(10, 5, 0)).unwrap().index, 1);
    }

    #[test]
    fn skip_respects_weights_of_following_slots() {
        let end = utc(11, 0, 0);
        let slots = vec![
            ("a".to_string(), 1),
            ("b".to_string(), 1),
            ("c".to_string(), 3),
        ];
        let mut schedule = SlotSchedule::new(slots, &utc(10, 0, 0), &end).unwrap();

        schedule.skip(&utc(10, 0, 0));

        assert_eq!(ends(&schedule), vec![utc(10, 0, 0), utc(10, 15, 0), end]);
    }

    #[test]
    fn skip_on_last_slot_returns_false_and_changes_nothing() {
        let mut schedule = SlotSchedule::new(named(2), &utc(10, 0, 0), &utc(11, 0, 0)).unwrap();
        let before = ends(&schedule);

        let skipped = schedule.skip(&utc(10, 45, 0));

        assert!(!skipped);
        assert_eq!(ends(&schedule), before);
    }

    #[test]
    fn skip_after_meeting_end_returns_false() {
        let mut schedule = SlotSchedule::new(named(2), &utc(10, 0, 0), &utc(11, 0, 0)).unwrap();
        let before = ends(&schedule);

        let skipped = schedule.skip(&utc(11, 0, 0));

        assert!(!skipped);
        assert_eq!(ends(&schedule), before);
    }

    #[test]
    fn skip_after_suspend_works_from_actual_current_slot() {
        let end = utc(11, 0, 0);
        let mut schedule = SlotSchedule::new(named(4), &utc(10, 0, 0), &end).unwrap();
        // Laptop asleep until 10:35: slot 2 (10:30-10:45) is the current one.
        let now = utc(10, 35, 0);

        let skipped = schedule.skip(&now);

        assert!(skipped);
        assert_eq!(
            ends(&schedule),
            vec![utc(10, 15, 0), utc(10, 30, 0), now, end]
        );
        let view = schedule.current(&now).unwrap();
        assert_eq!(view.index, 3);
        assert_eq!(view.remaining_ms, 1_500_000);
    }

    #[test]
    fn skip_at_fractional_instant_keeps_ends_on_whole_seconds() {
        let end = utc(11, 0, 0);
        let mut schedule = SlotSchedule::new(named(4), &utc(10, 0, 0), &end).unwrap();
        let now = utc(10, 5, 0) + TimeDelta::milliseconds(250);

        let skipped = schedule.skip(&now);

        assert!(skipped);
        let all = ends(&schedule);
        assert_eq!(all.first(), Some(&now));
        assert_eq!(all.last(), Some(&end));
        for slot_end in all.iter().skip(1) {
            let distance = end.signed_duration_since(*slot_end).num_milliseconds();
            assert_eq!(distance % 1000, 0);
        }
        // 3299.75 s over 3 equal weights: 1099 s + 750 ms, 1099 s, 1101 s.
        assert_eq!(all.get(1), Some(&utc(10, 23, 20)));
        assert_eq!(all.get(2), Some(&utc(10, 41, 39)));
    }

    #[test]
    fn skip_with_less_than_one_second_left_returns_false() {
        let end = utc(11, 0, 0);
        // A trailing zero-weight slot ends at the meeting end, so the slot before it can still be
        // current when less than a second is left.
        let slots = vec![
            Slot {
                name: "a".to_string(),
                weight: 1,
                end,
            },
            Slot {
                name: "b".to_string(),
                weight: 1,
                end,
            },
        ];
        let mut schedule = SlotSchedule {
            slots,
            meeting_end: end,
        };
        let before = ends(&schedule);

        let skipped = schedule.skip(&(end - TimeDelta::milliseconds(500)));

        assert!(!skipped);
        assert_eq!(ends(&schedule), before);
    }
}
