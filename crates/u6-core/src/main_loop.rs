//! Boot (`main()` 0x1351d8 after the platform setup) and the command loop 0x1041d8 (WU-P7).
//!
//! `run` is the game future the engine polls: the rest of main (text windows, `paper.bmp`, the
//! command bar, the tail of load state 0x105350, the character check, the first clock tick), then
//! the command loop with its dispatcher ([`commands`]). The helpers of the main hunk the loop and
//! the other modules call are in [`main_hunk`].
//!
//! Character check: main tests A4-0x7b03 (byte 0x81 of the objlist block at A4-0x7b84, objlist
//! 0x1C72, the Avatar's portrait, 1-based) and, when it is 0, prints "You must first create or
//! transfer a character.", waits for a key and exits. The shipped `DATA1:objlist` holds 0 there:
//! character creation is not in `game`. The port starts from the shipped disk state the way
//! Nuvie's new-game path does (formats.md §7: it patches name, stats, gender and the portrait
//! byte from the creation answers): without answers, the disk's name and stats stay and the
//! portrait byte becomes 1 (`gender·6 + portrait + 1` with both 0). The check is a stub point
//! ("0x1351d8 character creation"). The rest of the disk state is played as it is, combat-mode
//! byte included (A4-0x7b0c = 1 in the shipped objlist, so main calls 0x121c64, a stub).
//!
//! Synchronous game code that prints (actors, map, objects call printf 0x108658, which waits on
//! a more prompt in the port's async ui) queues its lines in [`MainState::pending_print`]; the
//! loop flushes them at its next await point, in order ([`flush_prints`]).
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x101ff6 [main] leaf; callers 2, from 2 subsystems
//! - 0x102bf2 [main] step of 0x102c90 (present/refresh map window [likel~)
//! - 0x102f2a [main] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x1032b6 [main] unnamed; calls 0x102f3c (per-turn party status effects~)
//! - 0x10348c [main] step of 0x1041d8 (command loop + A-Z key dispatcher~)
//! - 0x10355c [main] unnamed; calls 0x109e74 (view-window flags (blocking/l~)
//! - 0x103632 [main] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x103fd4 [main] unnamed; calls 0x108f90 (rebuild the map-window tile c~)
//! - 0x104094 [main] unnamed; 2 callees
//! - 0x104140 [main] step of 0x1041d8 (command loop + A-Z key dispatcher~)
//! - 0x1041d8 [main] command loop + A-Z key dispatcher, end-of-turn
//! - 0x1350d8 [boot] step of 0x1351d8 (main(): setup, load data, charact~)
//! - 0x135118 [boot] step of 0x1041d8 (command loop + A-Z key dispatcher~)
//! - 0x13511a [boot] step of 0x1351d8 (main(): setup, load data, charact~)
//! - 0x135120 [boot] unnamed; 1 callees
//! - 0x1351d8 [boot] main(): setup, load data, character check, music, command loop

pub mod commands;
pub mod main_hunk;
#[cfg(test)]
mod receipts;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use u6_formats::bitmap::Bitmap;

pub use main_hunk::*;

use crate::engine::Io;
use crate::image::A4;
use crate::state::State;
use crate::todo_stub;
use crate::ui::printf::{Arg, printf};

/// One queued `printf` argument (owned: the queue outlives the caller's borrows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintArg {
    Word(u16),
    Str(Vec<u8>),
}

/// One queued text-layer call of synchronous code, in call order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    /// printf 0x108658 of a format with its arguments.
    Print(Vec<u8>, Vec<PrintArg>),
    /// 0x107dd8: make a window current.
    Window(crate::ui::WindowId),
    /// 0x107e00: move the current window's cursor to (col, row).
    Cursor(i16, i16),
}

/// Globals of the main hunk the command loop and boot own (the rest of the loop's globals live
/// with the module that reads them most: `ui.input`, `ui.panel`, `actors.work`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MainState {
    /// Port-only: text-layer calls queued by synchronous code, flushed in order by the loop.
    pub pending_print: Vec<Pending>,
    /// Original: A4-0x7a2c (word: 1 while the loop waits for a command key outside a target)
    pub reading: u16,
    /// Original: A4-0x618e (byte: overlay frame counter 0x135120 steps)
    pub overlay_counter: u8,
    /// Original: A4-0x7a4a (word: the last attack target, -1 = none; the cross-hair of Attack
    /// starts on it)
    pub attack_target: i16,
    /// Original: A4+0x64ce (word: main sets it to 1 before load state; readers not identified)
    pub flag_64ce: u16,
}

/// Queues a `printf` of `fmt` for the loop to print (synchronous callers).
pub fn queue_print(s: &mut State, fmt: Vec<u8>, args: Vec<PrintArg>) {
    s.main.pending_print.push(Pending::Print(fmt, args));
}

