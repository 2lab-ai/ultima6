//! Helpers of `u` that move whole screens: the swaps through the screen view A4+0x1784 and the
//! key checks the scenes poll each frame.

use crate::engine::Io;

use super::state::IntroState;

/// Original: u 0x113db2 (display: swap with the screen view following the drawing buffer)
///
/// The port shows the swapped buffer for one frame here: 0x113db2 waits for raster line 200
/// and the copper takes the new list at the next frame, so a scene that swaps every iteration
/// runs at most one swap per frame.
pub async fn swap(st: &mut IntroState, io: &Io) {
    st.views.screen.planes = st.video.swap(&st.exe);
    st.video.wait_frame(io).await;
}

/// Original: u 0x113df8 (display: swap through the gradient copper lists, as [`swap`])
pub async fn swap_gradient(st: &mut IntroState, io: &Io) {
    st.views.screen.planes = st.video.swap_gradient(&st.exe);
    st.video.wait_frame(io).await;
}

/// Original: u 0x1039d2 (input: the scenes' per-frame check)
///
/// A mouse button returns 1 (the port has none). A waiting key is taken: a digit picks TV
/// script `digit % 7` (0x1029a0 = `atoi` of the one-character buffer) and returns 0, any other
/// code is returned. Nothing waiting returns 0.
pub fn poll_key(st: &mut IntroState, io: &Io) -> u16 {
    if !st.keys.ready(io) {
        return 0;
    }
    let c = st.keys.take(io);
    let digit = st
        .keys
        .ctype
        .get(usize::from(c & 0xff))
        .is_some_and(|t| t & 4 != 0);
    if digit {
        let n = (c as u8).wrapping_sub(b'0') as i16 % 7;
        super::opening::set_script(st, n);
        return 0;
    }
    c
}

/// Original: u 0x1039b8 (input: [`poll_key`], with Escape setting the skip flag A4+0x25ea)
pub fn check_key(st: &mut IntroState, io: &Io) -> u16 {
    let c = poll_key(st, io);
    if c == 0x1b {
        st.skip = 1;
    }
    c
}

/// Waits for a key with [`poll_key`], one frame per poll (the original spins).
pub async fn wait_key(st: &mut IntroState, io: &Io) {
    while poll_key(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
}

/// Original: u 0x1055ea (opening: swap, then the opening's views follow the drawing buffer)
pub async fn show(st: &mut IntroState, io: &Io) {
    swap(st, io).await;
    let p = st.views.screen.planes;
    let v = &mut st.views;
    v.draw.planes = p;
    v.front.planes = p;
    v.pan.planes = p;
    v.room.planes = p;
}
