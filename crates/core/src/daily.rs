//! Daily stand-up support.
//!
//! Recognises daily-like meeting titles, shuffles the speaking order deterministically and splits
//! the remaining meeting time equally among the people who still have to speak. All instants and
//! durations are `i64` milliseconds on a clock chosen by the caller. Nothing here reads the clock.

/// Shortest turn, in milliseconds, that every upcoming person keeps when time is added to the
/// current speaker.
pub const MIN_TURN_MS: i64 = 60_000;

/// Margin, in milliseconds, under which the current speaker cannot receive more time.
const ADD_MARGIN_MS: i64 = 500;

/// Words that mark a meeting title as a daily stand-up.
const DAILY_WORDS: [&str; 4] = ["daily", "standup", "stand-up", "scrum"];

/// Tells whether a meeting title looks like a daily stand-up.
///
/// True when the title contains `daily`, `standup`, `stand-up` or `scrum` as a whole word, ignoring
/// case. A word boundary is the start or end of the title or a character that is not alphanumeric.
///
/// Edge cases: an empty title is false. `dailyreport` is false because the word is glued to other
/// letters. Nothing is reported as an error.
pub fn looks_daily(title: &str) -> bool {
    let lower = title.to_lowercase();
    for word in DAILY_WORDS {
        for (start, _) in lower.match_indices(word) {
            let before = lower.get(..start).and_then(|head| head.chars().next_back());
            let after = lower
                .get(start + word.len()..)
                .and_then(|tail| tail.chars().next());
            let glued_before = before.is_some_and(char::is_alphanumeric);
            let glued_after = after.is_some_and(char::is_alphanumeric);
            if !glued_before && !glued_after {
                return true;
            }
        }
    }
    false
}

/// Advances a splitmix64 generator and returns the next value.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Shuffles `items` with a Fisher-Yates pass driven by a splitmix64 generator.
///
/// The same `seed` and the same input always give the same output. Every seed works, including 0.
///
/// Edge cases: an empty or one-element list is returned unchanged. The result is a permutation of
/// the input; nothing is dropped or duplicated.
pub fn shuffled(mut items: Vec<String>, seed: u64) -> Vec<String> {
    let mut state = seed;
    let mut i = items.len();
    while i > 1 {
        i -= 1;
        let bound = i as u64 + 1;
        let j = (splitmix64(&mut state) % bound) as usize;
        items.swap(i, j);
    }
    items
}

/// A daily stand-up where the remaining meeting time is split equally among the people who still
/// have to speak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyRound {
    order: Vec<String>,
    index: usize,
    end_ms: i64,
    deadline_ms: i64,
    paused_left_ms: Option<i64>,
    share_ms: i64,
}

impl DailyRound {
    /// Starts a round on the first person of `order`, which is expected to be already shuffled.
    ///
    /// The current speaker gets an equal share of the time between `now_ms` and `end_ms`.
    ///
    /// Edge cases: an empty `order` gives a round that is finished at once. When `end_ms` is not
    /// after `now_ms` every share is 0.
    pub fn new(order: Vec<String>, end_ms: i64, now_ms: i64) -> Self {
        let mut round = Self {
            order,
            index: 0,
            end_ms,
            deadline_ms: now_ms,
            paused_left_ms: None,
            share_ms: 0,
        };
        round.allocate(now_ms);
        round
    }

    /// Name of the person speaking now, or `None` when everybody spoke.
    pub fn current(&self) -> Option<&str> {
        self.order.get(self.index).map(String::as_str)
    }

    /// People after the current one, in speaking order. Empty when the round is finished.
    pub fn upcoming(&self) -> &[String] {
        self.order.get(self.index + 1..).unwrap_or(&[])
    }

    /// True when everybody spoke (or nobody was in the round).
    pub fn is_finished(&self) -> bool {
        self.index >= self.order.len()
    }

    /// True while the current speaker's time is frozen.
    pub fn is_paused(&self) -> bool {
        self.paused_left_ms.is_some()
    }

