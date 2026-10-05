use chrono::{prelude::*, Duration};
use log::{debug, error, info};
use rodio::{Decoder, DeviceSinkBuilder, Player};
use slint::{ModelRc, SharedString, VecModel};
use std::fs::File;
use std::path::PathBuf;
use std::{cell::RefCell, io::BufReader, rc::Rc, thread};

slint::include_modules!();

struct AppState {
    opts_time: Vec<DateTime<Local>>,
    selected: Option<DateTime<Local>>,
    options: Rc<VecModel<TimeOption>>,
}

fn update_ui_options(state: &mut AppState) {
    let (new_times, new_options) = compute_options();

    debug!("compute_options refreshed {} options", new_options.len());
    state.opts_time = new_times;
    state.options.set_vec(new_options);
}

fn compute_options() -> (Vec<DateTime<Local>>, Vec<TimeOption>) {
    let now: DateTime<Local> = Local::now() + Duration::seconds(5);
    debug!("compute_options at {}", now.format("%H:%M:%S"));

    let minutes_to_next_quarter = 15_i64 - (now.minute() % 15) as i64;

    let first_target = (now + Duration::minutes(minutes_to_next_quarter))
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();

    (0..8)
        .map(|i| {
            let time = first_target + Duration::minutes(i * 15);

            let text: SharedString = time.format("%H:%M").to_string().into();

            let option = TimeOption {
                time: text,
                minutes: (minutes_to_next_quarter + 15 * i).to_string().into(),
            };

            (time, option)
        })
        .unzip()
}

/// Locate the audio asset, checking every place it can live across dev and the
/// packaged bundles. cargo-bundle / the PKGBUILD copy `assets/` to a
/// platform-specific dir:
///   - linux pkg: `/usr/lib/<name>/assets/...` (exe is `/usr/bin/<name>`)
///   - macOS app: `<App>.app/Contents/Resources/assets/...` (exe is `.../MacOS/<name>`)
///     In dev, fall back to the crate dir baked in at compile time.
fn sound_path(sound_asset_name: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // next to the executable
            candidates.push(dir.join("assets").join(sound_asset_name));
            // linux pkg: /usr/bin/<name> -> /usr/lib/<name>/assets/<sound_asset_name>
            candidates.push(
                dir.join("../lib/schisaciaccole/assets")
                    .join(sound_asset_name),
            );
            // macOS .app: Contents/MacOS/<name> -> Contents/Resources/assets/<sound_asset_name>
            candidates.push(dir.join("../Resources/assets").join(sound_asset_name));
        }
    }

    // dev: resolve from the crate root regardless of cwd
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(sound_asset_name),
    );

    candidates.into_iter().find(|p| p.exists())
}

fn stop(state: &mut AppState, window: &MainWindow, stop_type: StopType) {
    info!("stop: timer stopped, back to selection");

    update_ui_options(state);

    window.set_running_state(false);
    window.set_timer_time(0);
    state.selected = None;
    match stop_type {
        StopType::TimerFinished => window.set_page(Page::TimerFinished),
        StopType::UserStopped => window.set_page(Page::SelectionPage),
    }

}

fn tick(state: &mut AppState, window: &MainWindow) {
    let Some(target) = state.selected else {
        return;
    };

    let now = Local::now();
    let remaining = target - now;
    let remaining = remaining.num_milliseconds();

    match remaining {
        1_i64.. => {
            // update timer
            window.set_timer_time(remaining);
            debug!("Remaining: {remaining}");
        },
        -59000_i64..=0_i64 => {
            // play sound and then reset timer
            stop(state, window, StopType::TimerFinished);
            play_sound();
        },
        _ => {
            // timer is expired, notify user and return to selection page
            // this is a temporary solution, TODO: implement a new window for pop up
            stop(state, window, StopType::UserStopped);  
        }
    }
}

