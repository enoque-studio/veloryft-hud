//! Loading, validating, and saving the overlay's persisted settings.
//!
//! Two plain-text files live in [`crate::paths::config_dir`]:
//!
//! * `layout.txt` — one line per panel, `key=x,y,w,h,visible`.
//! * `theme.txt` — one `key=value` line per theme field.
//!
//! Both formats are deliberately line-oriented and forgiving. Users edit them by
//! hand, and older releases wrote fewer fields, so parsing skips anything it
//! does not recognize rather than discarding the whole file. The `visible` field
//! was added after the first release: a layout without it loads as visible.
//!
//! Values are clamped on load, not on save, so a hand-edited file can never put
//! the overlay into a state the UI cannot get out of.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::geometry::Kind;
use super::theme::{Theme, GLASS_ALPHA_MAX, GLASS_ALPHA_MIN, THEME, THEME_DEFAULT};
use crate::diagnostics::{log_warn, Once};
use crate::paths;

/// One panel's persisted window placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayoutEntry {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub visible: bool,
}

/// A read-only profile directory fails on every save. Report once each.
static LAYOUT_SAVE_FAILED: Once = Once::new();
static THEME_SAVE_FAILED: Once = Once::new();

fn layout_path() -> PathBuf {
    paths::config_dir().join("layout.txt")
}

fn theme_path() -> PathBuf {
    paths::config_dir().join("theme.txt")
}

/// Reads `layout.txt`. A missing or unreadable file yields an empty map, which
/// makes every panel fall back to its default position.
pub(crate) fn load_layout() -> HashMap<String, LayoutEntry> {
    fs::read_to_string(layout_path())
        .map(|text| parse_layout(&text))
        .unwrap_or_default()
}

/// Writes `layout.txt` for the given panels.
pub(crate) fn save_layout(entries: &[(Kind, LayoutEntry)]) {
    write_settings_file(
        &layout_path(),
        &format_layout(entries),
        &LAYOUT_SAVE_FAILED,
        "layout",
    );
}

/// Reads `theme.txt`, falling back to [`THEME_DEFAULT`] for anything missing.
pub(crate) fn load_theme() -> Theme {
    fs::read_to_string(theme_path())
        .map(|text| parse_theme(&text))
        .unwrap_or(THEME_DEFAULT)
}

/// Writes the live theme to `theme.txt`.
pub(crate) fn save_theme() {
    write_settings_file(
        &theme_path(),
        &format_theme(THEME.get()),
        &THEME_SAVE_FAILED,
        "theme",
    );
}

fn write_settings_file(path: &Path, contents: &str, latch: &Once, what: &str) {
    if let Some(directory) = path.parent() {
        let _ = fs::create_dir_all(directory);
    }
    if let Err(cause) = fs::write(path, contents) {
        if latch.first() {
            log_warn!(
                "{what} could not be saved to {}: {cause} (repeats suppressed)",
                path.display()
            );
        }
    }
}

/// `key=x,y,w,h[,visible]` per line. Lines that do not parse are skipped.
fn parse_layout(text: &str) -> HashMap<String, LayoutEntry> {
    let mut layout = HashMap::new();
    for line in text.lines() {
        let Some((key, values)) = line.split_once('=') else {
            continue;
        };
        let numbers: Vec<i32> = values
            .split(',')
            .filter_map(|number| number.trim().parse().ok())
            .collect();
        if numbers.len() >= 4 {
            layout.insert(
                key.trim().to_string(),
                LayoutEntry {
                    x: numbers[0],
                    y: numbers[1],
                    w: numbers[2],
                    h: numbers[3],
                    // Written since the visibility toggles were added; absent in
                    // layouts saved before that, where everything was visible.
                    visible: numbers.get(4).copied().unwrap_or(1) != 0,
                },
            );
        }
    }
    layout
}

fn format_layout(entries: &[(Kind, LayoutEntry)]) -> String {
    let mut text = String::new();
    for (kind, entry) in entries {
        text.push_str(&format!(
            "{}={},{},{},{},{}\n",
            kind.key(),
            entry.x,
            entry.y,
            entry.w,
            entry.h,
            u8::from(entry.visible),
        ));
    }
    text
}

fn parse_theme(text: &str) -> Theme {
    let mut theme = THEME_DEFAULT;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "hue" => {
                if let Ok(hue) = value.parse() {
                    theme.hue = hue;
                }
            }
            "target" => theme.gear_target = value == "gear",
            "glass_alpha" => {
                if let Ok(alpha) = value.parse::<f32>() {
                    theme.glass_alpha = alpha.clamp(GLASS_ALPHA_MIN, GLASS_ALPHA_MAX);
                }
            }
            "glass_tone" => {
                if let Ok(tone) = value.parse::<f32>() {
                    // Negative means "neutral dark"; anything else is a hue.
                    theme.glass_tone = if tone < 0.0 {
                        -1.0
                    } else {
                        tone.rem_euclid(360.0)
                    };
                }
            }
            _ => {}
        }
    }
    theme
}

