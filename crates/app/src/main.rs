//! Schisaciaccole: countdown timer for time-boxed Scrum ceremonies.

mod sound;
mod state;

mod ui {
    slint::include_modules!();
}

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Context;
use log::{error, info, warn};
use slint::ComponentHandle;

use crate::state::AppState;
use crate::ui::{Help, MainWindow};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    info!("Starting schisaciaccole");

    let main_window = MainWindow::new().context("failed to create the main window")?;

    let app_state = AppState::new().context("failed to compute the initial time options")?;
    app_state.bind_options(&main_window);
    app_state.render(&main_window);
    let app_state = Rc::new(RefCell::new(app_state));

    main_window.set_is_macos(std::env::consts::OS == "macos");

    main_window.on_select_time({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |index| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let must_play = state.select(index);
            state.render(&window);
            if must_play {
                sound::play_sound();
            }
        }
    });

    main_window.on_stop({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |stop_type| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.stop(stop_type);
            state.render(&window);
        }
    });

    main_window.on_compute_options({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.refresh_options();
            state.render(&window);
        }
    });

    main_window.on_tick({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let must_play = state.tick();
            state.render(&window);
            if must_play {
                sound::play_sound();
            }
        }
    });

    main_window.on_set_selection_page({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.show_selection_page();
            state.render(&window);
        }
    });

    main_window.on_quit(|| {
        if let Err(e) = slint::quit_event_loop() {
            error!("Failed to quit the event loop: {e}");
        }
    });

    main_window.on_fullscreen({
        let window_weak = main_window.as_weak();

        move |exit_fullscreen| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let window = window.window();

            let is_window_fullscreen = window.is_fullscreen();

            if exit_fullscreen {
                window.set_fullscreen(false);
            } else {
                window.set_fullscreen(!is_window_fullscreen);
            }
        }
    });

    main_window.on_help(|| {
        let help_window = match Help::new() {
            Ok(help_window) => help_window,
            Err(e) => {
                error!("Failed to create the help window: {e}");
                return;
            }
        };
        if let Err(e) = help_window.show() {
            error!("Failed to show the help window: {e}");
        }
    });

    let name = env!("CARGO_PKG_NAME");
    if let Err(e) = i_slint_core::api::set_xdg_app_id(name) {
        warn!("Failed to set the XDG app id: {e}");
    }
    main_window
        .run()
        .context("the main window event loop failed")?;

    Ok(())
}
