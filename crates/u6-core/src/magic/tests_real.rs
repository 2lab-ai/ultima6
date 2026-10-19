//! Real data: spells cast from the start state of the user's archive.org zip, through the
//! engine with typed keys (the Cast command's own steps, as 0x1041d8 runs them before
//! 0x11af10). The hero gets a readied spellbook and reagents through the objects API.
//! Compares the message scroll with strings read from the user's `game` and prints only each
//! cast's label and the stub ids it hit (never the scroll's lines, how many there are or their
//! lengths); the Light test writes headless frames to /tmp/claude-501/. Skips where the zip is
//! missing; no game data in the repo.
#![allow(clippy::disallowed_methods)] // tests read the zip and write the frame receipts

use std::cell::RefCell;
use std::path::Path;

use u6_intake::{InputFile, intake};

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::image::A4;
use crate::main_loop::{flush_prints, game_string, pointed_string};
use crate::objects::{IN_CONTAINER, READIED};
use crate::state::State;
use crate::ui::printf::{Arg, printf};

use super::tables::Msg;
use u6_render::Framebuffer;
use u6_testkit::Quiet;

pub(super) const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
pub(super) const HERO: u16 = 1;

pub(super) fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// main()'s boot after load state (0x1351d8, see `main_loop::run`): palette, paper, windows,
/// the load-state tail, the animation substitutions, the clock, then the panel and the view.
pub(super) async fn boot(state: &RefCell<State>, io: &Io) {
    {
        let mut s = state.borrow_mut();
        let tables = s.ui.tables.clone();
        let pal = s.tables.palette;
        io.draw(|fb| {
            for (dst, c) in fb.palette.iter_mut().zip(&pal) {
                *dst = u6_render::Rgb::new(c.r, c.g, c.b);
            }
        });
        crate::main_loop::draw_paper(&s, io);
        s.ui.text.main_windows(&tables);
        crate::objects::init_slots(&mut s.objects);
        // As game's main, the start window's egg unhatched: its monsters would be foes in every
        // spell's view.
        crate::main_loop::test_support::load_state_like_main(&mut s, io);
        s.actors.objlist_tail[0x81] = 1;
        crate::actors::turn::after_load(&mut s);
        crate::actors::clock::tick(&mut s, 0);
    }
    flush_prints(state, io).await;
    crate::ui::panels::redraw_panel(state, io).await;
    crate::gfx::view::refresh_view(&mut state.borrow_mut(), io);
}

/// A readied spellbook with `spells` inside for the hero, and 10 of each reagent.
pub(super) fn give_book(s: &mut State, spells: &[u8]) -> i16 {
    let book = crate::objects::create_in(s, super::SPELLBOOK, READIED, 0, HERO as i16);
    for &n in spells {
        crate::objects::create_in(s, super::SPELL, IN_CONTAINER, u16::from(n) << 8, book);
    }
    for kind in s.magic.tables.reagents {
        crate::objects::add_to_owner(s, HERO as i16, kind, 10);
    }
    book
}

/// The command loop's part of Cast (0x104216 and 0x10498e..0x104a96): the spell cursor (mode
/// 3, range 7, cell (5, 5)), the target pending, the Cast line, the command, the cleanup.
pub(super) async fn cast_command(state: &RefCell<State>, io: &Io) {
    {
        let mut s = state.borrow_mut();
        crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
        let input = &mut s.ui.input;
        input.cursor_by_tab = false;
        input.targeting = 3;
        input.cursor = (5, 5);
        input.range = 7;
        input.target_pending = 1;
        input.target_command = 0x82;
    }
    let name = pointed_string(io, A4.wrapping_sub(0x7c8e) + 4 * 0x82);
    let fmt = game_string(io, 0x0010_1f48);
    printf(state, io, &fmt, &[Arg::Str(&name)]).await;
    super::cast(state, io).await;
    {
        let mut s = state.borrow_mut();
        s.ui.input.target_pending = 0;
        crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
        s.ui.input.targeting = 0;
        s.ui.input.target_command = crate::ui::globals::NO_COMMAND;
    }
    flush_prints(state, io).await;
}

