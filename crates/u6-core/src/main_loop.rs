//! The launcher, boot (`main()` 0x1351d8 after the platform setup) and the command loop
//! 0x1041d8 (WU-P7).
//!
//! [`run`] is the engine task: the launcher's program sequence (`u`, then `game` while `u`
//! journeys on). `game` is the rest of main (text windows, `paper.bmp`, the command bar, the
//! tail of load state 0x105350, the character check, the first clock tick), then the command
//! loop with its dispatcher ([`commands`]). The helpers of the main hunk the loop and the other
//! modules call are in [`main_hunk`].
//!
//! Character check: main tests A4-0x7b03 (byte 0x81 of the objlist block at A4-0x7b84, objlist
//! 0x1C72, the hero's portrait, 1-based) and, when it is 0, prints the message that a
//! character must be created first (0x1350a6), waits for a key and exits with code 0, which
//! makes the launcher run `u` again. The shipped `DATA1:objlist` holds 0 there: the character
//! is made by `u` ([`crate::intro`]), which writes it into `savegame/objlist`. Started without
//! the launcher ([`game_first`]: no usable `u` in the user's files, the platforms'
//! development entry), the port plays the shipped disk state: the disk's name and stats stay
//! and the portrait byte becomes 1, as on the oracle's game-first disks (which also set the sex
//! byte 0x1C71 to 1, female in `u`'s encoding; the port leaves it 0). The rest of the disk
//! state is played as it is, combat-mode byte included (A4-0x7b0c = 1 in the shipped objlist,
//! so main calls 0x121c64).
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
#[cfg(test)]
mod hd_receipt;
pub mod main_hunk;
#[cfg(test)]
mod plane_receipt;
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
use crate::ui::printf::{Arg, printf, printf_swallowed};

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
    /// Port (spec 08, 2026-10-24): [`Pending::Print`] of a combat line queued while Attack's
    /// newline is held (A4-0x7a48, [`crate::combat::CombatState::swallow`]): the format whole,
    /// as the user's `game` holds it, so the locale layer finds it; the leading newlines of the
    /// text it puts are dropped then ([`crate::ui::printf::printf_swallowed`]).
    PrintSwallowed(Vec<u8>, Vec<PrintArg>),
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
    /// Port-only: the program of the launcher's sequence that runs (for the platforms).
    pub program: Program,
}

/// The programs the launcher runs one after the other ([`run`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Program {
    /// `u`: the opening, the main menu, character creation ([`crate::intro`]).
    #[default]
    U,
    /// `game`: main 0x1351d8 and the command loop.
    Game,
}

/// Queues a `printf` of `fmt` for the loop to print (synchronous callers).
///
/// Printf 0x108658 tests the text switch (A4-0x7a4c, `ui.text.on`) when it is called (asm
/// 0x108660 `tst.w`, 0x108664 `beq.w` to its return at 0x10887e): a line queued with the switch
/// off is dropped, one queued with it on is printed with it on ([`flush_prints`]).
pub fn queue_print(s: &mut State, fmt: Vec<u8>, args: Vec<PrintArg>) {
    if s.ui.text.on {
        s.main.pending_print.push(Pending::Print(fmt, args));
    }
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
            Some(Pending::Print(fmt, args)) => print_queued(state, io, &fmt, &args, false).await,
            Some(Pending::PrintSwallowed(fmt, args)) => {
                print_queued(state, io, &fmt, &args, true).await
            }
        }
        crate::combat::present::printed_one(&mut state.borrow_mut());
    }
}

/// Port-only: a queued line, printed with the text switch on as it was when queued (a line
/// queued with it off never reaches the queue: [`queue_print`],
/// [`crate::combat::present::say`]; a hit inside text-off code forces it on, 0x1231be), the
/// switch put back after.
async fn print_queued(
    state: &RefCell<State>,
    io: &Io,
    fmt: &[u8],
    args: &[PrintArg],
    swallowed: bool,
) {
    let was = std::mem::replace(&mut state.borrow_mut().ui.text.on, true);
    if swallowed {
        printf_swallowed(state, io, fmt, &print_args(args)).await;
    } else {
        printf(state, io, fmt, &print_args(args)).await;
    }
    state.borrow_mut().ui.text.on = was;
}

