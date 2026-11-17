//! Complete semantic palettes for the curated themes; colors carry no timer state.

use crate::{theme_catalog::ThemeId, ui::ThemePalette};

/// Supplies one complete palette; light/dark selection remains a live Slint binding.
pub fn palette(theme: ThemeId, dark: bool) -> ThemePalette {
    let (light, dark_palette) = palettes(theme);
    if dark { dark_palette } else { light }
}

/// Returns the (light, dark) palettes of a theme; the exhaustive match forces a
/// palette pair for every new `ThemeId` variant.
fn palettes(theme: ThemeId) -> (ThemePalette, ThemePalette) {
    match theme {
        ThemeId::Schisaciaccole => (schisaciaccole_light(), schisaciaccole_dark()),
        ThemeId::OpenMove1 => (openmove_1_light(), openmove_1_dark()),
        ThemeId::OpenMove2 => (openmove_2_light(), openmove_2_dark()),
        ThemeId::OpenMove3 => (openmove_3_light(), openmove_3_dark()),
    }
}

fn schisaciaccole_light() -> ThemePalette {
    ThemePalette {
        page: color(0xFFFFFFFF),
        surface: color(0xF1ECFAFF),
        brand: color(0x6B2FBFFF),
        brand_hover: color(0x5F25ADFF),
        brand_pressed: color(0x521D99FF),
        on_brand: color(0xFFFFFFFF),
        text: color(0x1E1A26FF),
        text_secondary: color(0x6A6378FF),
        text_brand: color(0x6B2FBFFF),
        timer_title: color(0x6B2FBFFF),
        surface_accent: color(0x6B2FBFFF),
        border_subtle: color(0xB4A2CEFF),
        list_row: color(0xF1ECFAFF),
        list_row_border: color(0xB4A2CEFF),
        turn: color(0xE07B00FF),
        turn_text: color(0xA85A00FF),
        turn_soft: color(0xFDF1E1FF),
        turn_tint: color(0xF8D7A8FF),
        on_turn: color(0x1E1A26FF),
        notice: color(0xA85A00FF),
        focus: color(0x6B2FBFFF),
        error: color(0xB42318FF),
        danger: color(0x1E1A26FF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0x6B2FBFFF),
        timer_paused: color(0x6A6378FF),
        timer_warning: color(0xA85A00FF),
        timer_expired: color(0xB42318FF),
        shadow: color(0x1E1A2629),
        selection: color(0x6B2FBFFF),
        selection_text: color(0xFFFFFFFF),
        decoration_one: color(0x6B2FBFFF),
        decoration_two: color(0xE07B00FF),
        decoration_three: color(0xF6C453FF),
        decoration_four: color(0xC9B3F0FF),
        decoration_five: color(0x1E1A26FF),
    }
}

fn schisaciaccole_dark() -> ThemePalette {
    ThemePalette {
        page: color(0x222228FF),
        surface: color(0x35353EFF),
        brand: color(0x6B2FBFFF),
        brand_hover: color(0x7839CEFF),
        brand_pressed: color(0x5A23A6FF),
        on_brand: color(0xFFFFFFFF),
        text: color(0xF3F1F7FF),
        text_secondary: color(0xC1BECAFF),
        text_brand: color(0xC7B0EEFF),
        timer_title: color(0xC682F5FF),
        surface_accent: color(0xC682F5FF),
        border_subtle: color(0x797384FF),
        list_row: color(0x35353EFF),
        list_row_border: color(0x35353EFF),
        turn: color(0xF29A2EFF),
        turn_text: color(0xF29A2EFF),
        turn_soft: color(0x33250FFF),
        turn_tint: color(0x5C3F14FF),
        on_turn: color(0x1E1A26FF),
        notice: color(0xF6C453FF),
        focus: color(0xDBC2FFFF),
        error: color(0xFFB4ABFF),
        danger: color(0x3D3350FF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0xC682F5FF),
        timer_paused: color(0xC1BECAFF),
        timer_warning: color(0xF6C453FF),
        timer_expired: color(0xFFB4ABFF),
        shadow: color(0x00000066),
        selection: color(0xC7B0EEFF),
        selection_text: color(0x222228FF),
        decoration_one: color(0x9D65D7FF),
        decoration_two: color(0xF29A2EFF),
        decoration_three: color(0xF6C453FF),
        decoration_four: color(0xC9B3F0FF),
        decoration_five: color(0xF3EFF9FF),
    }
}

