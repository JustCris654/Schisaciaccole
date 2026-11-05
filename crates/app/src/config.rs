//! Build-time configuration of the app.

/// Backend base URL baked in at compile time from `SCHISACIACCOLE_BACKEND_URL`.
///
/// Returns `None` when the variable was unset at build time or is blank, so the app can run
/// without the meetings feature. See [`normalize_backend_url`] for how the value is cleaned.
pub fn backend_url() -> Option<String> {
    normalize_backend_url(option_env!("SCHISACIACCOLE_BACKEND_URL")?)
}

/// Trims whitespace and trailing slashes from a backend URL.
///
/// Returns `None` when nothing is left. The URL is not validated: a value that is not a
/// well-formed URL is returned unchanged and fails later, when the first request is made.
pub fn normalize_backend_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_clean_url() {
        let url = normalize_backend_url("https://api.example.com");
        assert_eq!(url.as_deref(), Some("https://api.example.com"));
    }

    #[test]
    fn trims_whitespace() {
        let url = normalize_backend_url("  https://api.example.com \n");
        assert_eq!(url.as_deref(), Some("https://api.example.com"));
    }

    #[test]
    fn removes_trailing_slashes() {
        let url = normalize_backend_url("https://api.example.com//");
        assert_eq!(url.as_deref(), Some("https://api.example.com"));
    }

    #[test]
    fn keeps_a_path_prefix() {
        let url = normalize_backend_url("https://example.com/backend/");
        assert_eq!(url.as_deref(), Some("https://example.com/backend"));
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(normalize_backend_url(""), None);
    }

    #[test]
    fn blank_is_none() {
        assert_eq!(normalize_backend_url("  \t"), None);
    }

    #[test]
    fn only_slashes_is_none() {
        assert_eq!(normalize_backend_url(" / "), None);
    }
}
