//! Schisaciaccole: countdown timer for time-boxed Scrum ceremonies.

mod appearance;
mod backend;
mod config;
mod editor;
mod login;
mod pkce;
mod session;
mod session_store;
mod sound;
mod state;

mod ui {
    slint::include_modules!();
}

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Context;
use log::{error, info, warn};
use slint::{ComponentHandle, Weak};

use crate::backend::BackendError;
use crate::session::Session;
use crate::state::{AppState, LOCAL_MODE_NOTICE, TickSound};
use crate::ui::{Help, MainWindow};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    info!("Starting schisaciaccole");

    let main_window = MainWindow::new().context("failed to create the main window")?;
    let window_weak = main_window.as_weak();
    let _reduced_motion_observer = appearance::observe_reduced_motion(move |reduce_motion| {
        let _ = window_weak.upgrade_in_event_loop(move |window| {
            window.set_reduce_motion(reduce_motion);
        });
    });

    let mut app_state = AppState::new().context("failed to compute the initial time options")?;
    app_state.bind_options(&main_window);
    app_state.bind_meetings(&main_window);
    app_state.bind_turns(&main_window);
    app_state.bind_editor_rows(&main_window);
    let session = connect_session(&mut app_state);
    app_state.render(&main_window);
    let app_state = Rc::new(RefCell::new(app_state));

    if let Some(session) = &session {
        session::set_event_handler({
            let window_weak = main_window.as_weak();
            let state = app_state.clone();
            let session = session.clone();

            move |event| {
                let Some(window) = window_weak.upgrade() else {
                    return;
                };
                let mut state = state.borrow_mut();
                session::apply_event(&mut state, &session, event);
                state.render(&window);
            }
        });
        session.spawn_startup_check();
    }

    main_window.set_is_macos(std::env::consts::OS == "macos");

    main_window.on_select_slot({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |index| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.select_slot(index);
            state.render(&window);
        }
    });

    main_window.on_start_selected({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let tick_sound = state.start_selected();
            state.render(&window);
            play_for(tick_sound);
        }
    });

    main_window.on_select_meeting({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |index| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let tick_sound = state.select_meeting(index);
            state.render(&window);
            play_for(tick_sound);
        }
    });

    main_window.on_show_turns({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.show_turns();
            state.render(&window);
        }
    });

    main_window.on_hide_turns({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.hide_turns();
            state.render(&window);
        }
    });

    main_window.on_select_turner({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |value| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.select_turner(value);
            state.render(&window);
        }
    });

    main_window.on_adjust_turn_minutes({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |value| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.adjust_turn_minutes(value);
            state.render(&window);
        }
    });

    main_window.on_start_turn({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.start_turn();
            state.render(&window);
        }
    });

    main_window.on_adjust_turn_time({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |value| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.adjust_turn_time(value);
            state.render(&window);
        }
    });

    main_window.on_end_turn({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.end_turn();
            state.render(&window);
        }
    });

    main_window.on_set_daily({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |index, on| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.set_daily(index, on);
            state.render(&window);
        }
    });

    main_window.on_toggle_daily_pause({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.toggle_daily_pause();
            state.render(&window);
        }
    });

    main_window.on_next_daily_speaker({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.next_daily_speaker();
            state.render(&window);
        }
    });

    main_window.on_adjust_daily_time({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |value| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.adjust_daily_time(value);
            state.render(&window);
        }
    });

    main_window.on_open_editor({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        let session = session.clone();

        move |index| {
            update_state(&window_weak, &state, |state| {
                let Some(event_id) = state.open_editor(index) else {
                    return;
                };
                match &session {
                    Some(session) => session.spawn_plan_fetch(&event_id),
                    None => state.on_editor_plan_loaded(&event_id, Err(BackendError::Unreachable)),
                }
            });
        }
    });

    main_window.on_editor_set_label({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move |index, label| {
            update_state(&window_weak, &state, |state| {
                state.editor_set_label(index, &label);
            });
        }
    });

    main_window.on_editor_set_minutes({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move |index, minutes| {
            update_state(&window_weak, &state, |state| {
                state.editor_set_minutes(index, minutes);
            });
        }
    });

    main_window.on_editor_set_included({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move |index, included| {
            update_state(&window_weak, &state, |state| {
                state.editor_set_included(index, included);
            });
        }
    });

    main_window.on_editor_move_up({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move |index| update_state(&window_weak, &state, |state| state.editor_move_up(index))
    });

    main_window.on_editor_move_down({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move |index| update_state(&window_weak, &state, |state| state.editor_move_down(index))
    });

    main_window.on_editor_remove_row({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move |index| update_state(&window_weak, &state, |state| state.editor_remove_row(index))
    });

    main_window.on_editor_add_row({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move || update_state(&window_weak, &state, AppState::editor_add_row)
    });

    main_window.on_editor_reset({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move || update_state(&window_weak, &state, AppState::editor_reset)
    });

    main_window.on_editor_save({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        let session = session.clone();

        move || {
            let Some(session) = &session else {
                return;
            };
            update_state(&window_weak, &state, |state| {
                if let Some(save) = state.start_template_save() {
                    session.spawn_template_save(save);
                }
            });
        }
    });

    main_window.on_editor_delete({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        let session = session.clone();

        move || {
            let Some(session) = &session else {
                return;
            };
            update_state(&window_weak, &state, |state| {
                if let Some(delete) = state.start_template_delete() {
                    session.spawn_template_delete(delete);
                }
            });
        }
    });

    main_window.on_editor_start({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let mut sound = TickSound::None;
            update_state(&window_weak, &state, |state| {
                sound = state.start_from_editor();
            });
            play_for(sound);
        }
    });

    main_window.on_editor_back({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move || update_state(&window_weak, &state, AppState::close_editor)
    });

    main_window.on_next_slot({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        move || update_state(&window_weak, &state, AppState::next_slot)
    });

    main_window.on_refresh_meetings({
        let session = session.clone();

        move || {
            if let Some(session) = &session {
                session.spawn_meetings_fetch();
            }
        }
    });

    main_window.on_set_meetings_page({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.show_meetings_page();
            state.render(&window);
        }
    });

    main_window.on_close_finished({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.show_home();
            state.render(&window);
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
            let tick_sound = state.tick();
            state.render(&window);
            play_for(tick_sound);
        }
    });

    main_window.on_set_manual_page({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.show_manual_page();
            state.render(&window);
        }
    });

    main_window.on_login({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        let session = session.clone();

        move || {
            let (Some(window), Some(session)) = (window_weak.upgrade(), &session) else {
                return;
            };
            let mut state = state.borrow_mut();
            match session.start_login() {
                Ok(true) => state.on_login_started(),
                Ok(false) => {}
                Err(e) => {
                    error!("Cannot start the login: {e}");
                    state.on_login_failed(login::FAILED_MESSAGE);
                }
            }
            state.render(&window);
        }
    });

    main_window.on_cancel_login({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        let session = session.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            if let Some(session) = &session {
                session.cancel_login();
            }
            let mut state = state.borrow_mut();
            state.on_login_cancelled();
            state.render(&window);
        }
    });

    main_window.on_skip_login({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.on_local_mode("");
            state.render(&window);
        }
    });

    main_window.on_logout({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();
        let session = session.clone();

        move || {
            let (Some(window), Some(session)) = (window_weak.upgrade(), &session) else {
                return;
            };
            session.spawn_logout();
            let mut state = state.borrow_mut();
            state.on_logout_started();
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
    let result = main_window.run();
    session::clear_event_handler();
    result.context("the main window event loop failed")?;

    Ok(())
}

/// Plays the sound that goes with a tick outcome: the finish sound for the end of the meeting,
/// nothing otherwise.
fn play_for(tick_sound: TickSound) {
    match tick_sound {
        TickSound::None => {}
        TickSound::MeetingEnd => sound::play_sound(),
        TickSound::TurnEnd => sound::play_beep(),
    }
}

/// Creates the backend session when the app was built with a backend URL.
///
/// Without a URL nothing is touched (no keychain, no network) and `None` is returned. With one,
/// `state` moves to the loading page; when the client cannot be created it stays local with a
/// notice and `None` is returned.
fn connect_session(state: &mut AppState) -> Option<Rc<Session>> {
    let url = config::backend_url()?;
    match Session::connect(&url) {
        Ok(session) => {
            info!("Backend session enabled");
            state.show_loading();
            Some(Rc::new(session))
        }
        Err(e) => {
            warn!("Backend unavailable, running in local mode: {e}");
            state.on_local_mode(LOCAL_MODE_NOTICE);
            None
        }
    }
}

/// Runs `action` on the state and renders the result, when the window still exists.
fn update_state(
    window_weak: &Weak<MainWindow>,
    state: &RefCell<AppState>,
    action: impl FnOnce(&mut AppState),
) {
    let Some(window) = window_weak.upgrade() else {
        return;
    };
    let mut state = state.borrow_mut();
    action(&mut state);
    state.render(&window);
}