fn openmove_1_light() -> ThemePalette {
    ThemePalette {
        page: color(0xFFFFFFFF),
        surface: color(0xF8ECFBFF),
        brand: color(0x4A47F7FF),
        brand_hover: color(0x3C38DDFF),
        brand_pressed: color(0x302BC3FF),
        on_brand: color(0xFFFFFFFF),
        text: color(0x1D1D1BFF),
        text_secondary: color(0x5E5468FF),
        text_brand: color(0x4A47F7FF),
        timer_title: color(0x9035ACFF),
        surface_accent: color(0x9035ACFF),
        border_subtle: color(0xA666BBFF),
        list_row: color(0xF8ECFBFF),
        list_row_border: color(0xA666BBFF),
        turn: color(0xD86F4EFF),
        turn_text: color(0xA44428FF),
        turn_soft: color(0xFDEEE8FF),
        turn_tint: color(0xF4C6B7FF),
        on_turn: color(0x1D1D1BFF),
        notice: color(0x8A5500FF),
        focus: color(0x4A47F7FF),
        error: color(0xB42318FF),
        danger: color(0x4A47F7FF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0x4A47F7FF),
        timer_paused: color(0x5E5468FF),
        timer_warning: color(0x8A5500FF),
        timer_expired: color(0xB42318FF),
        shadow: color(0x1D1D1B29),
        selection: color(0x4A47F7FF),
        selection_text: color(0xFFFFFFFF),
        decoration_one: color(0x4A47F7FF),
        decoration_two: color(0xD86F4EFF),
        decoration_three: color(0xF1E600FF),
        decoration_four: color(0xD38BE7FF),
        decoration_five: color(0xCBF02CFF),
    }
}

fn openmove_1_dark() -> ThemePalette {
    ThemePalette {
        page: color(0x222228FF),
        surface: color(0x2D2C4FFF),
        brand: color(0x4A47F7FF),
        brand_hover: color(0x5753FAFF),
        brand_pressed: color(0x3C38DDFF),
        on_brand: color(0xFFFFFFFF),
        text: color(0xF5F5FFFF),
        text_secondary: color(0xC4C3E6FF),
        text_brand: color(0xAAA8FFFF),
        timer_title: color(0xD38BE7FF),
        surface_accent: color(0xD38BE7FF),
        border_subtle: color(0x7A78D0FF),
        list_row: color(0x2D2C4FFF),
        list_row_border: color(0x2D2C4FFF),
        turn: color(0xD86F4EFF),
        turn_text: color(0xF5A38AFF),
        turn_soft: color(0x3A2A24FF),
        turn_tint: color(0x704034FF),
        on_turn: color(0x1D1D1BFF),
        notice: color(0xF6C453FF),
        focus: color(0xCBF02CFF),
        error: color(0xFFB4ABFF),
        danger: color(0x4A47F7FF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0xD38BE7FF),
        timer_paused: color(0xC4C3E6FF),
        timer_warning: color(0xF6C453FF),
        timer_expired: color(0xFFB4ABFF),
        shadow: color(0x00000066),
        selection: color(0x4A47F7FF),
        selection_text: color(0xFFFFFFFF),
        decoration_one: color(0x7673FFFF),
        decoration_two: color(0xD86F4EFF),
        decoration_three: color(0xF1E600FF),
        decoration_four: color(0xD38BE7FF),
        decoration_five: color(0xCBF02CFF),
    }
}

