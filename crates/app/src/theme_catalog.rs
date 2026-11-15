//! Stable curated theme identities shared by the desktop and its build script.

/// A theme whose light and dark palettes ship with the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeId {
    #[default]
    Schisaciaccole,
    OpenMove1,
    OpenMove2,
    OpenMove3,
}

/// One selectable theme: stable ID, settings label and optional short detail.
#[derive(Clone, Copy, Debug)]
pub struct ThemeEntry {
    pub theme: ThemeId,
    /// Stable ID accepted by the build and saved preferences.
    pub id: &'static str,
    /// Name shown in settings.
    pub name: &'static str,
    /// Optional short detail shown under the name; empty for none.
    pub detail: &'static str,
}

/// Ordered catalog of every theme; settings order follows this list.
/// Adding a theme means one entry here plus its palettes in `palettes.rs`.
pub const THEME_CATALOG: [ThemeEntry; 4] = [
    ThemeEntry {
        theme: ThemeId::Schisaciaccole,
        id: "schisaciaccole",
        name: "Schisaciaccole",
        detail: "",
    },
    ThemeEntry {
        theme: ThemeId::OpenMove1,
        id: "openmove-1",
        name: "Bubblegum",
        detail: "Chiaro: Bubblegum · Scuro: Grafite e blu",
    },
    ThemeEntry {
        theme: ThemeId::OpenMove2,
        id: "openmove-2",
        name: "Lavanda elettrica",
        detail: "Chiaro: Bianco e blu · Scuro: Nero e lime",
    },
    ThemeEntry {
        theme: ThemeId::OpenMove3,
        id: "openmove-3",
        name: "Campo Blu",
        detail: "Chiaro: Lavanda elettrica · Scuro: Campo blu",
    },
];

/// Stable IDs accepted by both the build and saved preferences, in catalog order.
pub fn theme_ids() -> impl Iterator<Item = &'static str> {
    THEME_CATALOG.iter().map(|entry| entry.id)
}

impl ThemeId {
    /// Finds a stable ID; whitespace, empty strings and unknown IDs are rejected.
    pub fn parse(id: &str) -> Option<Self> {
        THEME_CATALOG
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.theme)
    }

    /// Returns the catalog entry; a theme missing from the catalog falls back to the first entry.
    pub fn entry(self) -> &'static ThemeEntry {
        let [first, ..] = &THEME_CATALOG;
        THEME_CATALOG
            .iter()
            .find(|entry| entry.theme == self)
            .unwrap_or(first)
    }

    /// Returns the stable ID, independent of the visible label or build default.
    pub fn id(self) -> &'static str {
        self.entry().id
    }

    /// Returns the name shown in settings.
    pub fn name(self) -> &'static str {
        self.entry().name
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
                theme_ids().collect::<Vec<_>>().join(", ")
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
        for id in theme_ids() {
            let theme = validate_default(Some(id)).unwrap();
            assert_eq!(theme.id(), id);
        }
    }

    #[test]
    fn catalog_ids_and_themes_are_unique() {
        for (position, entry) in THEME_CATALOG.iter().enumerate() {
            assert!(!entry.id.is_empty() && !entry.name.is_empty());
            for other in THEME_CATALOG.iter().skip(position + 1) {
                assert_ne!(entry.id, other.id);
                assert_ne!(entry.theme, other.theme);
            }
        }
    }

    #[test]
    fn invalid_build_defaults_list_allowed_ids() {
        for invalid in ["", " ", "unknown", "OpenMove", "openmove", "openmove "] {
            let error = validate_default(Some(invalid)).unwrap_err();
            assert!(error.contains("SCHISACIACCOLE_DEFAULT_THEME"));
            for id in theme_ids() {
                assert!(error.contains(id));
            }
        }
    }
}
