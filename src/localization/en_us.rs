use super::Catalog;

/// Complete English catalog and fallback for every future locale.
pub(super) static CATALOG: Catalog = Catalog {
    language_tag: "en-US",
    app_name: "Veloryft",

    startup_not_authorized: "This build is not authorized to run on this machine.",
    startup_overlay_unavailable: "The Veloryft overlay could not be created. This usually means \
        Direct2D or DirectWrite is unavailable. Update your graphics driver and make sure you are \
        running 64-bit Windows 10 or later.",
    startup_windows_only: "Veloryft runs only on Windows because the overlay uses Win32 layered \
        windows and Direct2D.",
    startup_dialog_title: "Veloryft could not start",
    technical_detail_heading: "Technical detail",
    full_log_heading: "A full log is available at",

    tray_show_hide_hud: "Show/Hide HUD",
    tray_exit: "Exit",

    edit_hud: "EDIT HUD",
    color: "COLOR",
    card: "CARD",
    gear: "GEAR",
    panels: "PANELS",
    glass: "GLASS",
    tone: "TONE",
    panel_rpm: "RPM",
    panel_tires: "TIRES",
    panel_g_force: "G",
    panel_altitude: "ALT",

    elevation: "ELEVATION",
    minimum: "MIN",
    maximum: "MAX",
    tires: "TIRES",
    tire_corners: ["FL", "FR", "RL", "RR"],
    g_force: "G-FORCE",
    brake: "BRAKE",
    throttle: "THROTTLE",
    reverse_gear: "R",
    missing_value: "–",

    kilometres_abbreviation: "km",
    metres_abbreviation: "m",
    kilometres_per_hour: "km/h",
    metric_horsepower: "hp",
    degrees_celsius: "°C",
};
