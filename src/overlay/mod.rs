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
//! | module            | owns                                                    |
//! |-------------------|---------------------------------------------------------|
//! | [`runtime`]       | lifecycle and the render loop                           |
//! | [`theme`]         | colors, glass settings, color conversion                |
//! | [`geometry`]      | rectangle and canvas math shared by panels               |
//! | [`frame_pacing`]  | frame intervals and adaptive degradation                |
//! | [`settings`]      | loading, validating, and saving persisted layout/theme  |
//! | [`track_recording`] | the optional track-mapping adapter                    |
//!
//! Performance contract: the render loop allocates nothing per frame, takes no
//! lock on the critical throttle path, and never logs per frame.

pub(crate) mod runtime;
pub(crate) mod track_recording;

pub(crate) use runtime::run;
