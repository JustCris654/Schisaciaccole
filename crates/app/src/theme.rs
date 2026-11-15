//! Local visual preferences, independent of accounts and meeting state.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::theme_catalog::{THEME_CATALOG, ThemeId};

const INVALID_NOTICE: &str = "Tema salvato non disponibile. Uso il tema predefinito.";
const READ_NOTICE: &str = "Impossibile leggere le preferenze. Uso il tema predefinito.";
const WRITE_NOTICE: &str = "Scelta applicata per questa sessione, ma non salvata.";

/// No saved theme follows the build default; an explicit choice is saved by stable ID.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeChoice {
    #[default]
    BuildDefault,
    Explicit(ThemeId),
}

impl ThemeChoice {
    /// Resolves against the current build; explicit choices survive default changes.
    pub fn resolve(self, build_default: ThemeId) -> ThemeId {
        match self {
            Self::BuildDefault => build_default,
            Self::Explicit(theme) => theme,
        }
    }

    /// Returns the catalog index of the resolved active theme.
    pub fn catalog_index(self, build_default: ThemeId) -> i32 {
        let theme = self.resolve(build_default);
        THEME_CATALOG
            .iter()
            .position(|entry| entry.theme == theme)
            .and_then(|position| i32::try_from(position).ok())
            .unwrap_or(0)
    }

    /// Maps a catalog index to an explicit choice, rejecting indices outside the catalog.
    pub fn from_catalog_index(index: i32) -> Option<Self> {
        let position = usize::try_from(index).ok()?;
        THEME_CATALOG
            .get(position)
            .map(|entry| Self::Explicit(entry.theme))
    }
}

/// Appearance mode; system observation is kept live by Slint's native palette.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    /// Returns the settings selection, independent of the selected theme.
    pub fn index(self) -> i32 {
        match self {
            Self::System => 0,
            Self::Light => 1,
            Self::Dark => 2,
        }
    }

    /// Maps settings selections, rejecting other indices.
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::System),
            1 => Some(Self::Light),
            2 => Some(Self::Dark),
            _ => None,
        }
    }
}

/// Preferences and a recoverable inline notice for the current application session.
#[derive(Debug, Default)]
pub struct VisualPreferences {
    pub theme: ThemeChoice,
    pub mode: AppearanceMode,
    pub notice: String,
    document: toml::Table,
}

impl VisualPreferences {
    /// Loads TOML; absent files are normal and other errors use defaults with a notice.
    /// Unknown themes and modes also fall back without preventing startup.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                log::warn!("Could not read visual preferences: {error}");
                Self {
                    notice: READ_NOTICE.into(),
                    ..Self::default()
                }
            }
        }
    }

    /// Parses local preferences; invalid values use build-default/system defaults.
    /// A valid mode is retained when only the theme is unavailable, and vice versa.
    fn parse(text: &str) -> Self {
        let document = match text.parse::<toml::Table>() {
            Ok(document) => document,
            Err(_) => {
                return Self {
                    notice: INVALID_NOTICE.into(),
                    ..Self::default()
                };
            }
        };
        let mut preferences = Self {
            document,
            ..Self::default()
        };
        if let Some(value) = preferences.document.get("theme") {
            preferences.theme = match value.as_str() {
                // Written by early test builds; equivalent to an absent key.
                Some("distribution") => ThemeChoice::BuildDefault,
                Some(id) => match ThemeId::parse(id) {
                    Some(theme) => ThemeChoice::Explicit(theme),
                    None => {
                        preferences.notice = INVALID_NOTICE.into();
                        ThemeChoice::BuildDefault
                    }
                },
                None => {
                    preferences.notice = INVALID_NOTICE.into();
                    ThemeChoice::BuildDefault
                }
            };
        }
        if let Some(value) = preferences.document.get("mode") {
            preferences.mode = match value.as_str() {
                Some("system") => AppearanceMode::System,
                Some("light") => AppearanceMode::Light,
                Some("dark") => AppearanceMode::Dark,
                _ => {
                    preferences.notice =
                        "Modalità salvata non valida. Uso la modalità Sistema.".into();
                    AppearanceMode::System
                }
            };
        }
        preferences
    }

    /// Saves an already-applied session choice atomically, preserving unrelated TOML keys.
    /// On error the choice stays active and the inline notice reports lost persistence.
    pub fn save(&mut self, path: &Path) {
        match self.theme {
            ThemeChoice::BuildDefault => {
                self.document.remove("theme");
            }
            ThemeChoice::Explicit(theme) => {
                self.document.insert("theme".into(), theme.id().into());
            }
        }
        let mode = match self.mode {
            AppearanceMode::System => "system",
            AppearanceMode::Light => "light",
            AppearanceMode::Dark => "dark",
        };
        self.document.insert("mode".into(), mode.into());
        match write_preferences(path, &self.document.to_string()) {
            Ok(()) => self.notice.clear(),
            Err(error) => {
                log::warn!("Could not save visual preferences: {error}");
                self.notice = WRITE_NOTICE.into();
            }
        }
    }

    /// Records unavailable home configuration without preventing session changes.
    pub fn unavailable_home() -> Self {
        Self {
            notice: READ_NOTICE.into(),
            ..Self::default()
        }
    }

    /// Reports persistence failure when no home directory can be resolved.
    pub fn cannot_save(&mut self) {
        self.notice = WRITE_NOTICE.into();
    }
}