/// The queued arguments as `printf` takes them.
fn print_args(args: &[PrintArg]) -> Vec<Arg<'_>> {
    args.iter()
        .map(|a| match a {
            PrintArg::Word(w) => Arg::Word(*w),
            PrintArg::Str(s) => Arg::Str(s),
        })
        .collect()
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

/// Image address of the message that no character exists yet (main pushes it).
const MSG_CREATE_CHARACTER: u32 = 0x0013_50a6;
/// Objlist block bytes of A4-0x7b03 (a character exists) and A4-0x7b0c (combat mode).
const TAIL_CREATED: usize = 0x81;
const TAIL_COMBAT: usize = 0x78;
/// Exit codes of `game` (the argument of 0x14f15a), read by the launcher as its next state:
/// 0 runs `u` (no character, 0x13546e), 3 ends the launcher (the end of main, 0x1354a4).
const EXIT_RUN_U: u16 = 0;
const EXIT_QUIT: u16 = 3;

/// Original: ultcli 0x100360 (the launcher `PROGRAM:ultima6`: run `u`, then `game` while `u`
/// journeys on) [partial]
///
/// A state machine on the exit code of the program it ran last (D7, the jump table at ultcli
/// 0x100452): state 0 runs `u JJS` (0x10045c), 1 checks for a PAL display and runs `game JJS`
/// with three buffer addresses (0x100492), 2 runs `end` and stops (0x1004e6), 3 stops
/// (0x10052c), 4 stops with a requester (0x100532); each run waits for the program and takes
/// its return code as the next state. So `u`'s Journey Onward (exit 1) starts `game`, its quit
/// (exit 3) ends the launcher, and `game` without a character (exit 0) runs `u` again; the end
/// of `game` (exit 3) ends it. Every `game` is a new process, the first one included: the
/// host's player starts over at each hand-over from `u` ([`crate::sound::MedCall::NewProcess`]),
/// and every `game` after the first also gets a new State ([`relaunch`]; the first one's is the
/// State the engine booted, which `u` gave its character). Not ported: the PAL check (the port
/// has no NTSC display), `end` (exit 2; no path of the port reaches the end of the game) and the
/// requesters. With no usable `u` in the user's files the port starts `game` at once, as
/// [`game_first`] does: no program ran before it, so the player the host built (the audio part
/// of setup 0x14ef06, [`crate::sound::AudioDriver::new`]) is already `game`'s and nothing is
/// marked.
pub async fn run(state: Rc<RefCell<State>>, io: Rc<Io>) {
    let (state, io) = (&*state, &*io);
    let mut launched = false;
    loop {
        state.borrow_mut().main.program = Program::U;
        match crate::intro::run(state, io).await {
            crate::intro::IntroOutcome::Quit => return,
            crate::intro::IntroOutcome::Unavailable => {
                state.borrow_mut().main.program = Program::Game;
                game_main(state, io, false).await;
                return;
            }
            crate::intro::IntroOutcome::Play { .. } => {}
        }
        if launched {
            relaunch(state, io);
        }
        launched = true;
        state
            .borrow_mut()
            .sound
            .call(crate::sound::MedCall::NewProcess);
        state.borrow_mut().main.program = Program::Game;
        if game_main(state, io, true).await != EXIT_RUN_U {
            return;
        }
    }
}

/// The engine task without the launcher: `game` main at once (the platforms' development entry,
/// which the oracle's game-first disks match; tests). A save without a character plays the
/// shipped disk state (see the module doc).
pub async fn game_first(state: Rc<RefCell<State>>, io: Rc<Io>) {
    let (state, io) = (&*state, &*io);
    state.borrow_mut().main.program = Program::Game;
    game_main(state, io, false).await;
}

/// A new `game` process after the launcher ran `u` again: what the boot reads
/// ([`State::new_game`] over the user's files), with the `savegame/objlist` this run wrote
/// (`u`'s character, [`crate::saveload::SaveLoad::objlist`]) read in place of the files' one.
/// The RNG goes on (the port has no clock to seed it from again); so does the sound log
/// ([`crate::sound::SoundState::continue_calls`]), which [`run`] then marks with the change of
/// process for the host's player, as at every hand-over. No Save can come before a relaunch (a
/// Save is a command, and the end of the command loop ends the launcher), so the block files
/// are the user's. Unchanged when the files no longer load.
fn relaunch(state: &RefCell<State>, io: &Io) {
    let Ok(fresh) = State::new_game(io.files()) else {
        return;
    };
    take_over(&mut state.borrow_mut(), fresh, io);
}