/// Runs the queued text-layer calls (0x108658, 0x107dd8, 0x107e00), oldest first.
pub async fn flush_prints(state: &RefCell<State>, io: &Io) {
    loop {
        let next = {
            let mut s = state.borrow_mut();
            let q = &mut s.main.pending_print;
            (!q.is_empty()).then(|| q.remove(0))
        };
        match next {
            None => return,
            Some(Pending::Window(id)) => crate::ui::panels::select_window(state, io, id).await,
            Some(Pending::Cursor(col, row)) => {
                crate::ui::panels::set_cursor(state, io, col, row).await
            }
            Some(Pending::Print(fmt, args)) => {
                let args: Vec<Arg<'_>> = args
                    .iter()
                    .map(|a| match a {
                        PrintArg::Word(w) => Arg::Word(*w),
                        PrintArg::Str(s) => Arg::Str(s),
                    })
                    .collect();
                printf(state, io, &fmt, &args).await;
            }
        }
    }
}

/// The NUL-terminated string at image address `addr` of the user's `game` (empty when the file
/// or the address is missing: a message the port cannot read is not printed).
pub fn game_string(io: &Io, addr: u32) -> Vec<u8> {
    io.game_image()
        .and_then(|img| img.cstr(addr).ok())
        .unwrap_or_default()
}

/// [`game_string`] at `A4 - off`.
pub fn a4_string(io: &Io, off: u32) -> Vec<u8> {
    game_string(io, A4.wrapping_sub(off))
}

/// The string the pointer at image address `at` points to.
pub fn pointed_string(io: &Io, at: u32) -> Vec<u8> {
    let Some(img) = io.game_image() else {
        return Vec::new();
    };
    img.pointer(at)
        .and_then(|p| img.cstr(p))
        .unwrap_or_default()
}

