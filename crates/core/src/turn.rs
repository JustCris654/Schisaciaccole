//! A single speaking turn inside a meeting.
//!
//! The turn ends at an absolute instant and never outlives the meeting. All instants and
//! durations are `i64` milliseconds on a clock chosen by the caller. Nothing here reads the clock.

const MINUTE_MS: i64 = 60_000;

/// Longest turn, in whole minutes, that fits in the meeting.
///
/// Computed as `max(1, floor((meeting_end_ms - now_ms) / 60_000))`.
///
/// Edge cases: a meeting that is over or has less than a minute left gives 1. Values too large
/// for `u32` are capped at `u32::MAX`.
pub fn max_turn_minutes(now_ms: i64, meeting_end_ms: i64) -> u32 {
    let minutes = ((meeting_end_ms - now_ms) / MINUTE_MS).max(1);
    u32::try_from(minutes).unwrap_or(u32::MAX)
}

/// One person speaking for a limited time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakingTurn {
    name: String,
    deadline_ms: i64,
    alerted: bool,
}

impl SpeakingTurn {
    /// Starts a turn of `minutes` at `now_ms`.
    ///
    /// The deadline is `now_ms + minutes` capped at `meeting_end_ms`. A turn of 0 minutes, or
    /// started when the meeting is over, ends at once.
    pub fn new(name: String, minutes: u32, now_ms: i64, meeting_end_ms: i64) -> Self {
        let length = i64::from(minutes).saturating_mul(MINUTE_MS);
        let deadline_ms = now_ms.saturating_add(length).min(meeting_end_ms);
        Self {
            name,
            deadline_ms,
            alerted: false,
        }
    }

    /// Name of the person speaking.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Time left in the turn.
    ///
    /// Never goes past the meeting end and is never negative.
    pub fn left_ms(&self, now_ms: i64, meeting_end_ms: i64) -> i64 {
        (self.deadline_ms.min(meeting_end_ms) - now_ms).max(0)
    }

    /// Adds or removes time from the turn.
    ///
    /// The deadline moves by `delta_ms` and is clamped to `now_ms..=meeting_end_ms`. When the
    /// meeting end is before `now_ms` the turn has no time left. Adding time above zero re-arms
    /// the alert of [`SpeakingTurn::tick`].
    pub fn adjust(&mut self, delta_ms: i64, now_ms: i64, meeting_end_ms: i64) {
        let moved = self.deadline_ms.saturating_add(delta_ms);
        self.deadline_ms = moved.min(meeting_end_ms).max(now_ms);
        if self.left_ms(now_ms, meeting_end_ms) > 0 {
            self.alerted = false;
        }
    }

    /// True when time can still be added, false when the deadline is at the meeting end or later.
    pub fn can_add(&self, meeting_end_ms: i64) -> bool {
        self.deadline_ms < meeting_end_ms
    }

    /// Checks whether the turn just ran out.
    ///
    /// Returns true exactly once when the turn reaches zero while the meeting still has time left.
    /// The alert re-arms when time is added back above zero. Returns false when the meeting is
    /// over, because the meeting alert takes over.
    pub fn tick(&mut self, now_ms: i64, meeting_end_ms: i64) -> bool {
        if self.left_ms(now_ms, meeting_end_ms) > 0 {
            self.alerted = false;
            return false;
        }
        if self.alerted || meeting_end_ms - now_ms <= 0 {
            return false;
        }
        self.alerted = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const END: i64 = 600_000;

    fn turn(minutes: u32) -> SpeakingTurn {
        SpeakingTurn::new("Anna".to_string(), minutes, 0, END)
    }

    #[test]
    fn max_turn_minutes_floors_to_whole_minutes() {
        assert_eq!(max_turn_minutes(0, 5 * 60_000 + 59_000), 5);
    }

    #[test]
    fn max_turn_minutes_is_at_least_one() {
        assert_eq!(max_turn_minutes(0, 30_000), 1);
        assert_eq!(max_turn_minutes(0, 0), 1);
    }

    #[test]
    fn max_turn_minutes_is_one_when_meeting_is_over() {
        assert_eq!(max_turn_minutes(900_000, 600_000), 1);
    }

    #[test]
    fn new_sets_deadline_from_minutes() {
        let turn = turn(2);
        assert_eq!(turn.name(), "Anna");
        assert_eq!(turn.left_ms(0, END), 120_000);
    }

    #[test]
    fn new_caps_deadline_at_meeting_end() {
        assert_eq!(turn(60).left_ms(0, END), 600_000);
    }

    #[test]
    fn new_with_zero_minutes_has_no_time() {
        assert_eq!(turn(0).left_ms(0, END), 0);
    }

    #[test]
    fn left_ms_decreases_with_time() {
        assert_eq!(turn(2).left_ms(50_000, END), 70_000);
    }

    #[test]
    fn left_ms_is_never_negative() {
        assert_eq!(turn(1).left_ms(100_000, END), 0);
    }

    #[test]
    fn left_ms_never_passes_meeting_end() {
        assert_eq!(turn(5).left_ms(0, 100_000), 100_000);
    }

    #[test]
    fn adjust_adds_time() {
        let mut turn = turn(2);
        turn.adjust(30_000, 0, END);
        assert_eq!(turn.left_ms(0, END), 150_000);
    }

    #[test]
    fn adjust_clamps_to_now() {
        let mut turn = turn(2);
        turn.adjust(-1_000_000, 10_000, END);
        assert_eq!(turn.left_ms(10_000, END), 0);
    }

    #[test]
    fn adjust_clamps_to_meeting_end() {
        let mut turn = turn(2);
        turn.adjust(10_000_000, 0, END);
        assert_eq!(turn.left_ms(0, END), 600_000);
    }

    #[test]
    fn can_add_is_true_before_meeting_end() {
        assert!(turn(2).can_add(END));
    }

    #[test]
    fn can_add_is_false_at_meeting_end() {
        assert!(!turn(60).can_add(END));
    }

    #[test]
    fn tick_is_false_while_time_is_left() {
        let mut turn = turn(1);
        assert!(!turn.tick(59_999, END));
    }

    #[test]
    fn tick_fires_once_when_turn_ends() {
        let mut turn = turn(1);
        assert!(turn.tick(60_000, END));
        assert!(!turn.tick(61_000, END));
    }

    #[test]
    fn tick_does_not_fire_when_meeting_is_over() {
        let mut turn = turn(60);
        assert!(!turn.tick(END, END));
    }

    #[test]
    fn tick_rearms_after_time_is_added() {
        let mut turn = turn(1);
        assert!(turn.tick(60_000, END));
        turn.adjust(60_000, 60_000, END);
        assert!(!turn.tick(61_000, END));
        assert!(turn.tick(120_000, END));
    }

    #[test]
    fn tick_rearms_through_tick_seeing_time_left() {
        let mut turn = turn(1);
        assert!(turn.tick(60_000, END));
        turn.deadline_ms = 120_000;
        assert!(!turn.tick(90_000, END));
        assert!(turn.tick(120_000, END));
    }
}
