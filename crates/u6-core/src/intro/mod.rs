//! The separate Amiga program `u`: opening, main menu, character creation and the hand-off to
//! `game` (WU M2-intro).
//!
//! On the Amiga the launcher (`ultima6` / `ultcli`, a state machine on a byte) runs `PROGRAM:u
//! JJS` first; `u` plays the opening, shows the menu, creates a character into
//! `savegame/objlist` and exits with code 1, and the launcher then runs `PORTRAIT:game`, which
//! loads that objlist. The port runs `u` as [`run`] inside the game future, before `game`'s
//! boot reaches its character check, and applies the objlist it wrote to the game state.
//!
//! `u` is ported from its own image (`~/Games/ultima6/re/u.bin`, 24 hunks, A4 = 0x116798):
//! addresses in the `Original:` lines here are `u` addresses, written `u 0x…` so that the port
//! map (keyed by `game` addresses at the same image base) does not take them for `game`
//! functions. Everything the program shows comes from the user's files at run time: the
//! `.new` pictures (U6-LZW → a BE32 offset table → items of [`mem::ItemHeader`]: hot spot,
//! size, plane flags, the planes, a mask plane; `intro_1a/1b/2a.new` are bare 40000-byte
//! screens), the palettes, strings, passage layouts and creation tables inside `u`, the font
//! `u6.set`.
//!
//! Functions of `u` this module ports or stubs (see each item's `Original:` line):
//!
//! - u 0x1029d0 main · u 0x1031b8 main menu · u 0x10361a journey onward
//! - u 0x1041b0 opening · u 0x103a42 TV script · u 0x103966 · u 0x103830 · u 0x1040aa ·
//!   u 0x1039b8 / u 0x1039d2 key checks · u 0x1055ea; storm u 0x103c38 · u 0x103c52 ·
//!   u 0x103c6c · u 0x103d32 · u 0x103e80 · u 0x103ea4; composite u 0x1165c2 · u 0x11653c ·
//!   u 0x116490
//! - u 0x106666 creation · u 0x105876 name / sex · u 0x102e4a / u 0x102f66 input ·
//!   u 0x10a15e portrait · u 0x1030f8 buttons · u 0x109e44 · u 0x105700 objlist
//! - gypsy: u 0x105a04 · u 0x1059a8 · u 0x119944 · u 0x105aea · u 0x105c24 · u 0x106312 ·
//!   u 0x10632a · u 0x106386 · u 0x1064c0 · u 0x106554
//! - passages u 0x10ac08 · u 0x10f4da; text u 0x1140a2 · u 0x1142a6 · u 0x1142c6 · u 0x1142f0 ·
//!   u 0x115498 · u 0x1158ec · u 0x115a92 and its aligners; graphics u 0x1163fc · u 0x1199ba ·
//!   u 0x116180 · u 0x115588 · u 0x116326 · u 0x10f898; display u 0x113db2 · u 0x113df8 ·
//!   u 0x113e8e · u 0x113ea6 · u 0x113ebe · u 0x113fb2 · u 0x113fce · u 0x113fea · u 0x115078 ·
//!   u 0x1198f8 and the fades; input u 0x114f28 · u 0x113ed0 · u 0x113f0e · u 0x113f20;
//!   C runtime `rand` u 0x10fa6c / `srand` u 0x10fa8e
//! - u 0x1070c0 acknowledgments
//! - stubbed: u 0x1078e6 introduction story · u 0x10a81e transfer (Ultima IV / V saves) · the
//!   MED music and the VCR clock (seams)

pub mod create;
pub mod credits;
pub mod exe;
pub mod gate;
pub mod gypsy;
pub mod handoff;
pub mod input;
pub mod load;
pub mod mem;
pub mod opening;
pub mod passages;
pub mod screen;
pub mod seams;
pub mod state;
pub mod storm;
pub mod story;
#[cfg(test)]
mod tests;
pub mod text;
pub mod transfer;
pub mod video;

use std::cell::RefCell;

use u6_formats::font::WouFont;

use crate::engine::Io;
use crate::state::State;

