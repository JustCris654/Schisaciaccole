//! Schisaciaccole: countdown timer for time-boxed Scrum ceremonies.

mod backend;
mod config;
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
use slint::ComponentHandle;

use crate::session::Session;
use crate::state::{AppState, LOCAL_MODE_NOTICE};
use crate::ui::{Help, MainWindow};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    info!("Starting schisaciaccole");

    let main_window = MainWindow::new().context("failed to create the main window")?;

    let mut app_state = AppState::new().context("failed to compute the initial time options")?;
    app_state.bind_options(&main_window);
    app_state.bind_meetings(&main_window);
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

    main_window.on_select_meeting({
        let window_weak = main_window.as_weak();
        let state = app_state.clone();

        move |index| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let must_play = state.select_meeting(index);
            state.render(&window);
            if must_play {
                sound::play_sound();
            }
        }
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