/// Resolves the fixed path under the user's home; an unset or empty home is unavailable.
pub fn preferences_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".config/schisaciaccole/config.toml"))
}

/// Writes a sibling temporary file then renames it, leaving the previous file on failure.
///
/// # Errors
/// Returns I/O errors for missing parents, directory creation, writing or atomic replacement.
fn write_preferences(path: &Path, text: &str) -> io::Result<()> {
    use std::{
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("preferences path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut attempts = 0;
    let (temporary, mut file) = loop {
        let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".config.toml.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempts < 16 => {
                attempts += 1
            }
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory() -> PathBuf {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).unwrap();
        let path = std::env::temp_dir().join(format!(
            "schisaciaccole-theme-{:x}",
            u128::from_ne_bytes(bytes)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn absent_theme_follows_build_default_and_explicit_choices_do_not() {
        let distribution = VisualPreferences::parse("mode = 'dark'");
        let explicit = VisualPreferences::parse("theme = 'schisaciaccole'");
        assert_eq!(
            distribution.theme.resolve(ThemeId::OpenMove1),
            ThemeId::OpenMove1
        );
        assert_eq!(
            explicit.theme.resolve(ThemeId::OpenMove1),
            ThemeId::Schisaciaccole
        );
    }

    #[test]
    fn missing_preferences_are_build_default_and_system_without_notice() {
        let directory = test_directory();
        let preferences = VisualPreferences::load(&directory.join("absent.toml"));
        assert_eq!(preferences.theme, ThemeChoice::BuildDefault);
        assert_eq!(preferences.mode, AppearanceMode::System);
        assert!(preferences.notice.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn removed_theme_keeps_valid_mode_and_reports_fallback() {
        let preferences = VisualPreferences::parse("theme = 'removed'\nmode = 'dark'");
        assert_eq!(preferences.theme, ThemeChoice::BuildDefault);
        assert_eq!(preferences.mode, AppearanceMode::Dark);
        assert_eq!(preferences.notice, INVALID_NOTICE);
    }

    #[test]
    fn malformed_and_wrong_type_preferences_report_invalid_data() {
        for data in ["theme = [", "theme = 42", "mode = true", "mode = 'unknown'"] {
            let preferences = VisualPreferences::parse(data);
            assert!(!preferences.notice.is_empty());
            assert_eq!(preferences.mode, AppearanceMode::System);
        }
    }

    #[test]
    fn read_error_is_distinct_from_absence_and_invalid_data() {
        let directory = test_directory();
        let preferences = VisualPreferences::load(&directory);
        assert_eq!(preferences.theme, ThemeChoice::BuildDefault);
        assert_eq!(preferences.notice, READ_NOTICE);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn save_round_trips_both_choices_and_preserves_unrelated_keys() {
        let directory = test_directory();
        let path = directory.join("nested/config.toml");
        let mut preferences = VisualPreferences::parse("other = 'retained'");
        preferences.theme = ThemeChoice::Explicit(ThemeId::OpenMove1);
        preferences.mode = AppearanceMode::Light;
        preferences.save(&path);
        assert!(preferences.notice.is_empty());
        let loaded = VisualPreferences::load(&path);
        assert_eq!(loaded.theme, preferences.theme);
        assert_eq!(loaded.mode, preferences.mode);
        assert_eq!(
            loaded.document.get("other").and_then(toml::Value::as_str),
            Some("retained")
        );
        preferences.theme = ThemeChoice::BuildDefault;
        preferences.save(&path);
        let loaded = VisualPreferences::load(&path);
        assert_eq!(loaded.theme, ThemeChoice::BuildDefault);
        assert!(loaded.document.get("theme").is_none());
        assert_eq!(loaded.mode, AppearanceMode::Light);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn write_failure_retains_session_choice_and_existing_file() {
        let directory = test_directory();
        let path = directory.join("not-a-directory");
        fs::write(&path, "untouched").unwrap();
        let mut preferences = VisualPreferences {
            theme: ThemeChoice::Explicit(ThemeId::OpenMove1),
            mode: AppearanceMode::Dark,
            ..VisualPreferences::default()
        };
        preferences.save(&path.join("config.toml"));
        assert_eq!(preferences.notice, WRITE_NOTICE);
        assert_eq!(preferences.theme, ThemeChoice::Explicit(ThemeId::OpenMove1));
        assert_eq!(preferences.mode, AppearanceMode::Dark);
        assert_eq!(fs::read_to_string(path).unwrap(), "untouched");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_distribution_value_is_absent_without_notice() {
        let preferences = VisualPreferences::parse("theme = 'distribution'\nmode = 'light'");
        assert_eq!(preferences.theme, ThemeChoice::BuildDefault);
        assert_eq!(preferences.mode, AppearanceMode::Light);
        assert!(preferences.notice.is_empty());
    }

    #[test]
    fn removed_openmove_id_falls_back_with_notice() {
        let preferences = VisualPreferences::parse("theme = 'openmove'");
        assert_eq!(preferences.theme, ThemeChoice::BuildDefault);
        assert_eq!(preferences.notice, INVALID_NOTICE);
    }

    #[test]
    fn mode_only_save_writes_no_theme_key_and_drops_legacy_value() {
        let directory = test_directory();
        let path = directory.join("config.toml");
        let mut preferences = VisualPreferences::parse("theme = 'distribution'");
        preferences.mode = AppearanceMode::Dark;
        preferences.save(&path);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("theme"), "{text}");
        assert!(text.contains("dark"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn no_preference_selects_the_build_default_in_the_catalog() {
        for entry in &THEME_CATALOG {
            let index = ThemeChoice::BuildDefault.catalog_index(entry.theme);
            assert_eq!(
                THEME_CATALOG[usize::try_from(index).unwrap()].theme,
                entry.theme
            );
        }
        assert_eq!(
            ThemeChoice::Explicit(ThemeId::OpenMove1).catalog_index(ThemeId::OpenMove3),
            1
        );
    }

    #[test]
    fn settings_indices_round_trip_and_reject_unknown_values() {
        let len = i32::try_from(THEME_CATALOG.len()).unwrap();
        for index in 0..len {
            let choice = ThemeChoice::from_catalog_index(index).unwrap();
            assert_eq!(choice.catalog_index(ThemeId::default()), index);
        }
        for index in [-1, len] {
            assert!(ThemeChoice::from_catalog_index(index).is_none());
        }
        for index in 0..3 {
            assert_eq!(AppearanceMode::from_index(index).unwrap().index(), index);
        }
        for index in [-1, 3] {
            assert!(AppearanceMode::from_index(index).is_none());
        }
    }
}
