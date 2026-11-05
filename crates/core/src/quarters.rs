//! Selectable end times aligned to quarters of an hour.

use chrono::{DateTime, TimeDelta, TimeZone, Timelike};

use crate::error::TimeError;

/// Number of options the app shows by default.
pub const DEFAULT_OPTION_COUNT: usize = 8;
/// Distance between two consecutive options, in minutes.
pub const QUARTER_MINUTES: i64 = 15;
/// Seconds added to `now` before looking for the next quarter.
pub const LEAD_SECONDS: i64 = 5;

/// A selectable end time and the minutes left until it.
#[derive(Debug, Clone, PartialEq)]
pub struct QuarterOption<Tz: TimeZone> {
    /// End time, with seconds and nanoseconds set to zero.
    pub end: DateTime<Tz>,
    /// Minutes from `now` (plus the lead) to `end`; 1 to 15 for the first option.
    pub minutes_until: i64,
}

/// Builds `count` consecutive quarter-hour options after `now`.
///
/// `now` is first moved forward by [`LEAD_SECONDS`]. The first option is the next
/// quarter after that moment: if the shifted time is exactly on a quarter, the
/// first option is the following one, 15 minutes later. Each further option is
/// 15 minutes after the previous one.
///
/// Edge cases: `count == 0` returns an empty vector. Only the minute of the
/// shifted time decides the alignment; the offset of the time zone is not
/// considered, so zones with offsets that are not multiples of 15 minutes
/// align to local quarters.
///
/// # Errors
///
/// Returns [`TimeError::Overflow`] when an addition goes out of the range of
/// `DateTime`, and [`TimeError::InvalidTruncation`] when zeroing seconds and
/// nanoseconds does not give a valid time in the time zone.
pub fn quarter_options<Tz: TimeZone>(
    now: &DateTime<Tz>,
    count: usize,
) -> Result<Vec<QuarterOption<Tz>>, TimeError> {
    let lead = TimeDelta::try_seconds(LEAD_SECONDS).ok_or(TimeError::Overflow)?;
    let shifted = now
        .clone()
        .checked_add_signed(lead)
        .ok_or(TimeError::Overflow)?;

    let minutes_to_next = QUARTER_MINUTES - i64::from(shifted.minute()) % QUARTER_MINUTES;
    let first = add_minutes(&shifted, minutes_to_next)?;
    let first = first
        .with_second(0)
        .and_then(|time| time.with_nanosecond(0))
        .ok_or(TimeError::InvalidTruncation)?;

    let mut options = Vec::with_capacity(count);
    for index in 0..count {
        let steps = i64::try_from(index).map_err(|_| TimeError::Overflow)?;
        let offset = QUARTER_MINUTES
            .checked_mul(steps)
            .ok_or(TimeError::Overflow)?;
        let minutes_until = minutes_to_next
            .checked_add(offset)
            .ok_or(TimeError::Overflow)?;
        let end = add_minutes(&first, offset)?;
        options.push(QuarterOption { end, minutes_until });
    }
    Ok(options)
}

fn add_minutes<Tz: TimeZone>(time: &DateTime<Tz>, minutes: i64) -> Result<DateTime<Tz>, TimeError> {
    let delta = TimeDelta::try_minutes(minutes).ok_or(TimeError::Overflow)?;
    time.clone()
        .checked_add_signed(delta)
        .ok_or(TimeError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};

    fn utc(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 3, day)
            .unwrap()
            .and_hms_opt(hour, minute, second)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn mid_quarter_first_option_is_next_quarter() {
        let options = quarter_options(&utc(10, 9, 7, 20), 8).unwrap();

        assert_eq!(options[0].end, utc(10, 9, 15, 0));
        assert_eq!(options[0].minutes_until, 8);
    }

    #[test]
    fn mid_quarter_gives_eight_options_up_to_eleven() {
        let options = quarter_options(&utc(10, 9, 7, 20), DEFAULT_OPTION_COUNT).unwrap();

        assert_eq!(options.len(), 8);
        assert_eq!(options[7].end, utc(10, 11, 0, 0));
    }

    #[test]
    fn mid_quarter_minutes_until_step_by_fifteen() {
        let options = quarter_options(&utc(10, 9, 7, 20), 8).unwrap();

        let minutes: Vec<i64> = options.iter().map(|o| o.minutes_until).collect();
        assert_eq!(minutes, vec![8, 23, 38, 53, 68, 83, 98, 113]);
    }

    #[test]
    fn lead_pushes_past_quarter_when_within_five_seconds() {
        // 09:14:56 + 5 s = 09:15:01, so 09:15 is already gone.
        let options = quarter_options(&utc(10, 9, 14, 56), 1).unwrap();

        assert_eq!(options[0].end, utc(10, 9, 30, 0));
        assert_eq!(options[0].minutes_until, 15);
    }

    #[test]
    fn shifted_time_exactly_on_quarter_gives_next_quarter() {
        // 09:14:55 + 5 s = 09:15:00 exactly.
        let options = quarter_options(&utc(10, 9, 14, 55), 1).unwrap();

        assert_eq!(options[0].end, utc(10, 9, 30, 0));
        assert_eq!(options[0].minutes_until, 15);
    }

    #[test]
    fn just_before_lead_threshold_keeps_current_quarter() {
        // 09:14:54 + 5 s = 09:14:59, so 09:15 is still ahead.
        let options = quarter_options(&utc(10, 9, 14, 54), 1).unwrap();

        assert_eq!(options[0].end, utc(10, 9, 15, 0));
        assert_eq!(options[0].minutes_until, 1);
    }

    #[test]
    fn rolls_over_the_hour() {
        let options = quarter_options(&utc(10, 9, 52, 0), 1).unwrap();

        assert_eq!(options[0].end, utc(10, 10, 0, 0));
        assert_eq!(options[0].minutes_until, 8);
    }

    #[test]
    fn rolls_over_the_day() {
        let options = quarter_options(&utc(10, 23, 50, 0), 1).unwrap();

        assert_eq!(options[0].end, utc(11, 0, 0, 0));
    }

    #[test]
    fn seconds_and_nanoseconds_are_zero() {
        let now = utc(10, 9, 7, 20) + TimeDelta::nanoseconds(123_456_789);

        let options = quarter_options(&now, 8).unwrap();

        for option in &options {
            assert_eq!(option.end.second(), 0);
            assert_eq!(option.end.nanosecond(), 0);
        }
    }

    #[test]
    fn zero_count_gives_empty_vec() {
        let options = quarter_options(&utc(10, 9, 7, 20), 0).unwrap();

        assert!(options.is_empty());
    }

    #[test]
    fn count_one_gives_single_option() {
        let options = quarter_options(&utc(10, 9, 7, 20), 1).unwrap();

        assert_eq!(options.len(), 1);
    }

    #[test]
    fn near_max_date_returns_overflow_error() {
        let result = quarter_options(&DateTime::<Utc>::MAX_UTC, 1);

        assert_eq!(result, Err(TimeError::Overflow));
    }
}