/// `printf` of the string at `A4 - off` with `args`.
pub(crate) async fn say(state: &RefCell<State>, io: &Io, off: u32, args: &[Arg<'_>]) {
    let fmt = a4_string(io, off);
    printf(state, io, &fmt, args).await;
}

/// The roster name of the party leader (A4+0x470c + 14 × A4-0x7b7a).
pub(crate) fn leader_name(s: &State) -> Vec<u8> {
    let p = &s.actors.party;
    p.names
        .get(usize::from(p.leader_index))
        .map_or_else(Vec::new, |n| n.as_bytes().to_vec())
}

/// Image address of "You must first create or transfer a character." (main pushes it).
const MSG_CREATE_CHARACTER: u32 = 0x0013_50a6;
/// Objlist block bytes of A4-0x7b03 (a character exists) and A4-0x7b0c (combat mode).
const TAIL_CREATED: usize = 0x81;
const TAIL_COMBAT: usize = 0x78;

/// Original: 0x1351d8 (main(): setup, load data, character check, music, command loop) [partial]
///
/// The engine did the platform setup (0x14ef06), the RNG seed (0x151f2e), the tile graphics
/// (0x152e50), the font, the tables and the reads of load state 0x105350 (`State::new_game`).
/// This runs the rest in the original order: the screen window 0..39 × 0..24, `paper.bmp`
/// (0x1350d8), the status and message windows (the message scroll current, wrapping,
/// scrolling, capitalising), the slot reset 0x10ed44, the command bar (tiles 0x190..0x198 at
/// x = 8 + 16 i, y = 0xb0, then 0x19e at (0x98, 0xb0)), A4+0x64ce = 1, the tail of 0x105350,
/// one tile substitution per animdata slot (0x152cd2), the character check, then 0x1217d4,
/// the clock 0x10368a(0), combat or the area song, the command loop, music off and the `.tmp`
/// cleanup 0x10d4e8. Not ported: the argument check ("JJS"), the copper and display setup
/// (the engine's).
pub async fn run(state: Rc<RefCell<State>>, io: Rc<Io>) {
    let (state, io) = (&*state, &*io);
    boot_screen(state, io);
    {
        let mut s = state.borrow_mut();
        s.main.flag_64ce = 1;
        finish_load_state(&mut s, io);
        let anim = s.tables.anim.clone();
        for slot in anim.slots.iter().take(usize::from(anim.count)) {
            crate::gfx::anim::substitute(&mut s.gfx, slot.tile_to_animate, slot.first_anim_frame);
        }
    }
    let created = state
        .borrow()
        .actors
        .objlist_tail
        .get(TAIL_CREATED)
        .copied();
    if created.unwrap_or(0) == 0 {
        // Contract: the original prints MSG_CREATE_CHARACTER, waits for a key and exits; the
        // character is made by another program. Fallback: play the shipped disk state.
        let exit = todo_stub!("0x1351d8 character creation (another executable)", false);
        state.borrow_mut().actors.objlist_tail[TAIL_CREATED] = 1;
        if exit {
            let msg = game_string(io, MSG_CREATE_CHARACTER);
            printf(state, io, &msg, &[]).await;
            state.borrow_mut().ui.input.idle_refresh = false;
            crate::ui::wait::read_key(state, io).await;
            return;
        }
    }
    {
        let mut s = state.borrow_mut();
        crate::actors::turn::after_load(&mut s);
        crate::actors::clock::tick(&mut s, 0);
        if s.actors.objlist_tail.get(TAIL_COMBAT).copied().unwrap_or(0) != 0 {
            // 0x121c64, the B handler (the shipped objlist has the combat byte set).
            crate::combat::mode::begin_combat(&mut s);
        } else {
            crate::sound::play_area_song(&mut s);
        }
    }
    flush_prints(state, io).await;
    commands::command_loop(state, io).await;
    crate::sound::play_song(&mut state.borrow_mut(), 0);
    crate::objects::delete_tmp_files(&mut state.borrow_mut().objects);
}

/// The part of 0x1351d8 between the font load and load state, on the screen.
fn boot_screen(state: &RefCell<State>, io: &Io) {
    boot_hook();
    let mut s = state.borrow_mut();
    let tables = s.ui.tables.clone();
    s.ui.text
        .open(&tables, crate::ui::WindowId::Screen, 0, 0, 0x27, 0x18);
    io.draw(|fb| {
        for (dst, c) in fb.palette.iter_mut().zip(&s.tables.palette) {
            *dst = u6_render::Rgb::new(c.r, c.g, c.b);
        }
    });
    draw_paper(&s, io);
    s.ui.text.main_windows(&tables);
    crate::objects::init_slots(&mut s.objects);
    let st = &*s;
    io.draw(|fb| {
        for i in 0..9u16 {
            crate::gfx::blit::blit_front(&st.gfx, fb, 0x190 + i, i * 16 + 8, 0xb0);
        }
        crate::gfx::blit::blit_front(&st.gfx, fb, 0x19e, 0x98, 0xb0);
    });
    io.present();
}

/// Original: 0x1350d8 (boot: read `paper.bmp` and draw it over the whole screen)
///
/// The file name is the pointer at A4-0x61e2; 0x1346a0 reads and LZW-decodes it, 0x153384 draws
/// it at (0, 0) through the bitmap colour map. A missing or bad file draws nothing, as the
/// original skips the draw when the open fails.
pub fn draw_paper(s: &State, io: &Io) {
    let name = pointed_string(io, A4.wrapping_sub(0x61e2));
    let name = String::from_utf8_lossy(&name).into_owned();
    let Some(bmp) = io.files().get(&name).and_then(|f| Bitmap::from_lzw(f).ok()) else {
        return;
    };
    let colours = &s.gfx.tables.bitmap_colours;
    io.draw(|fb| crate::gfx::blit::blit_bmp(colours, fb, &bmp, 0, 0));
}

/// The calls 0x105350 makes after its reads, in its order: link the actors (0x10edb4, after the
/// save mode from the data hunk), underline the last command (0x106506 with A4-0x7b09), the
/// object blocks of the region (0x10e1ce), the map region (0x108ac4, the port holds the whole
/// map), A4-0x784e = 1, the window cache (0x108f90), each member into the leader's inventory
/// when the party is aboard (0x10cda8 with place 0x10, only when the leader index is the party
/// size), the view flags (0x109e74).
pub fn finish_load_state(s: &mut State, io: &Io) {
    s.objects.save_mode = s.objects.defs.save_mode;
    crate::objects::link_actors(&mut s.objects);
    let last = crate::ui::globals::last_command(&s.actors);
    crate::ui::cursor::select_command(s, io, last);
    region_steps(s);
}

/// The state part of the load-state tail without a screen (tools, tests): the slot reset main
/// does first (0x10ed44), then [`finish_load_state`] less the command-bar underline.
pub fn load_state_tail(s: &mut State) {
    crate::objects::init_slots(&mut s.objects);
    s.objects.save_mode = s.objects.defs.save_mode;
    crate::objects::link_actors(&mut s.objects);
    region_steps(s);
}

/// 0x105350 from the region load on (0x10e1ce .. 0x109e74).
fn region_steps(s: &mut State) {
    let c = s.map.centre;
    crate::objects::load_region(s, c.x, c.y, u16::from(c.z));
    s.map.load_region(c.x, c.y, u16::from(c.z));
    s.map.flag_784e = 1;
    crate::map::region::refresh_region(s);
    let party = s.actors.party.clone();
    if party.leader_index == party.size {
        for &m in party.members.iter().take(usize::from(party.size)) {
            crate::objects::move_into(s, i16::from(m), 0, 0x10);
        }
    }
    crate::map::view::update_view(s);
}
