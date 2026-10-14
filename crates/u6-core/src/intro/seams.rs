//! Seams of the intro to modules that do not exist yet (the dispatcher reconciles them).
//!
//! - Music: `u` plays the `ULTIMA_?a.MOD` songs with its own copy of the MED player (hunk 0).
//!   The sound unit ports MED for `game`; the intro calls it through [`music_load`],
//!   [`music_start`] and [`music_stop`]. Fallback: silence (the intro's timing does not depend
//!   on the music; `u` never waits for a song).
//! - Clock: the VCR in the TV room shows the local time (`time` + `localtime`, 0x101c0c /
//!   0x1021bc). The core reads no clock; [`local_time`] is for the platform. Fallback 0:00 (the
//!   VCR shows 12:00).

use super::state::IntroState;

/// Contract: load the MED module `name` (a `ULTIMA_?a.MOD` of the user's files) for playing;
/// `u` reads it into its 46390-byte buffer and relocates it (0x116602). No effect on the intro
/// state. Owner: the sound unit.
pub(crate) fn music_load(name: &[u8]) {
    let _ = name;
    crate::todo_stub!("u 0x10f596 load a MED module (sound seam)", ());
}

/// Contract: start the loaded module (0x10fafc → MED 0x100fa4). Owner: the sound unit.
pub(crate) fn music_start() {
    crate::todo_stub!("u 0x10fafc start the MED module (sound seam)", ());
}

/// Contract: stop the playing module (0x101010). Owner: the sound unit.
pub(crate) fn music_stop() {
    crate::todo_stub!("u 0x101010 stop the MED module (sound seam)", ());
}

/// Contract: the local time of day as (hour 0..23, minute 0..59), what `localtime(time())`
/// gives `u`. Owner: the platform (via Io). Fallback (0, 0).
pub(crate) fn local_time() -> (u16, u16) {
    crate::todo_stub!("u 0x1021bc local time of day (platform seam)", (0, 0))
}

/// Original: u 0x1036bc (music: switch to song `n`; 0 = stop)
///
/// The same song again does nothing; a playing one is stopped first. Song 2 (the opening) is
/// only loaded: the opening starts it with its first picture (0x10fafc).
pub fn play_song(st: &mut IntroState, n: u16) {
    if n == st.song {
        return;
    }
    if st.song != 0 {
        music_stop();
        st.song = 0;
    }
    if n == 0 {
        return;
    }
    let name = st
        .exe
        .song_names
        .get(usize::from(n))
        .cloned()
        .unwrap_or_default();
    music_load(&name);
    if n != 2 {
        music_start();
    }
    st.song = n;
}

/// Original: u 0x10fafc (music: start the loaded song)
pub fn start_music(st: &mut IntroState) {
    let _ = st;
    music_start();
}