/// The new process `fresh` takes the place of `s` ([`relaunch`]): the RNG, the save state, the
/// sound log and the locale layer go on, the character `u` made is read in. Its records'
/// versions start again at 0 while the screen buffers keep what the old records drew, so every
/// source plane is reset to classic, the colours untouched ([`Io::reset_src_plane`]): no old
/// pixel can match a new version (spec 07 HD0).
pub(crate) fn take_over(s: &mut State, mut fresh: State, io: &Io) {
    fresh.rng = s.rng;
    fresh.saveload = std::mem::take(&mut s.saveload);
    fresh.sound.continue_calls(&mut s.sound);
    // Port-only (spec 08): the locale pack the platform installed stays with the game.
    fresh.locale = std::mem::take(&mut s.locale);
    crate::locale::apply(&mut fresh);
    if let Some(objlist) = fresh.saveload.objlist.clone() {
        crate::saveload::objlist::decode(&mut fresh, &objlist);
        fresh.place_party();
    }
    *s = fresh;
    io.reset_src_plane();
}

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
/// cleanup 0x10d4e8. Returns the exit code (0x14f15a). Without a character (0x13544a): the
/// message (0x135452), A4-0x7a56 = 0, a key (0x10790a), the disk check for the PROGRAM disk
/// `u` runs from (0x1349a0 with D0 = 0, the platform's), exit 0. `launched` = started by the
/// launcher; otherwise a save without a character plays the shipped disk state (module doc).
/// Not ported: the argument check (`JJS` and the launcher's three buffer addresses), the copper
/// and display setup (the engine's).
async fn game_main(state: &RefCell<State>, io: &Io, launched: bool) -> u16 {
    boot_screen(state, io);
    {
        let mut s = state.borrow_mut();
        s.main.flag_64ce = 1;
        // A4-0x637a starts at its data-hunk value (Restore sets play before load state).
        s.objects.save_mode = s.objects.defs.save_mode;
        // Port (spec 18): load state's read of the remembered sight saved with this save.
        crate::saveload::boot_sight(&mut s, io.files());
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
        if launched {
            let msg = game_string(io, MSG_CREATE_CHARACTER);
            printf(state, io, &msg, &[]).await;
            state.borrow_mut().ui.input.idle_refresh = false;
            crate::ui::wait::read_key(state, io).await;
            return EXIT_RUN_U;
        }
        // [replaced] No program made a character: the portrait byte of the oracle's
        // game-first disks (module doc).
        state.borrow_mut().actors.objlist_tail[TAIL_CREATED] = 1;
    }
    {
        let mut s = state.borrow_mut();
        crate::actors::turn::after_load(&mut s);
        crate::actors::clock::tick(&mut s, 0);
        crate::ui::pointer::show(&mut s);
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
    EXIT_QUIT
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
    draw_paper(&mut s, io);
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
/// original skips the draw when the open fails. The source plane names it the paper, and the
/// state keeps the colours it drew ([`crate::ui::UiState::paper`]) for the text windows.
pub fn draw_paper(s: &mut State, io: &Io) {
    let Some(bmp) = paper_bitmap(io) else {
        return;
    };
    let colours = &s.gfx.tables.bitmap_colours;
    let role = u6_render::BITMAP_PAPER;
    io.draw(|fb| crate::gfx::blit::blit_bmp(colours, fb, &bmp, 0, 0, role));
    s.ui.paper = Some(crate::ui::text::Paper::drawn(colours, &bmp));
}

/// The bitmap [`draw_paper`] draws, decoded; `None` for a missing or bad file.
pub(crate) fn paper_bitmap(io: &Io) -> Option<Bitmap> {
    io.files()
        .get(&paper_file(io))
        .and_then(|f| Bitmap::from_lzw(f).ok())
}

/// The name of the file [`paper_bitmap`] reads (the pointer at A4-0x61e2).
pub(crate) fn paper_file(io: &Io) -> String {
    String::from_utf8_lossy(&pointed_string(io, A4.wrapping_sub(0x61e2))).into_owned()
}

/// The calls 0x105350 makes after its reads, in its order: link the actors (0x10edb4),
/// underline the last command (0x106506 with A4-0x7b09), the object blocks of the region
/// (0x10e1ce), the map region (0x108ac4, the port holds the whole map), A4-0x784e = 1, the
/// window cache (0x108f90), each member into the leader's inventory when the party is aboard
/// (0x10cda8 with place 0x10, only when the leader index is the party size), the view flags
/// (0x109e74). The save mode (A4-0x637a), which the block reads check, is left as it is: 0x105350
/// does not write it; the boot starts it at its data-hunk value and Restore at play.
pub fn finish_load_state(s: &mut State, io: &Io) {
    crate::objects::link_actors(&mut s.objects);
    let last = crate::ui::globals::last_command(&s.actors);
    crate::ui::cursor::select_command(s, io, last);
    region_steps(s);
}

/// The state part of the boot's load-state tail without a screen (tools, tests): the slot
/// reset main does first (0x10ed44), the save mode at its data-hunk value, then
/// [`finish_load_state`] less the command-bar underline.
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

#[cfg(test)]
mod queue_tests {
    //! The print queue and the text switch A4-0x7a4c (`ui.text.on`), which 0x108658 tests when
    //! it is called (asm 0x108660 `tst.w -0x7a4c(a4)`, 0x108664 `beq.w` to its return at
    //! 0x10887e).

    use std::path::Path;

    use super::*;
    use crate::engine::Engine;
    use crate::files::Files;
    use u6_testkit::Quiet;

    const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
    /// A vine root (type 0x16e) and the vine actor its quality's high byte names.
    const ROOT: usize = 0x200;
    const VINE: usize = 10;

    /// The user's files (skipped when the archive is absent; reads it, copies nothing).
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn files() -> Option<Files> {
        if !Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return None;
        }
        let data = std::fs::read(ZIP).quiet();
        let r = u6_intake::intake(vec![u6_intake::InputFile {
            name: "ultima-vi.zip".into(),
            data,
        }]);
        Some(Files::new(r.files))
    }

    /// `queue` run on a state with the test text layer inside an engine on `files`, then the
    /// queue flushed: the state after.
    fn queue_and_flush(files: Files, setup: fn(&mut State), queue: fn(&mut State)) -> State {
        run(files, setup, queue, true)
    }

    /// [`queue_and_flush`] without the flush: the state with what `queue` left queued.
    fn queued(files: Files, setup: fn(&mut State), queue: fn(&mut State)) -> State {
        run(files, setup, queue, false)
    }

    fn run(files: Files, setup: fn(&mut State), queue: fn(&mut State), flush: bool) -> State {
        let mut s = State::empty();
        s.ui = test_support::test_ui();
        setup(&mut s);
        let mut e = Engine::with_game(s, files, move |st, io| async move {
            queue(&mut st.borrow_mut());
            if flush {
                flush_prints(&st, &io).await;
            }
        });
        for _ in 0..100 {
            if e.finished() {
                break;
            }
            e.tick(&[], 16);
        }
        assert!(e.finished(), "the flush ends");
        e.state().clone()
    }

    #[test]
    fn a_line_queued_with_the_text_off_is_not_printed() {
        // The original's line is gone when its call finds the switch off, whatever the switch
        // is when the port's queue is flushed (0x11db7c turns it on again at 0x11dcaa).
        let after = queue_and_flush(
            Files::default(),
            |_| {},
            |s| {
                s.ui.text.on = false;
                queue_print(s, b"off\n".to_vec(), Vec::new());
                s.ui.text.on = true;
                queue_print(s, b"on\n".to_vec(), Vec::new());
            },
        );
        assert_eq!(after.ui.messages, ["on"]);
        assert!(after.main.pending_print.is_empty());
    }

    #[test]
    fn a_line_queued_with_the_text_on_is_printed_when_flushed_with_it_off() {
        // The original printed it at its call; the flush puts it with the switch on and leaves
        // the switch as it found it.
        let after = queue_and_flush(
            Files::default(),
            |_| {},
            |s| {
                queue_print(s, b"on\n".to_vec(), Vec::new());
                s.ui.text.on = false;
            },
        );
        assert_eq!(after.ui.messages, ["on"]);
        assert!(!after.ui.text.on, "the switch as the flush found it");
    }

    #[test]
    fn a_hit_reported_with_the_text_off_is_printed() {
        // 0x1231be forces the switch on while it reports (saved at 0x1231fc, set at 0x123204,
        // restored at 0x123870): with the switch off around it, its line still shows. Here the
        // line of a vine root's death (its vine dies), read from the user's `game`.
        let Some(files) = files() else { return };
        let after = queue_and_flush(
            files,
            |s| {
                s.objects.type_frame[ROOT] = 0x16e;
                s.objects.qq[ROOT] = (VINE as u16) << 8;
                s.actors.attrs.hp[VINE] = 5;
            },
            |s| {
                s.ui.text.on = false;
                crate::combat::damage::hurt(s, ROOT as i16, 1);
            },
        );
        let shown = after
            .ui
            .messages
            .iter()
            .filter(|l| !l.trim().is_empty())
            .count();
        println!("scroll lines with text: {shown}");
        assert_eq!(after.actors.attrs.hp[VINE], 0, "the vine died");
        assert!(shown > 0, "the report is printed");
        assert!(!after.ui.text.on, "the switch as 0x1231be left it");
    }

    /// Window and cursor calls and two lines, the first queued with the switch off.
    fn mixed(s: &mut State) {
        use crate::ui::WindowId;
        s.ui.text.on = false;
        let q = &mut s.main.pending_print;
        q.push(Pending::Window(WindowId::Status));
        q.push(Pending::Cursor(0, 0));
        queue_print(s, b"off\n".to_vec(), Vec::new());
        s.ui.text.on = true;
        s.main
            .pending_print
            .push(Pending::Window(WindowId::Messages));
        queue_print(s, b"on\n".to_vec(), Vec::new());
    }

    #[test]
    fn window_and_cursor_calls_stay_queued_in_order_with_the_text_off() {
        // 0x107dd8 and 0x107e00 do not test A4-0x7a4c (its only tests are in 0x107e5a, 0x108376
        // and 0x108658: `tst.w -0x7a4c(a4)` at 0x107e64, 0x108380, 0x108660): only the line
        // queued with the switch off is gone.
        use crate::ui::WindowId;
        let before = queued(Files::default(), |_| {}, mixed);
        assert_eq!(
            before.main.pending_print,
            [
                Pending::Window(WindowId::Status),
                Pending::Cursor(0, 0),
                Pending::Window(WindowId::Messages),
                Pending::Print(b"on\n".to_vec(), Vec::new()),
            ]
        );
        let after = queue_and_flush(Files::default(), |_| {}, mixed);
        assert_eq!(after.ui.messages, ["on"]);
        assert_eq!(after.ui.text.current, WindowId::Messages);
    }

    /// The vine's combat line (A4-0x755a), said with Attack's newline held.
    fn say_held(s: &mut State) {
        crate::combat::present::say(s, crate::image::A4 - 0x755a, Vec::new());
    }

    #[test]
    fn a_combat_line_said_with_the_text_off_keeps_the_held_newline() {
        // 0x107e5a reads and clears A4-0x7a48 (Attack's held newline) only with A4-0x7a4c set
        // (asm 0x107e64 `tst.w -0x7a4c(a4)`, 0x107e68 `beq.w` to its return; the clear at
        // 0x107e90), and 0x108658 returns before it (0x108664): a held newline stays held and
        // nothing is queued.
        let Some(files) = files() else { return };
        // With the switch on the line is queued and ends the hold (it is not only newlines).
        let on = queued(files.clone(), |s| s.combat.swallow = true, say_held);
        assert_eq!(on.main.pending_print.len(), 1, "queued with the text on");
        assert!(!on.combat.swallow, "the hold ended with the text on");
        let off = queued(
            files,
            |s| {
                s.combat.swallow = true;
                s.ui.text.on = false;
            },
            say_held,
        );
        assert!(off.main.pending_print.is_empty(), "nothing queued");
        assert!(off.combat.swallow, "the newline still held");
        assert!(off.combat.said.is_empty(), "nothing said");
    }
}