fn openmove_2_light() -> ThemePalette {
    ThemePalette {
        page: color(0xFFFFFFFF),
        surface: color(0xEEEEFFFF),
        brand: color(0x4A47F7FF),
        brand_hover: color(0x3C38DDFF),
        brand_pressed: color(0x302BC3FF),
        on_brand: color(0xFFFFFFFF),
        text: color(0x1D1D1BFF),
        text_secondary: color(0x55546AFF),
        text_brand: color(0x4A47F7FF),
        timer_title: color(0x4A47F7FF),
        surface_accent: color(0x4A47F7FF),
        border_subtle: color(0x7774EEFF),
        list_row: color(0xEEEEFFFF),
        list_row_border: color(0x7774EEFF),
        turn: color(0xD86F4EFF),
        turn_text: color(0xA44428FF),
        turn_soft: color(0xFDEEE8FF),
        turn_tint: color(0xF4C6B7FF),
        on_turn: color(0x1D1D1BFF),
        notice: color(0x8A5500FF),
        focus: color(0xC2502EFF),
        error: color(0xB42318FF),
        danger: color(0x1D1D1BFF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0x4A47F7FF),
        timer_paused: color(0x55546AFF),
        timer_warning: color(0x8A5500FF),
        timer_expired: color(0xB42318FF),
        shadow: color(0x1D1D1B29),
        selection: color(0x4A47F7FF),
        selection_text: color(0xFFFFFFFF),
        decoration_one: color(0x4A47F7FF),
        decoration_two: color(0xD86F4EFF),
        decoration_three: color(0xF1E600FF),
        decoration_four: color(0xD38BE7FF),
        decoration_five: color(0xCBF02CFF),
    }
}

fn openmove_2_dark() -> ThemePalette {
    ThemePalette {
        page: color(0x1D1D1BFF),
        surface: color(0x4A47F7FF),
        brand: color(0xCBF02CFF),
        brand_hover: color(0xD6F55AFF),
        brand_pressed: color(0xB5D81FFF),
        on_brand: color(0x1D1D1BFF),
        text: color(0xFFFFFFFF),
        text_secondary: color(0xE2E1FFFF),
        text_brand: color(0xD38BE7FF),
        timer_title: color(0xD38BE7FF),
        surface_accent: color(0xF1E600FF),
        border_subtle: color(0xC9C8FFFF),
        list_row: color(0x1D1D1BFF),
        list_row_border: color(0x4A47F7FF),
        turn: color(0xD86F4EFF),
        turn_text: color(0xF5A38AFF),
        turn_soft: color(0x3A2A24FF),
        turn_tint: color(0x704034FF),
        on_turn: color(0x1D1D1BFF),
        notice: color(0xF1E600FF),
        focus: color(0xF1E600FF),
        error: color(0xFFB4ABFF),
        danger: color(0x4A47F7FF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0xCBF02CFF),
        timer_paused: color(0xE2E1FFFF),
        timer_warning: color(0xF1E600FF),
        timer_expired: color(0xFFB4ABFF),
        shadow: color(0x00000066),
        selection: color(0xCBF02CFF),
        selection_text: color(0x1D1D1BFF),
        decoration_one: color(0x4A47F7FF),
        decoration_two: color(0xD86F4EFF),
        decoration_three: color(0xF1E600FF),
        decoration_four: color(0xD38BE7FF),
        decoration_five: color(0xCBF02CFF),
    }
}

fn openmove_3_light() -> ThemePalette {
    ThemePalette {
        page: color(0xECEBFFFF),
        surface: color(0xFFFFFFFF),
        brand: color(0x4A47F7FF),
        brand_hover: color(0x3C38DDFF),
        brand_pressed: color(0x302BC3FF),
        on_brand: color(0xFFFFFFFF),
        text: color(0x15144AFF),
        text_secondary: color(0x47458CFF),
        text_brand: color(0x3B38E0FF),
        timer_title: color(0x3B38E0FF),
        surface_accent: color(0x3B38E0FF),
        border_subtle: color(0x6F6CE6FF),
        list_row: color(0xFFFFFFFF),
        list_row_border: color(0x6F6CE6FF),
        turn: color(0xD86F4EFF),
        turn_text: color(0x9C3F24FF),
        turn_soft: color(0xFBE6DDFF),
        turn_tint: color(0xF2BFAEFF),
        on_turn: color(0x15144AFF),
        notice: color(0x7D4D00FF),
        focus: color(0xC2502EFF),
        error: color(0xB42318FF),
        danger: color(0x15144AFF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0x4A47F7FF),
        timer_paused: color(0x47458CFF),
        timer_warning: color(0x7D4D00FF),
        timer_expired: color(0xB42318FF),
        shadow: color(0x15144A29),
        selection: color(0x4A47F7FF),
        selection_text: color(0xFFFFFFFF),
        decoration_one: color(0x4A47F7FF),
        decoration_two: color(0xD86F4EFF),
        decoration_three: color(0xF1E600FF),
        decoration_four: color(0xD38BE7FF),
        decoration_five: color(0xCBF02CFF),
    }
}

