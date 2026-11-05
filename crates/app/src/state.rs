//! Application state: the single source of truth that the Slint UI renders.

use std::fmt::Display;
use std::rc::Rc;

use chrono::{DateTime, Local, TimeZone};
use log::{debug, error, info, warn};
use schisaciaccole_core::{
    DEFAULT_OPTION_COUNT, QuarterOption, TickOutcome, TimeError, evaluate, quarter_options,
    resolve_target,
};
use slint::{ModelRc, VecModel};

use crate::ui::{MainWindow, Page, StopType, TimeOption};

/// Everything the main window shows, owned by Rust and pushed to Slint by [`AppState::render`].
pub struct AppState {
    page: Page,
    options: Vec<DateTime<Local>>,
    target: Option<DateTime<Local>>,
    timer_ms: i64,
    options_model: Rc<VecModel<TimeOption>>,
}

impl AppState {
    /// Creates the state on the selection page, with options computed from the current time.
    ///
    /// # Errors
    ///
    /// Returns the [`TimeError`] of [`quarter_options`] when the options cannot
    /// be computed (time arithmetic out of range).
    pub fn new() -> Result<Self, TimeError> {
        let (options, rows) = split_options(quarter_options(&Local::now(), DEFAULT_OPTION_COUNT)?);

        Ok(Self {
            page: Page::SelectionPage,
            options,
            target: None,
            timer_ms: 0,
            options_model: Rc::new(VecModel::from(rows)),
        })
    }

    /// Recomputes the selectable end times from the current time.
    ///
    /// On error the error is logged and the previous options are kept.
    pub fn refresh_options(&mut self) {
        let now = Local::now();
        debug!("refresh_options at {}", now.format("%H:%M:%S"));

        match quarter_options(&now, DEFAULT_OPTION_COUNT) {
            Ok(options) => {
                let (options, rows) = split_options(options);
                debug!("refresh_options refreshed {} options", rows.len());
                self.options = options;
                self.options_model.set_vec(rows);
            }
            Err(err) => error!("refresh_options failed, keeping previous options: {err}"),
        }
    }

    /// Starts the countdown to the option at `index` and moves to the timer page.
    ///
    /// The target is chosen by [`resolve_target`], so an option already in the
    /// past moves to the next one. Returns `true` when the finish sound must be
    /// played, which only happens if the target is reached at once (see
    /// [`AppState::tick`]).
    ///
    /// Edge cases: a negative `index` or one out of range is logged and
    /// ignored; the state does not change and `false` is returned.
    #[must_use]
    pub fn select(&mut self, index: i32) -> bool {
        let Ok(position) = usize::try_from(index) else {
            warn!("select: ignoring negative option index {index}");
            return false;
        };

        let now = Local::now();
        let Some(target) = resolve_target(&self.options, position, &now) else {
            warn!("select: ignoring out of range option index {index}");
            return false;
        };

        info!(
            "select: target={} remaining={}s",
            target.format("%H:%M"),
            target.signed_duration_since(now).num_seconds()
        );

        self.page = Page::TimerPage;
        self.target = Some(target);
        self.tick()
    }

    /// Updates the countdown against the current time.
    ///
    /// While the target is ahead, stores the remaining milliseconds. When it
    /// has just been reached, stops with [`StopType::TimerFinished`] and returns
    /// `true`: the caller must play the finish sound. When it passed more than
    /// the grace period ago (e.g. after a suspend), stops with
    /// [`StopType::UserStopped`] without sound.
    ///
    /// Edge cases: without a running countdown nothing changes and `false` is returned.
    #[must_use]
    pub fn tick(&mut self) -> bool {
        let Some(target) = self.target else {
            return false;
        };

        match evaluate(&target, &Local::now()) {
            TickOutcome::Running { remaining_ms } => {
                self.timer_ms = remaining_ms;
                debug!("Remaining: {remaining_ms}");
                false
            }
            TickOutcome::Finished => {
                self.stop(StopType::TimerFinished);
                true
            }
            TickOutcome::Expired => {
                self.stop(StopType::UserStopped);
                false
            }
        }
    }

    /// Ends the countdown: refreshes the options, clears the target and the timer.
    ///
    /// Moves to the finished page for [`StopType::TimerFinished`] and to the
    /// selection page for [`StopType::UserStopped`]. Calling it without a
    /// running countdown still refreshes the options and changes the page.
    pub fn stop(&mut self, stop_type: StopType) {
        info!("stop: timer stopped, back to selection");

        self.refresh_options();
        self.target = None;
        self.timer_ms = 0;
        self.page = match stop_type {
            StopType::TimerFinished => Page::TimerFinished,
            StopType::UserStopped => Page::SelectionPage,
        };
    }

    /// Moves to the selection page without touching the countdown or the options.
    pub fn show_selection_page(&mut self) {
        self.page = Page::SelectionPage;
    }

    /// Binds the shared options model to `window`; call once after creating the window.
    ///
    /// Later option refreshes update the same model, so the UI follows without rebinding.
    pub fn bind_options(&self, window: &MainWindow) {
        window.set_options(ModelRc::from(self.options_model.clone()));
    }

    /// Pushes page, running state and timer to `window`.
    pub fn render(&self, window: &MainWindow) {
        window.set_page(self.page);
        window.set_running_state(self.target.is_some());
        window.set_timer_time(self.timer_ms);
    }
}

/// Splits quarter options into their end times and the rows the UI shows.
///
/// Each row has the end time as `HH:MM` and the minutes left as a decimal
/// number. Order is preserved; an empty input gives two empty vectors.
fn split_options<Tz: TimeZone>(
    options: Vec<QuarterOption<Tz>>,
) -> (Vec<DateTime<Tz>>, Vec<TimeOption>)
where
    Tz::Offset: Display,
{
    let mut ends = Vec::with_capacity(options.len());
    let mut rows = Vec::with_capacity(options.len());

    for option in options {
        rows.push(TimeOption {
            time: option.end.format("%H:%M").to_string().into(),
            minutes: option.minutes_until.to_string().into(),
        });
        ends.push(option.end);
    }

    (ends, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};

    fn utc(hour: u32, minute: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 3, 10)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn split_formats_time_and_minutes() {
        let options = vec![QuarterOption {
            end: utc(9, 5),
            minutes_until: 8,
        }];

        let (_, rows) = split_options(options);

        assert_eq!(rows[0].time, "09:05");
        assert_eq!(rows[0].minutes, "8");
    }

    #[test]
    fn split_keeps_end_times_in_order() {
        let options = vec![
            QuarterOption {
                end: utc(9, 15),
                minutes_until: 8,
            },
            QuarterOption {
                end: utc(9, 30),
                minutes_until: 23,
            },
        ];

        let (ends, rows) = split_options(options);

        assert_eq!(ends, vec![utc(9, 15), utc(9, 30)]);
        assert_eq!(rows[1].time, "09:30");
        assert_eq!(rows[1].minutes, "23");
    }

    #[test]
    fn split_empty_gives_empty_vectors() {
        let (ends, rows) = split_options::<Utc>(Vec::new());

        assert!(ends.is_empty());
        assert!(rows.is_empty());
    }
}
