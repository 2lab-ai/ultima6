//! Seams of the intro to modules that do not exist yet (the dispatcher reconciles them).
//!
//! - Clock: the VCR in the TV room shows the local time (`time` + `localtime`, 0x101c0c /
//!   0x1021bc). The core reads no clock; [`local_time`] is for the platform. Fallback 0:00 (the
//!   VCR shows 12:00).

/// Contract: the local time of day as (hour 0..23, minute 0..59), what `localtime(time())`
/// gives `u`. Owner: the platform (via Io). Fallback (0, 0).
pub(crate) fn local_time() -> (u16, u16) {
    crate::todo_stub!("u 0x1021bc local time of day (platform seam)", (0, 0))
}