fn openmove_3_dark() -> ThemePalette {
    ThemePalette {
        page: color(0x4A47F7FF),
        surface: color(0x3633D6FF),
        brand: color(0xFFFFFFFF),
        brand_hover: color(0xEEEEFFFF),
        brand_pressed: color(0xDCDBFFFF),
        on_brand: color(0x3633D6FF),
        text: color(0xFFFFFFFF),
        text_secondary: color(0xE2E1FFFF),
        text_brand: color(0xF1E600FF),
        timer_title: color(0xF1E600FF),
        surface_accent: color(0xF1E600FF),
        border_subtle: color(0xC4C3FFFF),
        list_row: color(0x3633D6FF),
        list_row_border: color(0x3633D6FF),
        turn: color(0xD86F4EFF),
        turn_text: color(0xFFC9B6FF),
        turn_soft: color(0x2A27B4FF),
        turn_tint: color(0x5B58ECFF),
        on_turn: color(0x1D1D1BFF),
        notice: color(0xF1E600FF),
        focus: color(0xCBF02CFF),
        error: color(0xFFE0DCFF),
        danger: color(0x1D1D1BFF),
        on_danger: color(0xFFFFFFFF),
        timer_normal: color(0xFFFFFFFF),
        timer_paused: color(0xE2E1FFFF),
        timer_warning: color(0xF1E600FF),
        timer_expired: color(0xFFE0DCFF),
        shadow: color(0x00000066),
        selection: color(0xF1E600FF),
        selection_text: color(0x1D1D1BFF),
        decoration_one: color(0xFFFFFFFF),
        decoration_two: color(0xD86F4EFF),
        decoration_three: color(0xF1E600FF),
        decoration_four: color(0xD38BE7FF),
        decoration_five: color(0xCBF02CFF),
    }
}

/// Converts an RGBA token without changing its opacity or color space.
fn color(rgba: u32) -> slint::Color {
    let [red, green, blue, alpha] = rgba.to_be_bytes();
    slint::Color::from_argb_u8(alpha, red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_catalog;

    fn contrast(first: slint::Color, second: slint::Color) -> f64 {
        fn luminance(color: slint::Color) -> f64 {
            let channel = |value: u8| {
                let value = f64::from(value) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(color.red())
                + 0.7152 * channel(color.green())
                + 0.0722 * channel(color.blue())
        }
        let first = luminance(first);
        let second = luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn text_and_action_states_have_readable_contrast_in_every_palette() {
        for theme in theme_catalog::THEME_CATALOG.iter().map(|entry| entry.theme) {
            for dark in [false, true] {
                let p = palette(theme, dark);
                let pairs = [
                    ("text/page", p.text, p.page),
                    ("text/surface", p.text, p.surface),
                    ("secondary/page", p.text_secondary, p.page),
                    ("secondary/surface", p.text_secondary, p.surface),
                    ("text/list row", p.text, p.list_row),
                    ("secondary/list row", p.text_secondary, p.list_row),
                    ("title/page", p.timer_title, p.page),
                    ("surface-accent/surface", p.surface_accent, p.surface),
                    ("brand text/page", p.text_brand, p.page),
                    ("action", p.on_brand, p.brand),
                    ("action hover", p.on_brand, p.brand_hover),
                    ("action pressed", p.on_brand, p.brand_pressed),
                    ("selection", p.selection_text, p.selection),
                    ("error/page", p.error, p.page),
                    ("notice/page", p.notice, p.page),
                    ("turn text", p.turn_text, p.turn_soft),
                    ("turn button", p.on_turn, p.turn),
                    ("tooltip", p.on_danger, p.danger),
                ];
                for (name, text, background) in pairs {
                    let ratio = contrast(text, background);
                    assert!(ratio >= 4.5, "{theme:?} dark={dark} {name}: {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn keyboard_focus_is_distinct_on_pages_and_panels() {
        for theme in theme_catalog::THEME_CATALOG.iter().map(|entry| entry.theme) {
            for dark in [false, true] {
                let p = palette(theme, dark);
                for background in [p.page, p.surface, p.turn_soft] {
                    assert!(
                        contrast(p.focus, background) >= 3.0,
                        "{theme:?} dark={dark}"
                    );
                }
            }
        }
    }
}
