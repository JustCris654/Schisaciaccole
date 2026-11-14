//! Stable curated theme identities shared by the desktop and its build script.

/// A theme whose light and dark palettes ship with the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeId {
    #[default]
    Schisaciaccole,
    OpenMove,
}

/// Stable IDs accepted by both the build and saved preferences.
pub const THEME_IDS: [&str; 2] = ["schisaciaccole", "openmove"];

impl ThemeId {
    /// Finds a stable ID; whitespace, empty strings and unknown IDs are rejected.
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "schisaciaccole" => Some(Self::Schisaciaccole),
            "openmove" => Some(Self::OpenMove),
            _ => None,
        }
    }

    /// Returns the stable ID, independent of the visible label or build default.
    pub fn id(self) -> &'static str {
        match self {
            Self::Schisaciaccole => "schisaciaccole",
            Self::OpenMove => "openmove",
        }
    }
}

/// Resolves the compile-time default; an absent value selects the original theme.
///
/// # Errors
/// Returns a descriptive message for empty or unknown IDs. Values are not trimmed.
pub fn validate_default(value: Option<&str>) -> Result<ThemeId, String> {
    match value {
        None => Ok(ThemeId::Schisaciaccole),
        Some(value) => ThemeId::parse(value).ok_or_else(|| {
            format!(
                "Invalid SCHISACIACCOLE_DEFAULT_THEME {value:?}; allowed IDs: {} (unset selects schisaciaccole)",
                THEME_IDS.join(", ")
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_default_preserves_original_theme() {
        assert_eq!(validate_default(None), Ok(ThemeId::Schisaciaccole));
    }

    #[test]
    fn every_catalog_id_round_trips_and_is_a_valid_build_default() {
        for id in THEME_IDS {
            let theme = validate_default(Some(id)).unwrap();
            assert_eq!(theme.id(), id);
        }
    }

    #[test]
    fn invalid_build_defaults_list_allowed_ids() {
        for invalid in ["", " ", "unknown", "OpenMove", "openmove "] {
            let error = validate_default(Some(invalid)).unwrap_err();
            assert!(error.contains("SCHISACIACCOLE_DEFAULT_THEME"));
            for id in THEME_IDS {
                assert!(error.contains(id));
            }
        }
    }
}