fn play_sound() {
    thread::spawn(|| {
        const SOUND_ASSET_NAME: &str = "game_over.mp3";
        // const SOUND_ASSET_NAME: &str = "level_up_copyrighted.mp3";
        let Some(audio_path) = sound_path(SOUND_ASSET_NAME) else {
            error!("Sound file not found: level_up_copyrighted.mp3 (searched bundle + crate dirs)");
            return;
        };

        let mut sink_handle =
            DeviceSinkBuilder::open_default_sink().expect("Open default audio stream");
        sink_handle.log_on_drop(false);
        let player = Player::connect_new(sink_handle.mixer());

        match File::open(&audio_path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                if let Ok(source) = Decoder::new(reader) {
                    info!("Playing sound: {}", audio_path.display());
                    player.append(source);
                    player.sleep_until_end();
                } else {
                    error!("Error while decoding audio file: {}", audio_path.display());
                }
            }
            Err(e) => {
                error!("Failed to open audio file {}: {}", audio_path.display(), e);
            }
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    info!("Starting schisaciaccole");

    let main_window = MainWindow::new().unwrap();

    let (opts_time, options) = compute_options();
    let options = Rc::new(VecModel::from(options));

    main_window.set_options(ModelRc::from(options.clone()));

    let app_state = Rc::new(RefCell::new(AppState { opts_time, options, selected: None }));

    let current_os = std::env::consts::OS;
    let is_macos = current_os == "macos";
    main_window.set_is_macos(is_macos);

    main_window.on_select_time({
        let window_weak = main_window.as_weak();
        let state_clone = app_state.clone();

        move |index| {
            let window = window_weak.unwrap();
            let mut state = state_clone.borrow_mut();

            let idx: usize = index.try_into().unwrap();
            let now: DateTime<Local> = Local::now();
            let mut target_time = state.opts_time[idx];
            let mut remaining_time = target_time - now;

            if remaining_time.num_milliseconds() < 0 && idx+1 < state.opts_time.len(){
                target_time = state.opts_time[idx+1];
                remaining_time = target_time - now;
            };

            state.selected = Some(target_time);

            info!(
                "select_time: target={} remaining={}s",
                target_time.format("%H:%M"),
                remaining_time
            );

            window.set_page(Page::TimerPage);
            window.set_running_state(true);

            tick(&mut state, &window);
        }
    });

    main_window.on_stop({
        let state_clone = app_state.clone();
        let window = main_window.as_weak();

        move |stop_type| {
            let mut state = state_clone.borrow_mut();
            let window = window.unwrap();

            stop(&mut state, &window, stop_type);
        }
    });

    main_window.on_compute_options({
        let state_clone = app_state.clone();

        move || {
            let mut state = state_clone.borrow_mut();

            update_ui_options(&mut state);
        }
    });

    main_window.on_tick({
        let state_clone = app_state.clone();
        let window = main_window.as_weak();

        move || {
            let mut state = state_clone.borrow_mut();
            let window = window.unwrap();

            tick(&mut state, &window);
        }
    });

    main_window.on_set_selection_page({
        let window_weak = main_window.as_weak();

        move || {
            let window = window_weak.unwrap();
            window.set_page(Page::SelectionPage)
        }
    });

    main_window.on_quit({
        || {
            slint::quit_event_loop().unwrap();
        }
    });

    main_window.on_fullscreen({
        let window_weak = main_window.as_weak();

        move |exit_fullscreen| {
            let window = window_weak.unwrap();
            let window = window.window();

            let is_window_fullscreen = window.is_fullscreen();

            if exit_fullscreen {
                window.set_fullscreen(false);
            } else {
                window.set_fullscreen(!is_window_fullscreen);
            }
        }
    });

    main_window.on_help({
        move || {
            let help_window = Help::new().unwrap();
            help_window.show().unwrap();
        }
    });

    let name = env!("CARGO_PKG_NAME");
    let _ = i_slint_core::api::set_xdg_app_id(name);
    main_window.run()
}