use exe::UExe;
use mem::{Ptr, Region, View};
use screen::swap;
pub use state::IntroState;

/// How `u` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntroOutcome {
    /// Journey onward (0x10361a: exit code 1, the launcher runs `game`). `created` = a
    /// character was written and applied to the game state.
    Play { created: bool },
    /// The menu's quit key (^Q): `u` exits and the launcher stops.
    Quit,
    /// The user's files have no usable `u`; nothing was shown.
    Unavailable,
}

/// Runs `u` on the shared game state: the opening, the menu, character creation; a character
/// it writes is applied to `state` (see [`handoff::apply`]). The RNG of `u` is seeded with the
/// low word of the game RNG's state, the platform seed both programs take from `time()`.
pub async fn run(state: &RefCell<State>, io: &Io) -> IntroOutcome {
    let Some(exe) = io.files().get("u").and_then(|f| UExe::parse(f).ok()) else {
        return IntroOutcome::Unavailable;
    };
    let seed = state.borrow().rng.state() as u16;
    let mut st = IntroState::new(exe, io.files().clone());
    let outcome = main(&mut st, io, seed).await;
    if let (IntroOutcome::Play { .. }, Some(bytes)) = (&outcome, st.objlist.as_ref()) {
        let applied = handoff::apply(&mut state.borrow_mut(), bytes).is_ok();
        return IntroOutcome::Play { created: applied };
    }
    outcome
}

/// Original: u 0x1029d0 (main: setup, opening, menu loop) [partial]
///
/// Not ported: the `JJS` argument check (the launcher always passes it) and the argument copy.
/// Setup as 0x113bfe and main leave it: the screen view on buffer 1, the menu panel view, the
/// spare view, the text context (`u6.set`, ink 15, paper 0, justified), `blocks.new`.
pub async fn main(st: &mut IntroState, io: &Io, seed: u16) -> IntroOutcome {
    st.srand(seed);
    let screen = View::screen(Ptr::at(Region::Screen1));
    let v = &mut st.views;
    v.screen = screen;
    v.draw = screen;
    v.v17b4 = screen;
    v.text = screen;
    v.panel = View {
        x1: 0x12e,
        x0: 0x12,
        y0: 0x50,
        y1: 0xc5,
        ..screen
    };
    v.spare = View::screen(Ptr::at(Region::Spare));
    st.font = load::load_lzw(&st.files, b"u6.set").and_then(|d| WouFont::parse(&d).ok());
    st.text = text::TextCtx {
        view: st.views.text,
        ..text::TextCtx::default()
    };
    let blocks = st.blocks;
    load::load_into(&mut st.video.chip, &st.files, b"blocks.new", blocks);
    // 0x113bfe also turns the question buttons on (A4+0x182e = 1).
    st.buttons_on = 1;
    st.video.pointer = 0;
    st.menu_drawn = 0;
    st.tv = opening::Tv::new(st);
    opening::opening(st, io).await;
    loop {
        match menu(st, io).await {
            0 => {
                story::introduction(st, io).await;
                st.intro_seen = 1;
            }
            1 => {
                if create::create(st, io).await == 0 {
                    return journey(st, io).await;
                }
            }
            2 => {
                if transfer::transfer(st, io).await != 0 {
                    return journey(st, io).await;
                }
            }
            3 => credits::credits(st, io).await,
            4 => return journey_onward(st),
            _ => return IntroOutcome::Quit,
        }
    }
}

/// After a creation or transfer main shows the introduction once, then journeys on.
async fn journey(st: &mut IntroState, io: &Io) -> IntroOutcome {
    st.views.draw = st.views.screen;
    if st.intro_seen == 0 {
        story::introduction(st, io).await;
    }
    journey_onward(st)
}

/// Original: u 0x10361a (journey onward: check the `game` disk, clean up, exit with code 1)
///
/// The disk check (open `game` on volume 1, with the disk prompt) and the cleanup 0x113b68 are
/// the platform's.
pub fn journey_onward(st: &mut IntroState) -> IntroOutcome {
    st.views.passage = View {
        planes: Ptr::at(Region::Work),
        x1: 0x13f,
        x0: 0,
        y0: 0,
        y1: 0x37,
    };
    IntroOutcome::Play {
        created: st.objlist.is_some(),
    }
}

