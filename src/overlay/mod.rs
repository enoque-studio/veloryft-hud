//! The native overlay: Win32 layered windows presenting Direct2D content.
//!
//! Each HUD panel is its own small `WS_EX_LAYERED | WS_EX_TRANSPARENT` window.
//! Direct2D draws into a premultiplied DIB which is handed to the compositor via
//! `UpdateLayeredWindow`, giving per-pixel alpha and cross-process click-through
//! without a fullscreen surface. See
//! `docs/decisions/adr_0001_native_direct2d_overlay.md`.
//!
//! Module boundaries:
//!
//! | module              | owns                                                   |
//! |---------------------|--------------------------------------------------------|
//! | [`runtime`]         | lifecycle, window procedures, the render loop          |
//! | [`window`]          | window creation, layered presentation, click-through   |
//! | [`graphics`]        | Direct2D, DirectWrite and GDI resources                |
//! | [`frame_pacing`]    | frame intervals and adaptive degradation               |
//! | [`geometry`]        | panel identity, design sizes, rectangle math           |
//! | [`theme`]           | colors, glass settings, color conversion               |
//! | [`settings`]        | loading, validating and saving persisted state         |
//! | [`editing`]         | dragging, resizing, snapping                           |
//! | [`tray`]            | the system-tray icon and its menu                      |
//! | [`panels`]          | per-panel layout and drawing                           |
//! | [`track_recording`] | the optional track-mapping adapter                     |
//!
//! Performance contract: the render loop allocates nothing per frame, takes no
//! lock on the critical throttle path, and never logs per frame. Anything that
//! would violate that belongs outside the loop, not inside it with a guard.

pub(crate) mod editing;
pub(crate) mod frame_pacing;
pub(crate) mod geometry;
pub(crate) mod graphics;
pub(crate) mod panels;
pub(crate) mod runtime;
pub(crate) mod settings;
pub(crate) mod theme;
pub(crate) mod track_recording;
pub(crate) mod tray;
pub(crate) mod window;

pub(crate) use runtime::run;