    /// Estimated time of each upcoming person, as of the last reconcile. 0 with nobody upcoming.
    pub fn share_ms(&self) -> i64 {
        self.share_ms
    }

    /// Time left to the current speaker.
    ///
    /// Frozen while paused, otherwise `max(0, deadline - now_ms)`. Returns 0 when finished.
    pub fn current_left_ms(&self, now_ms: i64) -> i64 {
        if self.is_finished() {
            return 0;
        }
        match self.paused_left_ms {
            Some(left) => left,
            None => (self.deadline_ms - now_ms).max(0),
        }
    }

    /// Moves to the next person, clears the pause and splits the remaining time again.
    ///
    /// Does nothing when the round is finished.
    pub fn next(&mut self, now_ms: i64) {
        if self.is_finished() {
            return;
        }
        self.index = (self.index + 1).min(self.order.len());
        self.allocate(now_ms);
    }

    /// Pauses or resumes the current speaker.
    ///
    /// Pausing freezes the speaker's left time while the meeting clock keeps running. Resuming
    /// clears the pause and splits the remaining meeting time again among the current speaker and
    /// the upcoming people. Does nothing when the round is finished.
    pub fn toggle_pause(&mut self, now_ms: i64) {
        if self.is_finished() {
            return;
        }
        if self.is_paused() {
            self.allocate(now_ms);
        } else {
            self.paused_left_ms = Some(self.current_left_ms(now_ms));
        }
    }

    /// Adds or removes time from the current speaker.
    ///
    /// The new left time is `current_left + delta_ms` clamped to `0..=max`, where `max` is the
    /// meeting time left minus [`MIN_TURN_MS`] for each upcoming person (never below 0). The value
    /// is stored as the frozen time when paused and as the deadline otherwise. Does nothing when
    /// the round is finished.
    pub fn adjust(&mut self, now_ms: i64, delta_ms: i64) {
        if self.is_finished() {
            return;
        }
        let wanted = self.current_left_ms(now_ms).saturating_add(delta_ms);
        let new_left = wanted.min(self.max_current_ms(now_ms)).max(0);
        if self.is_paused() {
            self.paused_left_ms = Some(new_left);
        } else {
            self.deadline_ms = now_ms.saturating_add(new_left);
        }
        self.reconcile(now_ms);
    }

    /// True when the current speaker can receive more time.
    ///
    /// False when the round is finished or the speaker is within 500 ms of the maximum.
    pub fn can_add(&self, now_ms: i64) -> bool {
        !self.is_finished()
            && self.current_left_ms(now_ms) < self.max_current_ms(now_ms) - ADD_MARGIN_MS
    }

    /// True when the current speaker has time to take away. False when finished or at 0.
    pub fn can_remove(&self, now_ms: i64) -> bool {
        !self.is_finished() && self.current_left_ms(now_ms) > 0
    }

    /// Updates the round for the current instant.
    ///
    /// When the round is not finished and not paused and the speaker's time is over, advances to
    /// the next person and returns true only if the meeting still has time left. In every
    /// non-finished case the estimated share is refreshed. Returns false when finished or paused.
    pub fn tick(&mut self, now_ms: i64) -> bool {
        if self.is_finished() {
            return false;
        }
        let mut advanced = false;
        if !self.is_paused() && self.current_left_ms(now_ms) == 0 {
            self.next(now_ms);
            advanced = true;
        }
        if !self.is_finished() {
            self.reconcile(now_ms);
        }
        advanced && self.end_ms - now_ms > 0
    }

    /// Largest time the current speaker can hold while keeping [`MIN_TURN_MS`] for each upcoming
    /// person.
    fn max_current_ms(&self, now_ms: i64) -> i64 {
        let reserved = (self.upcoming().len() as i64).saturating_mul(MIN_TURN_MS);
        (self.end_ms - now_ms - reserved).max(0)
    }

