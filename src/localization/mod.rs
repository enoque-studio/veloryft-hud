//! User-facing text and locale-aware value formatting.
//!
//! English is the source and fallback language. Keeping visible strings in a
//! typed catalog prevents rendering, windowing, and diagnostics code from
//! becoming translation stores. The catalog contains only `&'static str`
//! values, so looking up a label does not allocate or lock in the render path.

mod en_us;

use std::fmt;
use std::path::Path;

/// Text used by the native UI. Fields are grouped by meaning rather than by
/// window so translators see one stable contract when another locale is added.
pub struct Catalog {
    /// BCP 47 language tag passed to DirectWrite for shaping and fallback.
    pub language_tag: &'static str,
    pub app_name: &'static str,

    pub startup_not_authorized: &'static str,
    pub startup_overlay_unavailable: &'static str,
    pub startup_windows_only: &'static str,
    pub startup_dialog_title: &'static str,
    pub technical_detail_heading: &'static str,
    pub full_log_heading: &'static str,

    pub tray_show_hide_hud: &'static str,
    pub tray_exit: &'static str,

    pub edit_hud: &'static str,
    pub color: &'static str,
    pub card: &'static str,
    pub gear: &'static str,
    pub panels: &'static str,
    pub glass: &'static str,
    pub tone: &'static str,
    pub panel_rpm: &'static str,
    pub panel_tires: &'static str,
    pub panel_g_force: &'static str,
    pub panel_altitude: &'static str,

    pub elevation: &'static str,
    pub minimum: &'static str,
    pub maximum: &'static str,
    pub tires: &'static str,
    pub tire_corners: [&'static str; 4],
    pub g_force: &'static str,
    pub brake: &'static str,
    pub throttle: &'static str,
    pub reverse_gear: &'static str,
    pub missing_value: &'static str,

    pub kilometres_abbreviation: &'static str,
    pub metres_abbreviation: &'static str,
    pub kilometres_per_hour: &'static str,
    pub metric_horsepower: &'static str,
    pub degrees_celsius: &'static str,
}

/// Active catalog. English is intentionally the only shipped locale until a
/// complete translation can be reviewed; unsupported partial locales are worse
/// than a predictable English fallback.
#[must_use]
pub const fn text() -> &'static Catalog {
    &en_us::CATALOG
}

/// Build the body of the native startup error dialog.
#[must_use]
pub fn startup_failure_body(
    summary: &str,
    detail: &fmt::Arguments<'_>,
    log_directory: &Path,
) -> String {
    let messages = text();
    format!(
        "{summary}\n\n{}:\n{detail}\n\n{}:\n{}",
        messages.technical_detail_heading,
        messages.full_log_heading,
        log_directory.display()
    )
}

/// Explain a UDP bind failure in terms an end user can act on.
#[must_use]
pub fn udp_bind_failure(port: u16, port_variable: &str) -> String {
    format!(
        "Veloryft could not listen for Forza telemetry on UDP port {port}.\n\n\
         Another program is probably already using it. Close that program, or set \
         {port_variable} to an available port and configure the same port in Forza's \
         Data Out settings.\n\nThe overlay will keep running, but it will remain hidden \
         until it receives telemetry."
    )
}

/// Format a distance measured in kilometres for the HUD.
#[must_use]
pub fn distance_km(value: f32) -> String {
    format!("{value:.2} {}", text().kilometres_abbreviation)
}

/// Format a whole-metre elevation for the HUD.
#[must_use]
pub fn elevation_m(value: f32) -> String {
    format!("{} {}", value.round() as i32, text().metres_abbreviation)
}

/// Format metric horsepower for the HUD.
#[must_use]
pub fn power_cv(value: f32) -> String {
    format!("{} {}", value.round() as i32, text().metric_horsepower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_catalog_has_a_bcp47_language_tag() {
        let tag = text().language_tag;
        assert!(!tag.is_empty());
        assert!(tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    }

    #[test]
    fn localized_value_formatters_include_their_units() {
        assert_eq!(distance_km(1.25), "1.25 km");
        assert_eq!(elevation_m(12.6), "13 m");
        assert_eq!(power_cv(100.4), "100 hp");
    }

    #[test]
    fn udp_failure_text_includes_the_port_and_configuration_variable() {
        let message = udp_bind_failure(20_066, "FORZA_UDP_PORT");
        assert!(message.contains("20066"));
        assert!(message.contains("FORZA_UDP_PORT"));
    }
}
