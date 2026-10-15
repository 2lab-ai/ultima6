//! The acknowledgments of `u` (0x1070c0): four pages of credits on the vellum in the menu
//! panel, each shown until a key.

use crate::engine::Io;

use super::mem::{self, Ptr, Region, View};
use super::screen::{swap, wait_key};
use super::state::IntroState;
use super::text::{self, Arg};

/// One step of the credits: a `printf` restarting the buffer (`New`) or appending (`More`) of
/// the string at an image address of `u`, with the ink colours of its `%F` codes; a page turn.
#[derive(Clone, Copy)]
enum Step {
    New(u32, &'static [u16]),
    More(u32),
    /// Swap the page in, copy it to the other buffer, clear the text box there.
    Turn,
    /// Wait for a key, then [`Step::Turn`] without the copy.
    KeyTurn,
}

/// The `%F` colours of every heading (title ink 0xc, names ink 0x3d).
const HEAD: &[u16] = &[0xc, 0x3d];

/// 0x10728e..0x107418 as data: the string addresses and the page breaks of the original's
/// calls (the strings themselves are read from `u`).
const PAGES: &[Step] = &[
    Step::New(0x0010_7484, HEAD),
    Step::New(0x0010_7498, &[]),
    Step::New(0x0010_74c0, HEAD),
    Step::New(0x0010_74da, &[]),
    Step::New(0x0010_74ec, HEAD),
    Step::New(0x0010_7500, &[]),
    Step::More(0x0010_751c),
    Step::Turn,
    Step::New(0x0010_753a, HEAD),
    Step::New(0x0010_754c, &[]),
    Step::More(0x0010_7576),
    Step::More(0x0010_7598),
    Step::More(0x0010_75b6),
    Step::KeyTurn,
    Step::New(0x0010_75d8, HEAD),
    Step::New(0x0010_75e4, &[]),
    Step::More(0x0010_7608),
    Step::More(0x0010_7620),
    Step::New(0x0010_7642, HEAD),
    Step::New(0x0010_7650, &[]),
    Step::More(0x0010_766e),
    Step::More(0x0010_768c),
    Step::KeyTurn,
    Step::New(0x0010_76aa, HEAD),
    Step::New(0x0010_76c4, &[]),
    Step::More(0x0010_76e0),
    Step::New(0x0010_7702, HEAD),
    Step::New(0x0010_771c, &[]),
    Step::More(0x0010_7740),
    Step::More(0x0010_7760),
    Step::More(0x0010_7780),
];

/// The heading of the first page (the title, then the line naming its maker).
const TITLE: u32 = 0x0010_745e;

fn print(st: &mut IntroState, fresh: bool, addr: u32, args: &[u16]) {
    let fmt = st.exe.cstr(addr);
    let args: Vec<Arg> = args.iter().map(|&w| Arg::Word(w)).collect();
    if let Some(font) = st.font.as_ref() {
        let (chip, ctx) = (&mut st.video.chip, &mut st.text);
        if fresh {
            text::print_new(chip, ctx, font, &fmt, &args);
        } else {
            text::print_more(chip, ctx, font, &fmt, &args);
        }
    }
}

/// The text box on the drawing buffer, cleared, the cursor at its corner.
fn clear_box(st: &mut IntroState, mut b: View) {
    b.planes = st.views.screen.planes;
    st.text.view = b;
    if let Some(font) = st.font.as_ref() {
        text::clear_view(&mut st.video.chip, &mut st.text, font);
    }
}

/// Original: u 0x1070c0 (acknowledgments: the menu panel saved, the vellum, the title, then the
/// four credit pages; the panel restored at the end)
pub async fn credits(st: &mut IntroState, io: &Io) {
    let (d, s) = (st.views.draw, st.views.screen);
    mem::copy_view(&mut st.video.chip, &d, &s);
    st.views.draw = st.views.screen;
    let rect = |v: View| View {
        x1: 0x124,
        x0: 0x1c,
        y0: 0x59,
        y1: 0xbd,
        ..v
    };
    let mut title_box = rect(st.views.draw);
    st.views.v17f4 = View {
        planes: st.views.draw.planes,
        ..st.views.panel
    };
    let v = st.views.v17f4;
    mem::fill_view(&mut st.video.chip, &v, 0);
    let p = st.views.panel;
    let saved = View {
        planes: Ptr::at(Region::Files),
        x1: p.x1 - p.x0,
        x0: 0,
        y0: 0,
        y1: p.y1 - p.y0,
    };
    let vellum = st.files_ptr.add(0x9c40);
    super::load::load_into(&mut st.video.chip, &st.files, b"vellum1.new", vellum);
    st.views.panel.planes = st.video.other();
    let panel = st.views.panel;
    mem::copy_view(&mut st.video.chip, &panel, &saved);
    mem::fill_view(&mut st.video.chip, &panel, 0);
    st.views.v17f4.planes = st.video.current();
    let v = st.views.v17f4;
    mem::draw_item(&mut st.video.chip, &v, v.x0, v.y0, vellum, 0);
    st.text.view = title_box;
    st.text.fg = 0;
    st.text.bg = 0x1e;
    st.text.x = title_box.x0;
    st.text.y = title_box.y0;
    st.text.align = 2;
    print(st, true, TITLE, &[]);
    // The cursor is wherever the title string left it; word sums (`add.w` / `addq.w` at
    // 0x10722a..0x10728a).
    let y = st.text.y;
    let rule = View {
        y0: y.wrapping_add(9),
        y1: y.wrapping_add(9),
        ..title_box
    };
    mem::fill_view(&mut st.video.chip, &rule, 0);
    title_box.y0 = y.wrapping_add(0xe);
    st.views.draw.planes = st.views.screen.planes;
    clear_box(st, title_box);
    st.text.y = st.text.y.wrapping_add(1);
    for step in PAGES {
        match *step {
            Step::New(a, args) => print(st, true, a, args),
            Step::More(a) => print(st, false, a, &[]),
            Step::Turn => {
                swap(st, io).await;
                let (d, s) = (st.views.draw, st.views.screen);
                mem::copy_view(&mut st.video.chip, &d, &s);
                clear_box(st, title_box);
            }
            Step::KeyTurn => {
                wait_key(st, io).await;
                swap(st, io).await;
                clear_box(st, title_box);
            }
        }
    }
    wait_key(st, io).await;
    st.views.panel.planes = st.views.screen.planes;
    swap(st, io).await;
    wait_key(st, io).await;
    let panel = st.views.panel;
    mem::fill_view(&mut st.video.chip, &panel, 0);
    mem::copy_view(&mut st.video.chip, &saved, &panel);
}
