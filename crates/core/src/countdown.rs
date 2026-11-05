//! Countdown evaluation against an injected clock.

use chrono::{DateTime, TimeZone};

/// How long after the target, in milliseconds, a finished timer still counts as finished.
pub const FINISH_GRACE_MS: i64 = 59_000;

/// State of a countdown at a given moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// The target is still ahead by at least one millisecond.
    Running {
        /// Milliseconds left until the target.
        remaining_ms: i64,
    },
    /// The target was reached at most [`FINISH_GRACE_MS`] ago.
    Finished,
    /// The target passed more than [`FINISH_GRACE_MS`] ago.
    Expired,
}

/// Compares `target` with `now` and says what the countdown should do.
///
/// Edge cases: a remaining time of exactly 0 ms and exactly
/// `-FINISH_GRACE_MS` ms are both [`TickOutcome::Finished`]; one millisecond
/// earlier than `-FINISH_GRACE_MS` is [`TickOutcome::Expired`]. Sub-millisecond
/// parts are truncated toward zero, so a target 0.5 ms ahead is `Finished`.
pub fn evaluate<Tz: TimeZone>(target: &DateTime<Tz>, now: &DateTime<Tz>) -> TickOutcome {
    let remaining_ms = target
        .clone()
        .signed_duration_since(now.clone())
        .num_milliseconds();

    if remaining_ms >= 1 {
        TickOutcome::Running { remaining_ms }
    } else if remaining_ms >= -FINISH_GRACE_MS {
        TickOutcome::Finished
    } else {
        TickOutcome::Expired
    }
}

/// Picks the target time for the option the user selected.
///
/// If the chosen option is already in the past (negative remaining time) and
/// a following option exists, the following one is returned; otherwise the
/// chosen one. Only one step forward is taken, even if the next option is
/// also in the past.
///
/// Edge cases: an `index` out of range, or an empty `options` slice, returns
/// `None`. An option equal to `now` (0 ms remaining) is not in the past.
pub fn resolve_target<Tz: TimeZone>(
    options: &[DateTime<Tz>],
    index: usize,
    now: &DateTime<Tz>,
) -> Option<DateTime<Tz>> {
    let chosen = options.get(index)?;
    let is_past = chosen
        .clone()
        .signed_duration_since(now.clone())
        .num_milliseconds()
        < 0;

    if !is_past {
        return Some(chosen.clone());
    }
    let next = index
        .checked_add(1)
        .and_then(|next_index| options.get(next_index));
    match next {
        Some(next) => Some(next.clone()),
        None => Some(chosen.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeDelta, Utc};

    fn utc(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 3, 10)
            .unwrap()
            .and_hms_opt(hour, minute, second)
            .unwrap()
            .and_utc()
    }

    fn ms(value: i64) -> TimeDelta {
        TimeDelta::milliseconds(value)
    }

    #[test]
    fn running_one_ms_before_target() {
        let now = utc(10, 0, 0);

        let outcome = evaluate(&(now + ms(1)), &now);

        assert_eq!(outcome, TickOutcome::Running { remaining_ms: 1 });
    }

    #[test]
    fn running_reports_remaining_milliseconds() {
        let now = utc(10, 0, 0);

        let outcome = evaluate(&(now + ms(90_500)), &now);

        assert_eq!(
            outcome,
            TickOutcome::Running {
                remaining_ms: 90_500
            }
        );
    }

    #[test]
    fn finished_at_exact_target() {
        let now = utc(10, 0, 0);

        assert_eq!(evaluate(&now, &now), TickOutcome::Finished);
    }

    #[test]
    fn finished_at_end_of_grace_period() {
        let now = utc(10, 0, 0);

        let outcome = evaluate(&(now - ms(FINISH_GRACE_MS)), &now);

        assert_eq!(outcome, TickOutcome::Finished);
    }

    #[test]
    fn expired_one_ms_after_grace_period() {
        let now = utc(10, 0, 0);

        let outcome = evaluate(&(now - ms(FINISH_GRACE_MS + 1)), &now);

        assert_eq!(outcome, TickOutcome::Expired);
    }

    #[test]
    fn resolve_keeps_future_slot() {
        let options = [utc(10, 15, 0), utc(10, 30, 0)];

        let target = resolve_target(&options, 0, &utc(10, 5, 0));

        assert_eq!(target, Some(utc(10, 15, 0)));
    }

    #[test]
    fn resolve_moves_passed_slot_to_next() {
        let options = [utc(10, 15, 0), utc(10, 30, 0)];

        let target = resolve_target(&options, 0, &utc(10, 15, 1));

        assert_eq!(target, Some(utc(10, 30, 0)));
    }

    #[test]
    fn resolve_keeps_slot_equal_to_now() {
        let options = [utc(10, 15, 0), utc(10, 30, 0)];

        let target = resolve_target(&options, 0, &utc(10, 15, 0));

        assert_eq!(target, Some(utc(10, 15, 0)));
    }

    #[test]
    fn resolve_returns_passed_last_slot_itself() {
        let options = [utc(10, 15, 0), utc(10, 30, 0)];

        let target = resolve_target(&options, 1, &utc(10, 45, 0));

        assert_eq!(target, Some(utc(10, 30, 0)));
    }

    #[test]
    fn resolve_returns_none_for_index_out_of_range() {
        let options = [utc(10, 15, 0), utc(10, 30, 0)];

        assert_eq!(resolve_target(&options, 2, &utc(10, 0, 0)), None);
    }

    #[test]
    fn resolve_returns_none_for_empty_slice() {
        let options: [DateTime<Utc>; 0] = [];

        assert_eq!(resolve_target(&options, 0, &utc(10, 0, 0)), None);
    }

    #[test]
    fn resolve_returns_none_for_usize_max_index() {
        let options = [utc(10, 15, 0)];

        assert_eq!(resolve_target(&options, usize::MAX, &utc(10, 0, 0)), None);
    }
}
