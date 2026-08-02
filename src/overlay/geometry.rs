//! Panel identity, design sizes, and the rectangle math the panels share.
//!
//! Every panel is authored against a fixed *design* size in
//! device-independent pixels. The user may resize its window freely; the
//! drawing code then maps the design space onto the actual window through
//! [`panel_canvas`], which fills the window without letterboxing and keeps the
//! scale uniform, so a resized card looks identical, only bigger.
//!
//! Edit mode snaps windows onto a [`GRID`] grid, but only within [`MAGNET`]
//! pixels — see [`soft_snap`]. Outside that distance movement stays free, so
//! positioning never feels quantized.

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;

use crate::telemetry::display::PanelKind;

/// Edit-mode grid pitch, in pixels.
pub(crate) const GRID: i32 = 40;
/// Snap radius: a window within this many pixels of a grid line clicks onto it.
pub(crate) const MAGNET: i32 = 12;
/// Width of the resize band along a window edge, in pixels.
pub(crate) const RESIZE_MARGIN: i32 = 8;
/// Smallest a panel may be resized to, on either axis.
pub(crate) const MIN_SZ: i32 = 80;

/// Which HUD panel a window is. Each kind owns a design size, a default screen
/// position, and the settings key its layout is stored under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Cluster,
    Tires,
    GForce,
    Elevation,
}

/// Number of panels the overlay creates.
pub(crate) const PANEL_COUNT: usize = 4;
/// Every panel, in the order their visibility flags are stored.
pub(crate) const PANEL_KINDS: [Kind; PANEL_COUNT] =
    [Kind::Cluster, Kind::Tires, Kind::GForce, Kind::Elevation];

// --- Design sizes, in device-independent pixels ---
pub(crate) const CL_W: i32 = 680;
pub(crate) const CL_H: i32 = 240;
pub(crate) const CL_BAR_X: f32 = 20.0;
pub(crate) const CL_BAR_W: f32 = 640.0;
pub(crate) const CL_BAR_H: f32 = 6.0;
pub(crate) const CL_PEDAL_H: f32 = 5.0;
pub(crate) const CL_PAD: f32 = 8.0;
pub(crate) const CL_GEAR_W: f32 = 160.0;
pub(crate) const CL_GEAR_GAP: f32 = 14.0;
pub(crate) const CL_GEAR_Y_SHIFT: f32 = 14.0;
pub(crate) const CL_RIGHT_X: f32 = CL_BAR_X + CL_GEAR_W + CL_GEAR_GAP;
pub(crate) const CL_RIGHT_R: f32 = CL_BAR_X + CL_BAR_W;
pub(crate) const CL_RPM_Y: f32 = 50.0;
pub(crate) const CL_META_Y: f32 = 86.0;
pub(crate) const CL_SPEED_Y: f32 = 124.0;
pub(crate) const CL_BRAKE_Y: f32 = 156.0;
pub(crate) const CL_ACCEL_Y: f32 = 188.0;
pub(crate) const TH_W: i32 = (CL_RIGHT_R - CL_RIGHT_X) as i32;
pub(crate) const TH_H: i32 = 34;
// --- Pneus ---
pub(crate) const TR_W: i32 = 256;
pub(crate) const TR_H: i32 = 272;
// --- G-force ---
pub(crate) const GF_W: i32 = 200;
pub(crate) const GF_H: i32 = 250;
// --- Elevação ---
pub(crate) const EL_W: i32 = 300;
pub(crate) const EL_H: i32 = 200;

impl Kind {
    /// Design size in device-independent pixels.
    pub fn size(self) -> (i32, i32) {
        match self {
            Kind::Cluster => (CL_W, CL_H),
            Kind::Tires => (TR_W, TR_H),
            Kind::GForce => (GF_W, GF_H),
            Kind::Elevation => (EL_W, EL_H),
        }
    }
    /// Where the panel sits on a screen of `sw` by `sh` before the user moves it.
    pub fn default_pos(self, sw: i32, sh: i32) -> (i32, i32) {
        match self {
            Kind::Cluster => ((sw - CL_W) / 2, sh - CL_H - 30),
            Kind::Tires => (40, sh - TR_H - 30),
            Kind::GForce => (sw - GF_W - 40, sh - GF_H - 30),
            Kind::Elevation => (sw - EL_W - 40, 40),
        }
    }
    /// Stable key used in the persisted layout file. Never change these.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Cluster => "cluster",
            Kind::Tires => "tires",
            Kind::GForce => "gforce",
            Kind::Elevation => "elevation",
        }
    }
    /// Index into the visibility array.
    pub fn idx(self) -> usize {
        match self {
            Kind::Cluster => 0,
            Kind::Tires => 1,
            Kind::GForce => 2,
            Kind::Elevation => 3,
        }
    }
    /// Label shown on the edit toolbar's visibility toggles.
    pub fn short_label(self) -> &'static str {
        match self {
            Kind::Cluster => "RPM",
            Kind::Tires => "PNEUS",
            Kind::GForce => "G",
            Kind::Elevation => "ALT",
        }
    }
    /// The telemetry-side counterpart, used to look up smoothed values.
    pub fn panel_kind(self) -> PanelKind {
        match self {
            Kind::Cluster => PanelKind::Cluster,
            Kind::Tires => PanelKind::Tires,
            Kind::GForce => PanelKind::GForce,
            Kind::Elevation => PanelKind::Elevation,
        }
    }
}