    /// Splits the meeting time left equally from the current speaker to the last one.
    fn allocate(&mut self, now_ms: i64) {
        let left = (self.end_ms - now_ms).max(0);
        let remaining = self.order.len().saturating_sub(self.index) as i64;
        let share = if remaining == 0 { 0 } else { left / remaining };
        self.paused_left_ms = None;
        self.deadline_ms = now_ms.saturating_add(share);
        self.reconcile(now_ms);
    }

    /// Caps the current speaker to the meeting time left and recomputes the estimated share.
    fn reconcile(&mut self, now_ms: i64) {
        let left = (self.end_ms - now_ms).max(0);
        match self.paused_left_ms {
            Some(frozen) => self.paused_left_ms = Some(frozen.min(left)),
            None => self.deadline_ms = self.deadline_ms.min(now_ms.saturating_add(left)),
        }
        let upcoming = self.upcoming().len() as i64;
        self.share_ms = if upcoming == 0 {
            0
        } else {
            ((left - self.current_left_ms(now_ms)) / upcoming).max(0)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    fn round_of_four() -> DailyRound {
        // 10 minutes for four people, starting at t = 0.
        DailyRound::new(names(&["a", "b", "c", "d"]), 600_000, 0)
    }

    #[test]
    fn looks_daily_matches_keywords() {
        assert!(looks_daily("Daily PMO"));
        assert!(looks_daily("Team standup"));
        assert!(looks_daily("Scrum of scrums"));
    }

    #[test]
    fn looks_daily_matches_hyphenated_standup() {
        assert!(looks_daily("Stand-up team"));
    }

    #[test]
    fn looks_daily_ignores_case() {
        assert!(looks_daily("DAILY"));
        assert!(looks_daily("sTaNdUp"));
    }

    #[test]
    fn looks_daily_rejects_glued_word() {
        assert!(!looks_daily("dailyreport"));
        assert!(!looks_daily("scrumban"));
        assert!(!looks_daily("my3daily"));
    }

    #[test]
    fn looks_daily_accepts_punctuation_boundaries() {
        assert!(looks_daily("[Daily]"));
        assert!(looks_daily("team_daily-sync"));
    }

    #[test]
    fn looks_daily_rejects_empty_and_unrelated_titles() {
        assert!(!looks_daily(""));
        assert!(!looks_daily("Retrospective"));
    }

    #[test]
    fn looks_daily_finds_later_occurrence_after_glued_one() {
        assert!(looks_daily("dailyreport daily"));
    }

    #[test]
    fn shuffled_is_deterministic() {
        let input = names(&["a", "b", "c", "d", "e", "f"]);
        assert_eq!(shuffled(input.clone(), 42), shuffled(input, 42));
    }

    #[test]
    fn shuffled_keeps_every_item() {
        let input = names(&["a", "b", "c", "d", "e", "f"]);
        let mut out = shuffled(input.clone(), 7);
        out.sort();
        assert_eq!(out, input);
    }

    #[test]
    fn shuffled_works_with_seed_zero() {
        let input = names(&["a", "b", "c", "d", "e", "f"]);
        let mut out = shuffled(input.clone(), 0);
        out.sort();
        assert_eq!(out, input);
    }

    #[test]
    fn shuffled_differs_for_some_seeds() {
        let input = names(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        let first = shuffled(input.clone(), 1);
        assert!((2..20).any(|seed| shuffled(input.clone(), seed) != first));
    }

    #[test]
    fn shuffled_handles_empty_and_single() {
        assert!(shuffled(Vec::new(), 5).is_empty());
        assert_eq!(shuffled(names(&["a"]), 5), names(&["a"]));
    }

    #[test]
    fn new_round_splits_time_equally() {
        let round = round_of_four();
        assert_eq!(round.current(), Some("a"));
        assert_eq!(round.current_left_ms(0), 150_000);
        assert_eq!(round.share_ms(), 150_000);
        assert_eq!(round.upcoming(), names(&["b", "c", "d"]).as_slice());
        assert!(!round.is_paused());
        assert!(!round.is_finished());
    }

    #[test]
    fn new_round_with_empty_order_is_finished() {
        let round = DailyRound::new(Vec::new(), 600_000, 0);
        assert!(round.is_finished());
        assert_eq!(round.current(), None);
        assert!(round.upcoming().is_empty());
        assert_eq!(round.current_left_ms(0), 0);
        assert_eq!(round.share_ms(), 0);
    }

    #[test]
    fn new_round_after_meeting_end_has_no_time() {
        let round = DailyRound::new(names(&["a", "b"]), 1_000, 5_000);
        assert_eq!(round.current_left_ms(5_000), 0);
        assert_eq!(round.share_ms(), 0);
    }

    #[test]
    fn single_person_gets_all_the_time_and_no_share() {
        let round = DailyRound::new(names(&["a"]), 300_000, 0);
        assert_eq!(round.current_left_ms(0), 300_000);
        assert_eq!(round.share_ms(), 0);
        assert!(round.upcoming().is_empty());
    }

    #[test]
    fn current_left_decreases_with_time() {
        let round = round_of_four();
        assert_eq!(round.current_left_ms(50_000), 100_000);
        assert_eq!(round.current_left_ms(400_000), 0);
    }

    #[test]
    fn next_reallocates_remaining_time() {
        let mut round = round_of_four();
        round.next(30_000);
        assert_eq!(round.current(), Some("b"));
        // 570 s over three people.
        assert_eq!(round.current_left_ms(30_000), 190_000);
        assert_eq!(round.upcoming().len(), 2);
    }

    #[test]
    fn next_clears_pause() {
        let mut round = round_of_four();
        round.toggle_pause(10_000);
        round.next(20_000);
        assert!(!round.is_paused());
    }

    #[test]
    fn next_past_last_person_finishes_round() {
        let mut round = DailyRound::new(names(&["a"]), 600_000, 0);
        round.next(10_000);
        assert!(round.is_finished());
        assert_eq!(round.current(), None);
        assert!(round.upcoming().is_empty());
    }

    #[test]
    fn next_is_noop_when_finished() {
        let mut round = DailyRound::new(Vec::new(), 600_000, 0);
        round.next(10_000);
        assert!(round.is_finished());
    }

    #[test]
    fn pause_freezes_current_left() {
        let mut round = round_of_four();
        round.toggle_pause(50_000);
        assert!(round.is_paused());
        assert_eq!(round.current_left_ms(50_000), 100_000);
        assert_eq!(round.current_left_ms(90_000), 100_000);
    }

    #[test]
    fn resume_splits_remaining_time_again() {
        let mut round = round_of_four();
        round.toggle_pause(50_000);
        round.toggle_pause(110_000);
        assert!(!round.is_paused());
        // 490 s over four people.
        assert_eq!(round.current_left_ms(110_000), 122_500);
    }

    #[test]
    fn toggle_pause_is_noop_when_finished() {
        let mut round = DailyRound::new(Vec::new(), 600_000, 0);
        round.toggle_pause(0);
        assert!(!round.is_paused());
    }

    #[test]
    fn paused_speaker_is_capped_when_meeting_runs_out() {
        let mut round = round_of_four();
        round.toggle_pause(0);
        round.tick(590_000);
        assert_eq!(round.current_left_ms(590_000), 10_000);
    }

    #[test]
    fn adjust_adds_time_and_shrinks_share() {
        let mut round = round_of_four();
        round.adjust(0, 30_000);
        assert_eq!(round.current_left_ms(0), 180_000);
        assert_eq!(round.share_ms(), 140_000);
    }

    #[test]
    fn adjust_removes_time_without_going_below_zero() {
        let mut round = round_of_four();
        round.adjust(0, -1_000_000);
        assert_eq!(round.current_left_ms(0), 0);
    }

    #[test]
    fn adjust_is_capped_to_keep_minimum_for_upcoming() {
        let mut round = round_of_four();
        round.adjust(0, 10_000_000);
        // 600 s minus three minimum turns.
        assert_eq!(round.current_left_ms(0), 420_000);
    }

    #[test]
    fn adjust_while_paused_changes_frozen_value() {
        let mut round = round_of_four();
        round.toggle_pause(0);
        round.adjust(10_000, 20_000);
        assert!(round.is_paused());
        assert_eq!(round.current_left_ms(99_000), 170_000);
    }

    #[test]
    fn adjust_is_noop_when_finished() {
        let mut round = DailyRound::new(Vec::new(), 600_000, 0);
        round.adjust(0, 10_000);
        assert!(round.is_finished());
    }

    #[test]
    fn adjust_with_little_meeting_time_gives_zero() {
        let mut round = round_of_four();
        round.adjust(500_000, 50_000);
        assert_eq!(round.current_left_ms(500_000), 0);
    }

    #[test]
    fn can_add_is_true_with_room() {
        assert!(round_of_four().can_add(0));
    }

    #[test]
    fn can_add_is_false_near_the_maximum() {
        let mut round = round_of_four();
        round.adjust(0, 10_000_000);
        assert!(!round.can_add(0));
    }

    #[test]
    fn can_add_is_false_within_margin_of_maximum() {
        let mut round = round_of_four();
        round.adjust(0, 10_000_000);
        round.adjust(0, -400);
        assert!(!round.can_add(0));
        round.adjust(0, -200);
        assert!(round.can_add(0));
    }

    #[test]
    fn can_add_is_false_when_finished() {
        let round = DailyRound::new(Vec::new(), 600_000, 0);
        assert!(!round.can_add(0));
    }

    #[test]
    fn can_remove_depends_on_left_time() {
        let mut round = round_of_four();
        assert!(round.can_remove(0));
        round.adjust(0, -1_000_000);
        assert!(!round.can_remove(0));
    }

    #[test]
    fn can_remove_is_false_when_finished() {
        let round = DailyRound::new(Vec::new(), 600_000, 0);
        assert!(!round.can_remove(0));
    }

    #[test]
    fn tick_advances_when_speaker_time_is_over() {
        let mut round = round_of_four();
        assert!(round.tick(150_000));
        assert_eq!(round.current(), Some("b"));
    }

    #[test]
    fn tick_does_not_advance_before_time_is_over() {
        let mut round = round_of_four();
        assert!(!round.tick(149_999));
        assert_eq!(round.current(), Some("a"));
    }

    #[test]
    fn tick_does_not_advance_while_paused() {
        let mut round = round_of_four();
        round.adjust(0, -1_000_000);
        round.toggle_pause(0);
        assert!(!round.tick(10_000));
        assert_eq!(round.current(), Some("a"));
    }

    #[test]
    fn tick_returns_false_when_meeting_is_over() {
        let mut round = round_of_four();
        assert!(!round.tick(700_000));
        assert_eq!(round.current(), Some("b"));
    }

    #[test]
    fn tick_returns_false_when_finished() {
        let mut round = DailyRound::new(Vec::new(), 600_000, 0);
        assert!(!round.tick(10_000));
    }

    #[test]
    fn tick_on_last_speaker_finishes_round() {
        let mut round = DailyRound::new(names(&["a"]), 100_000, 0);
        round.adjust(0, -1_000_000);
        assert!(round.tick(10_000));
        assert!(round.is_finished());
    }

    #[test]
    fn tick_on_last_speaker_at_meeting_end_finishes_without_alert() {
        let mut round = DailyRound::new(names(&["a"]), 100_000, 0);
        assert!(!round.tick(100_000));
        assert!(round.is_finished());
    }

    #[test]
    fn tick_refreshes_share_after_time_passes() {
        let mut round = round_of_four();
        round.tick(100_000);
        // 500 s left, speaker has 50 s, three people upcoming.
        assert_eq!(round.share_ms(), 150_000);
    }
}