/// Original: u 0x1031b8 (main menu: title and menu pictures, then a key) — the choice: 0
/// introduction, 1 create, 2 transfer, 3 acknowledgments, 4 journey onward, 5 quit
///
/// I, C, T, A, J choose; Return takes the highlighted entry (the first, without a mouse);
/// the digits, `-` and `=` switch songs; ^Q quits. The extended codes H / K and M / P move the
/// highlight. The pictures are drawn only when the menu is not on the screen yet.
pub async fn menu(st: &mut IntroState, io: &Io) -> i16 {
    let menu_file = st.files_ptr;
    let title = st.files_ptr.add(0x9c40);
    load::load_into(&mut st.video.chip, &st.files, b"mainmenu.new", menu_file);
    load::load_into(&mut st.video.chip, &st.files, b"title.new", title);
    if st.song != 1 {
        seams::play_song(st, 1);
    }
    st.video.set_palette(&st.exe, 5);
    if st.menu_drawn == 0 {
        let at = |v: View, x1, x0, y0, y1| View {
            x1,
            x0,
            y0,
            y1,
            ..v
        };
        let v = at(st.views.draw, 0x13f, 0, 0, 0x3f);
        mem::draw_item(&mut st.video.chip, &v, 0x13, 0, title, 0);
        swap(st, io).await;
        let (d, s) = (st.views.draw, st.views.screen);
        mem::copy_view(&mut st.video.chip, &d, &s);
        st.views.draw = st.views.screen;
        let v = at(st.views.draw, 0x108, 0x3b, 0x2f, 0x4b);
        mem::draw_item(&mut st.video.chip, &v, 0x3b, 0x2f, title, 1);
        let v = at(st.views.draw, 0x12b, 0x31, 0x53, 0xc4);
        mem::draw_item(&mut st.video.chip, &v, 0x31, 0x53, menu_file, 0);
        swap(st, io).await;
        st.menu_drawn = 1;
    }
    st.skip = 0;
    st.views.draw.planes = st.video.other();
    st.video.pointer = 1;
    let mut choice: i16 = 0;
    loop {
        if !st.keys.ready(io) {
            st.video.wait_frame(io).await;
            continue;
        }
        let c = input::to_upper(st.keys.take(io));
        let song = |n| Some(n);
        let mut pick = None;
        let play = match c {
            0 => {
                match st.keys.take(io) {
                    0x48 | 0x4b => choice = (choice - 1).max(0),
                    0x4d | 0x50 => choice = (choice + 1).min(4),
                    _ => {}
                }
                None
            }
            0x0d => {
                pick = Some(choice);
                None
            }
            0x11 => {
                pick = Some(5);
                None
            }
            0x2d => song(0xb),
            0x30 => song(0xa),
            0x31..=0x39 => song(c - 0x30),
            0x3d => song(0xc),
            0x41 => {
                pick = Some(3);
                None
            }
            0x43 => {
                pick = Some(1);
                None
            }
            0x49 => {
                pick = Some(0);
                None
            }
            0x4a => {
                pick = Some(4);
                None
            }
            0x54 => {
                pick = Some(2);
                None
            }
            _ => None,
        };
        if let Some(n) = play {
            seams::play_song(st, n);
            if n == 2 {
                seams::start_music(st);
            }
        }
        if let Some(p) = pick {
            choice = p;
            break;
        }
    }
    st.video.pointer = 0;
    if choice != 3 {
        seams::play_song(st, 0);
    }
    choice
}

/// The intro's own context to drive in tests and tools without a game state: `u` from
/// `files`, the RNG seeded with `seed`.
pub fn state_for(files: &crate::Files) -> Option<IntroState> {
    let exe = UExe::parse(files.get("u")?).ok()?;
    Some(IntroState::new(exe, files.clone()))
}