/// Lines of the message scroll. They are the game's text, so the debug form (the assertion
/// messages) is a label only: not their text, how many there are nor their lengths (the count
/// and lengths of the user's records are not shown either); the tests compare them with strings
/// read from the user's `game` at run time ([`text_at`], [`msg`], [`printed`]).
pub(super) struct Scroll(Vec<String>);

impl std::ops::Deref for Scroll {
    type Target = [String];
    fn deref(&self) -> &[String] {
        &self.0
    }
}

impl std::fmt::Debug for Scroll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the message scroll (its lines are not shown)")
    }
}

impl Scroll {
    /// The first line holding `text`; never for an empty `text` (a string missing from `game`).
    pub(super) fn find(&self, text: &str) -> Option<usize> {
        if text.is_empty() {
            return None;
        }
        self.0.iter().position(|l| l.contains(text))
    }

    /// True when a line holds `text` (see [`Self::find`]), or the lines do once joined (the
    /// scroll wraps long lines at a space or mid-word).
    pub(super) fn has(&self, text: &str) -> bool {
        self.find(text).is_some()
            || (!text.is_empty() && [" ", ""].iter().any(|sep| self.0.join(sep).contains(text)))
    }
}

/// `bytes` as text without its newlines (the scroll keeps one line per entry).
fn plain(bytes: &[u8]) -> String {
    bytes
        .iter()
        .filter(|&&b| b != b'\n')
        .map(|&b| char::from(b))
        .collect()
}

/// The string at image address `addr` of the user's `game`, without its newlines.
pub(super) fn text_at(io: &Io, addr: u32) -> String {
    plain(&game_string(io, addr))
}

/// Result message `m` of the spell tables (read from the user's `game` at load).
pub(super) fn msg(e: &Engine, m: Msg) -> String {
    plain(e.state().magic.tables.message(m))
}

/// The format at image address `addr` with each conversion (`%s`, `%d`, `%3d` ...) replaced by
/// the next of `args`, without its newlines.
pub(super) fn printed(io: &Io, addr: u32, args: &[&str]) -> String {
    let fmt = text_at(io, addr);
    let mut args = args.iter();
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut conv = chars.next();
        while conv.is_some_and(|d| d.is_ascii_digit()) {
            conv = chars.next();
        }
        if conv.is_some() {
            out.push_str(args.next().copied().unwrap_or_default());
        }
    }
    out
}

/// The longest piece of the format at `addr` between its conversions.
pub(super) fn literal(io: &Io, addr: u32) -> String {
    let fmt = text_at(io, addr);
    let mut best = String::new();
    for part in fmt.split('%') {
        let lit = part.trim_start_matches(|c: char| c.is_ascii_digit());
        let lit = lit.get(1..).unwrap_or_default();
        if lit.len() > best.len() {
            best = lit.to_string();
        }
    }
    let first = fmt.split('%').next().unwrap_or_default();
    if first.len() > best.len() {
        best = first.to_string();
    }
    best
}

/// The scroll lines so far, the one being written last.
pub(super) fn lines_of(s: &State) -> Scroll {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t
        .transcript_line
        .iter()
        .map(|&b| crate::ui::wide::latin_char(b))
        .collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| crate::ui::wide::latin_char(b)));
    lines.push(last);
    Scroll(lines)
}

/// The line the command loop prints for Cast (0x101f48 with the command's name).
pub(super) fn cast_line_text(io: &Io) -> String {
    let name = plain(&pointed_string(io, A4.wrapping_sub(0x7c8e) + 4 * 0x82));
    printed(io, 0x0010_1f48, &[&name])
}

pub(super) fn keys(spec: &str) -> Vec<InputEvent> {
    spec.chars()
        .map(|c| {
            InputEvent::Key(match c {
                '\n' => Key::Enter,
                '^' => Key::Up,
                'v' => Key::Down,
                '~' => Key::Escape,
                '<' => Key::Left,
                '>' => Key::Right,
                c => Key::Char(c),
            })
        })
        .collect()
}

