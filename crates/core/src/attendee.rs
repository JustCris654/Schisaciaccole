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

/// Shortens a person's name to fit in a small space.
///
/// Level 0 returns the trimmed name. Level 1 keeps the first word and turns the others into
/// initials: `Andrea Zanella Rossi` becomes `Andrea Z. R.`. Level 2 or more turns every word into
/// an initial with no spaces: `A.Z.R.`. Words are separated by whitespace and initials are the
/// first `char` of each word, so non-ASCII names are safe.
///
/// Edge cases: a single-word name is returned trimmed and unchanged at every level. An empty or
/// blank name gives an empty string.
pub fn abbreviate_name(name: &str, level: u8) -> String {
    let trimmed = name.trim();
    if level == 0 {
        return trimmed.to_string();
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() < 2 {
        return trimmed.to_string();
    }

    let mut out = String::with_capacity(trimmed.len());
    for (position, word) in words.iter().enumerate() {
        if level == 1 {
            if position == 0 {
                out.push_str(word);
                continue;
            }
            out.push(' ');
        }
        if let Some(initial) = word.chars().next() {
            out.push(initial);
            out.push('.');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviate_level_zero_returns_trimmed_name() {
        assert_eq!(abbreviate_name("  Andrea Zanella ", 0), "Andrea Zanella");
    }

    #[test]
    fn abbreviate_level_one_keeps_first_word() {
        assert_eq!(abbreviate_name("Andrea Zanella Rossi", 1), "Andrea Z. R.");
    }

    #[test]
    fn abbreviate_level_two_uses_only_initials() {
        assert_eq!(abbreviate_name("Andrea Zanella Rossi", 2), "A.Z.R.");
    }

    #[test]
    fn abbreviate_level_above_two_behaves_like_two() {
        assert_eq!(abbreviate_name("Andrea Zanella Rossi", 9), "A.Z.R.");
    }

    #[test]
    fn abbreviate_single_word_is_unchanged_at_every_level() {
        assert_eq!(abbreviate_name("Andrea", 0), "Andrea");
        assert_eq!(abbreviate_name("Andrea", 1), "Andrea");
        assert_eq!(abbreviate_name("Andrea", 2), "Andrea");
    }

    #[test]
    fn abbreviate_empty_gives_empty() {
        assert_eq!(abbreviate_name("", 0), "");
        assert_eq!(abbreviate_name("   ", 1), "");
        assert_eq!(abbreviate_name("", 2), "");
    }

    #[test]
    fn abbreviate_collapses_repeated_spaces() {
        assert_eq!(abbreviate_name("Anna   Maria", 1), "Anna M.");
    }

    #[test]
    fn abbreviate_is_unicode_safe() {
        assert_eq!(abbreviate_name("Émile Ångström", 1), "Émile Å.");
        assert_eq!(abbreviate_name("Émile Ångström", 2), "É.Å.");
    }

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
