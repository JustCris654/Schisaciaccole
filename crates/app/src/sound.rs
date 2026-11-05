//! Playback of the sound that marks the end of a ceremony.

use log::{error, info};
use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::thread;

const SOUND_ASSET_NAME: &str = "level_up_no_copyright.mp3";

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

/// Plays the finish sound once, on a background thread, and returns at once.
///
/// Every failure (asset not found, no audio device, file not readable or not
/// decodable) is logged and the sound is skipped; nothing is reported to the
/// caller. Calls made while a sound is playing overlap instead of queueing.
pub fn play_sound() {
    thread::spawn(|| {
        let Some(audio_path) = sound_path(SOUND_ASSET_NAME) else {
            error!("Sound file not found: {SOUND_ASSET_NAME} (searched bundle + crate dirs)");
            return;
        };

        let mut sink_handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink_handle) => sink_handle,
            Err(e) => {
                error!("Failed to open default audio stream: {e}");
                return;
            }
        };
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