/// A booted engine that sets the state up with `setup`, then casts once; `input` is typed
/// one key per tick. Returns the engine when the cast is over.
pub(super) fn cast_with(files: &Files, setup: fn(&mut State), input: &str) -> Engine {
    let state = State::new_game(files).quiet();
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot(&st, &io).await;
        setup(&mut st.borrow_mut());
        cast_command(&st, &io).await;
    });
    e.tick(&[], 16);
    for k in keys(input) {
        e.tick(&[k], 16);
    }
    for _ in 0..8000 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(
        e.finished(),
        "the cast still waits: {:?}",
        lines_of(&e.state())
    );
    e
}

/// Reagent counts the hero carries, in reagent order.
pub(super) fn reagents(s: &State) -> Vec<i16> {
    let t = s.magic.tables.reagents;
    t.iter()
        .map(|&k| super::book::carried_count(&s.objects, HERO as i16, k, 0))
        .collect()
}

/// The scroll from the last Cast line on ([`cast_line_text`]); prints the label and the stub
/// ids hit (the lines and their count are the game's).
pub(super) fn show(name: &str, e: &Engine) -> Scroll {
    let lines = lines_of(&e.state());
    let cast = cast_line_text(e.io());
    let from = if cast.is_empty() {
        0
    } else {
        lines.iter().rposition(|l| l.contains(&cast)).unwrap_or(0)
    };
    let tail = Scroll(lines[from..].to_vec());
    eprintln!("--- {name}");
    eprintln!(
        "  stub hits: {:?}",
        e.stub_hits().keys().collect::<Vec<_>>()
    );
    tail
}

/// A PNG of the frame (RGB, zlib stored blocks).
pub(super) fn png(fb: &Framebuffer) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let (w, h) = (fb.width(), fb.height());
    let mut rgba = Vec::new();
    fb.write_rgba(&mut rgba);
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&rgba[(y * w + x) * 4..(y * w + x) * 4 + 3]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(0xffff).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = b.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// View cells drawn lit (neither unseen 0xff nor dark 0x1bc).
pub(super) fn lit_cells(s: &State) -> usize {
    (0..11)
        .flat_map(|r| (0..11).map(move |c| (c, r)))
        .filter(|&(c, r)| {
            let t = s.map.view_tile(c, r);
            t != crate::map::view::NOT_VISIBLE && t != crate::map::view::DARK
        })
        .count()
}

/// What a cast of `spell` must spend: one of each of its reagents, one point per circle.
pub(super) fn spent(before: &[i16], after: &[i16], mp: (u8, u8), s: &State, spell: usize) {
    let mask = s.magic.tables.mask(spell);
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        let want = b - i16::from(mask >> i & 1);
        assert!(*a == want, "reagent {i} of spell {spell:#x}");
    }
    assert!(
        mp.1 == mp.0 - (spell / 16 + 1) as u8,
        "magic points of spell {spell:#x}"
    );
}

fn night_book(s: &mut State) {
    give_book(s, &[0, 1, 6, 9, 0x11, 0x14, 0x34]);
    s.actors.calendar.hour = 22;
    crate::actors::clock::tick(s, 0);
    crate::map::view::update_view(s);
}

/// The hero's magic points in the shipped objlist (its byte of the table at 0x13F1,
/// formats.md §6), read from the raw file: what a cast from the start spends from.
fn start_magic(files: &Files) -> u8 {
    files.get("savegame/objlist").quiet()[0x13F1 + usize::from(HERO)]
}

/// The NPC the casts below aim at: actor 7, put on the tile the cross-hair reaches with two
/// rights and three ups.
const TARGET: usize = 7;