/// Direct2D rectangle from its four edges.
pub(crate) fn rectf(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}
/// Rounded-rectangle wrapper with equal x and y radii.
pub(crate) fn rounded(r: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: r,
        radiusX: radius,
        radiusY: radius,
    }
}

/// Drawing space that fills the window without letterboxing, at uniform scale.
///
/// Returns `(width, height, scale)` in design units: draw against `width` by
/// `height`, and `scale` maps that onto the window.
pub(crate) fn panel_canvas(sw: f32, sh: f32, base_w: f32, base_h: f32) -> (f32, f32, f32) {
    let aspect = sw / sh;
    let base_aspect = base_w / base_h;
    let (w, h) = if aspect >= base_aspect {
        (base_h * aspect, base_h)
    } else {
        (base_w, base_w / aspect)
    };
    (w, h, sw / w)
}

/// Corner radius for a card of this size, clamped to stay tasteful at extremes.
pub(crate) fn glass_radius(w: f32, h: f32) -> f32 {
    (w.min(h) * 0.078).clamp(8.0, 28.0)
}
/// Soft magnet: snaps to the nearest grid line only within [`MAGNET`] pixels.
/// Further away the position stays exactly where the user put it.
pub(crate) fn soft_snap(v: i32) -> i32 {
    let nearest = ((v + GRID / 2).div_euclid(GRID)) * GRID;
    if (v - nearest).abs() <= MAGNET {
        nearest
    } else {
        v
    }
}

/// Unpacks the signed x and y a Win32 mouse message packs into its `LPARAM`.
pub(crate) fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    let v = lparam.0 as u32;
    (
        (v & 0xFFFF) as i16 as i32,
        ((v >> 16) & 0xFFFF) as i16 as i32,
    )
}

/// Hit test against a `(left, top, right, bottom)` tuple.
pub(crate) fn in_rect(x: i32, y: i32, r: (i32, i32, i32, i32)) -> bool {
    x >= r.0 && x < r.2 && y >= r.1 && y < r.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_kind_has_a_unique_index_and_settings_key() {
        let mut indices: Vec<usize> = PANEL_KINDS.iter().map(|kind| kind.idx()).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..PANEL_COUNT).collect::<Vec<_>>());

        let mut keys: Vec<&str> = PANEL_KINDS.iter().map(|kind| kind.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), PANEL_COUNT, "settings keys must not collide");
    }

    #[test]
    fn settings_keys_are_the_ones_already_written_to_disk() {
        // Changing any of these silently discards an existing user's layout.
        assert_eq!(Kind::Cluster.key(), "cluster");
        assert_eq!(Kind::Tires.key(), "tires");
        assert_eq!(Kind::GForce.key(), "gforce");
        assert_eq!(Kind::Elevation.key(), "elevation");
    }

    #[test]
    fn soft_snap_grabs_only_inside_the_magnet_radius() {
        assert_eq!(soft_snap(0), 0);
        assert_eq!(soft_snap(GRID + MAGNET), GRID, "just inside snaps");
        let free = GRID + MAGNET + 1;
        assert_eq!(soft_snap(free), free, "just outside stays put");
    }

    #[test]
    fn soft_snap_works_for_negative_coordinates_on_multi_monitor_setups() {
        // A monitor to the left of the primary one has negative x. Truncating
        // division would round toward zero and snap the wrong way, so soft_snap
        // uses div_euclid; these cases pin that down.
        assert_eq!(soft_snap(-GRID), -GRID);
        assert_eq!(soft_snap(-GRID - MAGNET), -GRID, "just inside still snaps");
        let free = -GRID - MAGNET - 1;
        assert_eq!(soft_snap(free), free, "just outside stays put");
    }

    #[test]
    fn panel_canvas_fills_the_window_and_keeps_scale_uniform() {
        // A window wider than the design aspect grows the canvas horizontally.
        let (width, height, scale) = panel_canvas(1360.0, 240.0, 680.0, 240.0);
        assert!((height - 240.0).abs() < f32::EPSILON);
        assert!((width - 1360.0).abs() < 0.01);
        assert!((scale - 1.0).abs() < 0.01);

        // Scaling the window uniformly scales the mapping, not the canvas.
        let (width, height, scale) = panel_canvas(1360.0, 480.0, 680.0, 240.0);
        assert!((width - 680.0).abs() < 0.01);
        assert!((height - 240.0).abs() < 0.01);
        assert!((scale - 2.0).abs() < 0.01);
    }

    #[test]
    fn glass_radius_stays_within_its_clamp() {
        assert_eq!(glass_radius(10.0, 10.0), 8.0);
        assert_eq!(glass_radius(10_000.0, 10_000.0), 28.0);
        assert!((8.0..=28.0).contains(&glass_radius(680.0, 240.0)));
    }

    #[test]
    fn lparam_xy_unpacks_negative_coordinates() {
        // Coordinates are signed 16-bit halves; a point left of the window is
        // negative and must not be read as a large positive number.
        let packed = LPARAM(((-5_i16 as u16 as u32) | ((-7_i16 as u16 as u32) << 16)) as isize);
        assert_eq!(lparam_xy(packed), (-5, -7));
    }

    #[test]
    fn in_rect_excludes_the_far_edges() {
        let rect = (10, 20, 30, 40);
        assert!(in_rect(10, 20, rect));
        assert!(in_rect(29, 39, rect));
        assert!(!in_rect(30, 39, rect));
        assert!(!in_rect(29, 40, rect));
        assert!(!in_rect(9, 20, rect));
    }
}
