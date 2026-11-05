//! Helpers for attendee names.

/// Builds a readable name from an email address.
///
/// Takes the part before the first `@` (the whole string when there is none), splits it on `.`,
/// `_` and `-`, drops empty parts, uppercases the first character of each part and joins the
/// parts with a space. The rest of each part is left unchanged.
///
/// Edge cases: when no part is left (for example `@x` or an empty string) the original email is
/// returned. It does not validate the email and does not strip `+tag` suffixes.
pub fn display_name_from_email(email: &str) -> String {
    let local = match email.split_once('@') {
        Some((local, _)) => local,
        None => email,
    };

    let mut name = String::with_capacity(local.len());
    for part in local.split(['.', '_', '-']) {
        let mut chars = part.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        if !name.is_empty() {
            name.push(' ');
        }
        name.extend(first.to_uppercase());
        name.push_str(chars.as_str());
    }

    if name.is_empty() {
        email.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_dotted_local_part() {
        assert_eq!(
            display_name_from_email("anna.rossi@openmove.com"),
            "Anna Rossi"
        );
    }

    #[test]
    fn splits_on_underscore_and_dash() {
        assert_eq!(
            display_name_from_email("mario_de-luca@x.it"),
            "Mario De Luca"
        );
    }

    #[test]
    fn returns_email_when_local_part_is_empty() {
        assert_eq!(display_name_from_email("@x"), "@x");
    }

    #[test]
    fn keeps_existing_uppercase() {
        assert_eq!(display_name_from_email("ANNA@x"), "ANNA");
    }

    #[test]
    fn works_without_at_sign() {
        assert_eq!(display_name_from_email("anna.rossi"), "Anna Rossi");
    }

    #[test]
    fn collapses_repeated_separators() {
        assert_eq!(display_name_from_email("..anna__-rossi.@x"), "Anna Rossi");
    }

    #[test]
    fn returns_email_when_only_separators() {
        assert_eq!(display_name_from_email("._-@x"), "._-@x");
    }

    #[test]
    fn capitalizes_non_ascii_initial() {
        assert_eq!(display_name_from_email("émile.zola@x.fr"), "Émile Zola");
    }

    #[test]
    fn uses_only_text_before_first_at() {
        assert_eq!(display_name_from_email("a.b@c@d"), "A B");
    }
}