fn day_book(s: &mut State) {
    give_book(s, &[0, 1, 6, 9, 0x11, 0x14, 0x34]);
    s.actors.attrs.hp[2] = 10;
    let c = s.map.centre;
    let z = u16::from(c.z);
    crate::objects::move_to(s, TARGET as i16, c.x + 2, c.y - 3, z);
    // A potion on the floor two cells east of the hero, for Detect Magic.
    let c = s.map.centre;
    crate::objects::create_on_map(
        &mut s.objects,
        0x113 | 3 << 10,
        0,
        0,
        c.x + 2,
        c.y,
        u16::from(c.z),
    );
    crate::map::view::update_view(s);
}

#[test]
fn light_at_night_lights_the_view() {
    let Some(files) = files() else { return };
    let state = State::new_game(&files).quiet();
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot(&st, &io).await;
        night_book(&mut st.borrow_mut());
        crate::gfx::view::refresh_view(&mut st.borrow_mut(), &io);
        cast_command(&st, &io).await;
    });
    e.tick(&[], 16);
    let (dark, before, mp0) = {
        let s = e.state();
        (lit_cells(&s), reagents(&s), s.actors.attrs.magic[1])
    };
    std::fs::write("/tmp/claude-501/m2-magic-light-before.png", png(e.frame())).quiet();
    // The book picture 0x116028 draws is the user's file named at A4-0x61de.
    let name = pointed_string(e.io(), A4.wrapping_sub(0x61de));
    let bmp = files
        .get(&String::from_utf8_lossy(&name))
        .and_then(|f| u6_formats::bitmap::Bitmap::from_lzw(f).ok());
    assert!(bmp.is_some(), "the book picture decodes");
    for k in keys("IL\n") {
        e.tick(&[k], 16);
    }
    for _ in 0..2000 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let tail = show("spell 0x09 at 22:00", &e);
    std::fs::write("/tmp/claude-501/m2-magic-light-after.png", png(e.frame())).quiet();
    let s = e.state();
    let lit = lit_cells(&s);
    let light = plain(s.magic.tables.name(9));
    let announced = printed(e.io(), super::cast::FMT_PAREN, &[&light]);
    assert!(tail.has(&announced), "{tail:?}");
    assert!(s.actors.timers[0] == 100);
    assert!(lit > dark, "more view cells lit after the cast");
    spent(
        &before,
        &reagents(&s),
        (mp0, s.actors.attrs.magic[1]),
        &s,
        9,
    );
}

#[test]
fn create_food_adds_food() {
    let Some(files) = files() else { return };
    let food = |s: &State| super::book::carried_count(&s.objects, 1, 0x81, 0);
    let e = cast_with(&files, day_book, "IMY\n");
    let tail = show("spell 0x00", &e);
    let s = e.state();
    let n = food(&s);
    assert!((1..=10).contains(&n), "1..=10 food made");
    let line = printed(e.io(), super::spells::MSG_FOOD, &[&n.to_string()]);
    assert!(tail.has(&line), "{tail:?}");
    assert!(s.magic.current == 0, "the books remember the spell");
    spent(
        &[10; 8],
        &reagents(&s),
        (start_magic(&files), s.actors.attrs.magic[1]),
        &s,
        0,
    );
}

/// Detect Magic on an object. The cross-hair cannot pick an object yet (its pick 0x1225c0 is a
/// ui seam whose fallback finds actors only), so the spell runs with the target chosen, as
/// 0x11abd0 runs it for a target picked before the book (0x11a586 with D1 = 1).
#[test]
fn detect_magic_on_a_potion() {
    let Some(files) = files() else { return };
    let state = State::new_game(&files).quiet();
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot(&st, &io).await;
        {
            let s = &mut *st.borrow_mut();
            day_book(s);
            let c = s.map.centre;
            let at =
                crate::objects::begin_at(&mut s.objects, &s.tables, c.x + 2, c.y, u16::from(c.z));
            s.ui.input.selection = at;
            s.ui.input.target = (c.x as i16 + 2, c.y as i16);
        }
        let name = pointed_string(&io, A4.wrapping_sub(0x7c8e) + 4 * 0x82);
        let fmt = game_string(&io, 0x0010_1f48);
        printf(&st, &io, &fmt, &[Arg::Str(&name)]).await;
        super::dispatch(&st, &io, 1, true).await;
        flush_prints(&st, &io).await;
    });
    for _ in 0..2000 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let tail = show("spell 0x01 on the potion", &e);
    let s = e.state();
    let sel = s.ui.input.selection;
    assert!(
        s.objects.kind(sel) == super::detect::POTION,
        "a potion two east"
    );
    let kind = usize::from(s.objects.tf(sel) >> 10);
    let what = plain(s.magic.tables.potions.get(kind).map_or(&[], Vec::as_slice));
    let line = printed(e.io(), super::detect::MSG_POTION, &[&what]);
    assert!(tail.has(&line), "{tail:?}");
}