fn format_theme(theme: Theme) -> String {
    format!(
        "hue={}\ntarget={}\nglass_alpha={}\nglass_tone={}\n",
        theme.hue,
        if theme.gear_target { "gear" } else { "card" },
        theme.glass_alpha,
        theme.glass_tone,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(x: i32, y: i32, visible: bool) -> LayoutEntry {
        LayoutEntry {
            x,
            y,
            w: 680,
            h: 240,
            visible,
        }
    }

    #[test]
    fn layout_round_trips_through_its_text_format() {
        let saved = [
            (Kind::Cluster, entry(10, 20, true)),
            (Kind::Tires, entry(30, 40, false)),
        ];
        let parsed = parse_layout(&format_layout(&saved));

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["cluster"], saved[0].1);
        assert_eq!(parsed["tires"], saved[1].1);
    }

    #[test]
    fn a_layout_without_the_visible_field_still_loads() {
        // Written by releases before the visibility toggles existed. Those
        // panels were all visible, so that is what they must load as.
        let parsed = parse_layout("cluster=10,20,680,240\n");

        assert_eq!(parsed["cluster"], entry(10, 20, true));
    }

    #[test]
    fn unparsable_layout_lines_are_skipped_not_fatal() {
        let parsed = parse_layout(
            "cluster=10,20,680,240,1\n\
             garbage without an equals sign\n\
             tires=only,three,values\n\
             =12,13,14,15\n\
             gforce=1,2,3,4,0\n",
        );

        assert_eq!(
            parsed.len(),
            3,
            "the empty key is kept, the bad lines are not"
        );
        assert!(parsed.contains_key("cluster"));
        assert!(parsed.contains_key("gforce"));
        assert!(!parsed.contains_key("tires"));
    }

    #[test]
    fn negative_positions_survive_for_monitors_left_of_the_primary() {
        let parsed = parse_layout("cluster=-1920,-40,680,240,1\n");

        assert_eq!(parsed["cluster"].x, -1920);
        assert_eq!(parsed["cluster"].y, -40);
    }

    #[test]
    fn theme_round_trips_through_its_text_format() {
        let saved = Theme {
            hue: 218.0,
            gear_target: true,
            glass_alpha: 0.5,
            glass_tone: 120.0,
        };
        let parsed = parse_theme(&format_theme(saved));

        assert_eq!(parsed.hue, saved.hue);
        assert_eq!(parsed.gear_target, saved.gear_target);
        assert_eq!(parsed.glass_alpha, saved.glass_alpha);
        assert_eq!(parsed.glass_tone, saved.glass_tone);
    }

    #[test]
    fn a_missing_theme_field_keeps_its_default() {
        let parsed = parse_theme("hue=100\n");

        assert_eq!(parsed.hue, 100.0);
        assert_eq!(parsed.glass_alpha, THEME_DEFAULT.glass_alpha);
        assert_eq!(parsed.glass_tone, THEME_DEFAULT.glass_tone);
        assert_eq!(parsed.gear_target, THEME_DEFAULT.gear_target);
    }

    #[test]
    fn a_hand_edited_theme_cannot_make_the_glass_unusable() {
        // Fully transparent or fully opaque glass would leave the user with no
        // way back through the UI, so both ends are clamped on load.
        assert_eq!(parse_theme("glass_alpha=-5\n").glass_alpha, GLASS_ALPHA_MIN);
        assert_eq!(parse_theme("glass_alpha=99\n").glass_alpha, GLASS_ALPHA_MAX);
    }

    #[test]
    fn out_of_range_glass_tone_wraps_and_negative_means_neutral() {
        assert_eq!(parse_theme("glass_tone=-1\n").glass_tone, -1.0);
        assert_eq!(parse_theme("glass_tone=-999\n").glass_tone, -1.0);
        assert_eq!(parse_theme("glass_tone=400\n").glass_tone, 40.0);
        assert_eq!(parse_theme("glass_tone=360\n").glass_tone, 0.0);
    }

    #[test]
    fn an_unparsable_theme_value_leaves_the_default_in_place() {
        assert_eq!(parse_theme("hue=not-a-number\n").hue, THEME_DEFAULT.hue);
    }

    #[test]
    fn the_shift_target_is_stored_as_a_word_not_a_flag() {
        // "gear" and "card" are what already sits in users' theme.txt.
        assert!(parse_theme("target=gear\n").gear_target);
        assert!(!parse_theme("target=card\n").gear_target);
        assert!(!parse_theme("target=anything else\n").gear_target);
    }
}
