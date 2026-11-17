//! Playback of the sound that marks the end of a ceremony.

use log::{error, info, warn};
use rodio::source::SineWave;
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const SOUND_ASSET_NAME: &str = "arcade_victory.mp3";
const BEEP_FREQUENCY_HZ: f32 = 880.0;
const BEEP_DURATION: Duration = Duration::from_millis(200);
const BEEP_AMPLITUDE: f32 = 0.2;

/// Locate the audio asset, checking every place it can live across dev and the
/// packaged bundles. cargo-bundle / the PKGBUILD copy `assets/` to a
/// platform-specific dir:
///   - linux pkg: `/usr/lib/<name>/assets/...` (exe is `/usr/bin/<name>`)
///   - macOS app: `<App>.app/Contents/Resources/assets/...` (exe is `.../MacOS/<name>`)
///     In dev, fall back to the crate dir baked in at compile time.
fn sound_path(sound_asset_name: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
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

    // dev: resolve from the crate root regardless of cwd
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(sound_asset_name),
    );

    candidates.into_iter().find(|p| p.exists())
}

/// Opens and decodes an audio file.
///
/// # Errors
/// Returns a message naming the file when it cannot be opened or decoded.
fn decode_file(path: &Path) -> Result<Decoder<BufReader<File>>, String> {
    let file = File::open(path)
        .map_err(|e| format!("Failed to open audio file {}: {e}", path.display()))?;
    Decoder::try_from(file)
        .map_err(|e| format!("Error while decoding audio file {}: {e}", path.display()))
}

/// Plays the finish sound once, on a background thread, and returns at once.
///
/// `custom` is tried first; when it cannot be opened or decoded the failure is logged and the
/// bundled sound plays instead. Every other failure (bundled asset not found or not decodable, no
/// audio device) is logged and the sound is skipped; nothing is reported to the caller. Calls made
/// while a sound is playing overlap instead of queueing.
pub fn play_sound(custom: Option<PathBuf>) {
    thread::spawn(move || {
        let mut sink_handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink_handle) => sink_handle,
            Err(e) => {
                error!("Failed to open default audio stream: {e}");
                return;
            }
        };
        sink_handle.log_on_drop(false);
        let player = Player::connect_new(sink_handle.mixer());

        if let Some(custom) = custom {
            match decode_file(&custom) {
                Ok(source) => {
                    info!("Playing sound: {}", custom.display());
                    player.append(source);
                    player.sleep_until_end();
                    return;
                }
                Err(message) => warn!("{message}; using the bundled sound"),
            }
        }

        let Some(audio_path) = sound_path(SOUND_ASSET_NAME) else {
            error!("Sound file not found: {SOUND_ASSET_NAME} (searched bundle + crate dirs)");
            return;
        };
        match decode_file(&audio_path) {
            Ok(source) => {
                info!("Playing sound: {}", audio_path.display());
                player.append(source);
                player.sleep_until_end();
            }
            Err(message) => error!("{message}"),
        }
    });
}

/// Plays a short beep that marks the end of a speaking turn, on a background thread, and returns
/// at once.
///
/// Every failure (no audio device) is logged and the beep is skipped; nothing is reported to the
/// caller. Calls made while a beep is playing overlap instead of queueing.
pub fn play_beep() {
    thread::spawn(|| {
        let mut sink_handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink_handle) => sink_handle,
            Err(e) => {
                error!("Failed to open default audio stream: {e}");
                return;
            }
        };
        sink_handle.log_on_drop(false);
        let player = Player::connect_new(sink_handle.mixer());

        let source = SineWave::new(BEEP_FREQUENCY_HZ)
            .take_duration(BEEP_DURATION)
            .amplify(BEEP_AMPLITUDE);
        player.append(source);
        player.sleep_until_end();
    });
}
