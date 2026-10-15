//! The music of `u`: the songs named by its pointer table A4+0xa0 (read at 0x1036fc..0x103704,
//! as `game` reads its own) through its copy of the MED replay routine.
//!
//! `u` links the same MED routine as `game` into its hunk 0: 0x1001f8..0x101130 and the
//! routine's tables are byte for byte `game`'s but for one relocated pointer (0x1002dc, the
//! null sample's address). So the port plays `u`'s songs on the host's one audio driver
//! ([`crate::sound::AudioDriver`]): `u`'s calls into the routine are the [`MedCall`]s `game`
//! makes, and the engine task hands them to the game state's call log as `u` makes them
//! ([`super::run`]), which the hosts replay after each tick as they replay `game`'s. `u`
//! never waits for a song, so the music changes nothing else in the intro.
//!
//! `u`'s setup 0x113bfe calls InitPlayer (0x100f7c) once; the host's driver is set up the same
//! way already. Its cleanup 0x113b68 calls RemPlayer ([`remove_player`]).

use std::cell::RefCell;
use std::rc::Rc;

use crate::sound::MedCall;

use super::state::IntroState;

/// Song 2, the opening's: [`play_song`] only loads it; the opening starts it with its first
/// picture.
const OPENING_SONG: u16 = 2;

/// Port-only: the calls `u` made into its MED routine that the engine task has not handed to
/// the game state yet.
#[derive(Debug, Default)]
pub struct Calls(Rc<RefCell<Vec<MedCall>>>);

impl Clone for Calls {
    /// A log of its own with the same calls (a copied intro state does not share its log).
    fn clone(&self) -> Self {
        Self(Rc::new(RefCell::new(self.0.borrow().clone())))
    }
}

impl Calls {
    pub fn push(&self, call: MedCall) {
        self.0.borrow_mut().push(call);
    }

    /// The calls made since the last take, oldest first.
    pub fn take(&self) -> Vec<MedCall> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    /// The same log, for the engine task to take from while `main` holds the intro state.
    pub(super) fn handle(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

/// Original: u 0x1036bc (music: switch to song `n`; 0 = stop)
///
/// The same song again does nothing; a playing one is stopped first. The song's file then goes
/// into the module buffer ([`MedCall::Load`]): opened with the disk prompt (0x10f596), read for
/// at most 50000 bytes (0x10f6ae with 0xc350), closed (0x10f716) and relocated (0x116602; the
/// port's player reads offsets). Every song but the opening's then starts (0x10fafc).
///
/// The original does none of it without a module buffer (A4+0xb42: setup 0x113bfe allocates
/// its 46390 bytes only with more than 0x316ce bytes of chip memory free). The port always has
/// one, the driver's 50000-byte buffer, which a file longer than 46390 bytes does not overflow
/// as it would the original's.
///
/// Replaces: u 0x10f596 (open with the disk prompt: the driver takes the file from the user's
/// files, a missing one leaves the buffer as it is), u 0x116602 (the module's relocation)
pub fn play_song(st: &mut IntroState, n: u16) {
    if n == st.song {
        return;
    }
    if st.song != 0 {
        stop_player(st);
        st.song = 0;
    }
    if n == 0 {
        return;
    }
    // A song past the table (no caller passes one) reads no file; the buffer stays.
    if let Some(name) = st
        .exe
        .song_names
        .get(usize::from(n))
        .filter(|s| !s.is_empty())
    {
        let name = String::from_utf8_lossy(name).into_owned();
        st.music.push(MedCall::Load(name));
    }
    if n != OPENING_SONG {
        start_music(st);
    }
    st.song = n;
}

/// Original: u 0x10fafc (music: start the module in the buffer, MED PlayModule 0x100fa4; its
/// argument is not used)
pub fn start_music(st: &mut IntroState) {
    st.music.push(MedCall::PlayBuffer);
}

/// Original: u 0x101010 (MED StopPlayer: `u`'s copy of `game`'s, which the host's driver runs)
pub fn stop_player(st: &mut IntroState) {
    st.music.push(MedCall::Stop);
}

/// Original: u 0x100f96 (MED RemPlayer, as cleanup 0x113b68 calls it at both exits of `u`:
/// StopPlayer once the player is set up, which setup always does; then the timer and the audio
/// channels are freed, which the host's driver keeps for `game`)
pub fn remove_player(st: &mut IntroState) {
    stop_player(st);
}