#[test]
fn heal_and_great_heal_on_a_member() {
    let Some(files) = files() else { return };
    let e = cast_with(&files, day_book, "IM\n2");
    show("spell 0x06 on member 2", &e);
    let s = e.state();
    let hp = s.actors.attrs.hp[2];
    assert!((11..=40).contains(&hp), "Heal gives 1..30");
    spent(
        &[10; 8],
        &reagents(&s),
        (start_magic(&files), s.actors.attrs.magic[1]),
        &s,
        6,
    );
    drop(s);
    let e = cast_with(&files, day_book, "VM\n2");
    show("spell 0x34 on member 2", &e);
    let s = e.state();
    assert!(u16::from(s.actors.attrs.hp[2]) == crate::actors::leaves::max_hp(&s, 2));
    spent(
        &[10; 8],
        &reagents(&s),
        (start_magic(&files), s.actors.attrs.magic[1]),
        &s,
        0x34,
    );
}

#[test]
fn sleep_on_an_npc() {
    let Some(files) = files() else { return };
    let e = cast_with(&files, day_book, "IZ\n>>^^^\n");
    let tail = show("spell 0x14 on the target", &e);
    let s = e.state();
    let asleep = s.actors.attrs.status[TARGET] & 4 != 0;
    assert!(
        asleep || tail.has(&msg(&e, Msg::Failed)) || tail.has(&msg(&e, Msg::NoEffect)),
        "{tail:?}"
    );
    spent(
        &[10; 8],
        &reagents(&s),
        (start_magic(&files), s.actors.attrs.magic[1]),
        &s,
        0x14,
    );
}

#[test]
fn magic_arrow_goes_through_the_combat_seams() {
    let Some(files) = files() else { return };
    let before = State::new_game(&files).quiet().actors.attrs.hp[TARGET];
    let e = cast_with(&files, day_book, "OJ\n>>^^^\n");
    let tail = show("spell 0x11 on the target", &e);
    let hits = e.stub_hits();
    let s = e.state();
    // The flight (0x1228ba) and the hit (0x1231be, 0x123c56, 0x12395a) are combat's ports.
    assert!(
        !hits
            .keys()
            .any(|k| k.starts_with("0x1228ba") || k.starts_with("0x1231be"))
    );
    let failed = tail.has(&msg(&e, Msg::Failed));
    assert!(failed || s.actors.attrs.hp[TARGET] < before, "{tail:?}");
    if !failed {
        // 0x11669e's order: the target's reaction (0x123c56) before the newline and the wound
        // line (0x12395a).
        // The words of the yell (A4-0x7318, its longest literal part) and of the wound word
        // (A4-0x772e), from the user's `game`.
        let yell = literal(e.io(), A4.wrapping_sub(0x7318));
        let wounded = text_at(e.io(), A4.wrapping_sub(0x772e));
        let (y, w) = (tail.find(&yell), tail.find(wounded.trim()));
        assert!(y.zip(w).is_none_or(|(y, w)| y < w), "{tail:?}");
    }
    spent(
        &[10; 8],
        &reagents(&s),
        (start_magic(&files), s.actors.attrs.magic[1]),
        &s,
        0x11,
    );
}
