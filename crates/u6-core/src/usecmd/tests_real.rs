//! Real data: Use on objects of the shipped world, from the user's archive.org zip. Skips where
//! the zip is missing; prints the case labels and the stub points, never the text nor how many
//! lines or how long (no game text in the repo or the test output). With
//! `U6_USE_FRAMES=<dir>` each case also writes the frame after the command as a PNG there.
#![allow(clippy::disallowed_methods)] // tests may read files and the environment

use std::path::Path;

use u6_intake::{InputFile, intake};

use super::use_command;
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::objects::Slot;
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
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

fn start(files: &Files) -> State {
    let mut s = State::new_game(files).quiet();
    crate::main_loop::load_state_tail(&mut s);
    let t = s.ui.tables.clone();
    s.ui.text.main_windows(&t);
    s.ui.text.more_rows = 100;
    s
}

/// The scroll lines so far, the one being written last.
fn lines_of(s: &State) -> Vec<String> {
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
    lines
}

/// Map records of `kind` in the object blocks, nearest to (x, y) on level `z` first.
fn records(s: &State, kind: u16, x: u16, y: u16, z: u8) -> Vec<(u16, u16, u8, u8)> {
    let mut v: Vec<_> = s
        .objects
        .blocks
        .iter()
        .flat_map(|(_, b)| b.objects.iter())
        .filter(|r| r.obj_n == kind && r.status & 0x18 == 0 && r.pos.z == z)
        .map(|r| (r.pos.x, r.pos.y, r.frame, r.quality))
        .collect();
    v.sort_by_key(|&(rx, ry, _, _)| (rx as i32 - x as i32).abs() + (ry as i32 - y as i32).abs());
    v
}

/// The party teleported to (x, y, z) (map 0x1092fc).
fn goto(s: &mut State, x: u16, y: u16, z: u8) {
    crate::map::travel::teleport(s, x, y, u16::from(z), 0);
    crate::objects::build_draw_lists(s);
    crate::map::view::update_view(s);
}

/// The object of `kind` at (x, y, z) in slots, -1.
fn slot_at(s: &mut State, x: u16, y: u16, z: u8, kind: u16) -> Slot {
    crate::objects::object_of_kind_at(s, x, y, u16::from(z), kind)
}

/// Runs Use on `target` (`cell` = the selected map cell) with the screen drawn before and
/// after; `keys` answer the handler's own prompts, one per idle tick. Returns the engine.
fn use_on(s: State, files: &Files, target: Slot, cell: (i16, i16), keys: &[InputEvent]) -> Engine {
    let mut s = s;
    s.ui.input.selection = target;
    s.ui.input.target = cell;
    s.ui.input.target_pending = 1;
    s.gfx.window_shown = 1;
    s.ui.panel.dirty = true;
    let mut e = Engine::with_game(s, files.clone(), |st, io| async move {
        {
            let mut s = st.borrow_mut();
            crate::main_loop::draw_paper(&mut s, &io);
        }
        crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
        crate::ui::panels::redraw_panel(&st, &io).await;
        use_command(&st, &io).await;
        crate::main_loop::flush_prints(&st, &io).await;
        crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
        io.present();
    });
    let mut queued = keys.iter();
    // Room for the crank's 20 paced view refreshes (some 280 ticks).
    for _ in 0..1000 {
        if e.finished() {
            break;
        }
        let input: Vec<InputEvent> = if e.io().pending_events() == 0 {
            queued.next().copied().into_iter().collect()
        } else {
            Vec::new()
        };
        e.tick(&input, 16);
    }
    assert!(e.finished(), "Use did not finish");
    e
}

/// With the environment variable `var` naming a directory, the engine's frame as
/// `<dir>/<name>.png`; prints the file's name only.
pub(crate) fn save_frame(e: &Engine, var: &str, name: &str) {
    let Ok(dir) = std::env::var(var) else {
        return;
    };
    let mut rgba = Vec::new();
    e.frame().write_rgba(&mut rgba);
    let file = format!("{name}.png");
    let (w, h) = (u6_render::WIDTH as u32, u6_render::HEIGHT as u32);
    std::fs::write(Path::new(&dir).join(&file), png(w, h, &rgba)).quiet();
    eprintln!("frame: {file}");
}

/// A minimal PNG (stored deflate blocks) of an RGBA image.
fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for row in rgba.chunks(w as usize * 4) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(0xffff).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend(*b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend((b << 16 | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Prints the stub points the command reached; writes its frame. Returns the lines it added
/// for the caller's checks.
fn report(e: &Engine, name: &str, before: usize) -> Vec<String> {
    let lines = lines_of(&e.state());
    let new: Vec<String> = lines
        .into_iter()
        .skip(before.saturating_sub(1))
        .filter(|l| !l.trim().is_empty())
        .collect();
    eprintln!(
        "[{name}] stubs: {:?}",
        e.stub_hits().keys().collect::<Vec<_>>()
    );
    save_frame(e, "U6_USE_FRAMES", name);
    new
}

/// The lookup the cases below use: the map records of each Use type on the start's level come
/// nearest the start first, and the leader carries objects (what is where stays unprinted: it
/// is the user's world).
#[test]
fn where_the_use_objects_are() {
    let Some(files) = files() else { return };
    let s = start(&files);
    let c = s.map.centre;
    let dist = |r: &(u16, u16, u8, u8)| r.0.abs_diff(c.x) + r.1.abs_diff(c.y);
    for kind in [
        0x62, 0x5a, 0x7a, 0x81, 0x10c, 0x12c, 0x12a, 0x19c, 0x19e, 0x57, 0x9a, 0xdf, 0x13c,
    ] {
        let r = records(&s, kind, c.x, c.y, c.z);
        assert!(
            r.windows(2).all(|w| dist(&w[0]) <= dist(&w[1])),
            "type {kind:#x}: nearest first"
        );
    }
    let lead = s.actors.party.leader().quiet().slot();
    let mut o = s.objects.clone();
    assert!(
        crate::objects::first_child(&mut o, lead) >= 0,
        "the leader carries objects"
    );
}

/// The leader's slot.
fn lead(s: &State) -> Slot {
    s.actors.party.leader().map_or(-1, |l| l.slot())
}

/// Nearest map record of `kind` with frame `frame` (any when `None`) to the start.
fn nearest(files: &Files, kind: u16, frame: Option<u8>) -> Option<(u16, u16)> {
    let s = start(files);
    let c = s.map.centre;
    records(&s, kind, c.x, c.y, 0)
        .into_iter()
        .find(|r| frame.is_none_or(|f| r.2 == f))
        .map(|r| (r.0, r.1))
}

/// Teleports south of (x, y) and finds the object of `kind` there.
fn at(files: &Files, kind: u16, (x, y): (u16, u16)) -> (State, Slot) {
    let mut s = start(files);
    goto(&mut s, x, y + 1, 0);
    let slot = slot_at(&mut s, x, y, 0, kind);
    assert!(slot >= 0, "no {kind:#x} where the records put it");
    (s, slot)
}

fn frame_of(e: &Engine, slot: Slot) -> u16 {
    e.state().objects.tf(slot) >> 10
}

#[test]
fn use_on_eight_kinds_of_objects() {
    let Some(files) = files() else { return };

    // 1. A closed chest (0x62 frame 1) opens.
    let pos = nearest(&files, 0x62, Some(1)).expect("a closed chest");
    let (s, chest) = at(&files, 0x62, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, chest, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "1-chest", n);
    assert!(e.state().objects.kind(chest) == 0x62);
    assert!(frame_of(&e, chest) == 0, "the chest is open");

    // 2. A closed door (0x12c, frames 4-7) opens; used again it closes.
    let pos = nearest(&files, 0x12c, Some(5)).expect("a closed door");
    let (s, door) = at(&files, 0x12c, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, door, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "2-door-open", n);
    assert!(frame_of(&e, door) == 1, "frame 5 ^ 4: open");
    let s = e.state().clone();
    let n = lines_of(&s).len();
    let e = use_on(s, &files, door, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "2-door-close", n);
    assert!(frame_of(&e, door) == 5, "closed again");

    // 3. A lever (0x10c) flips its frame bit 0 and works the portcullises of its quality.
    let pos = nearest(&files, 0x10c, None).expect("a lever");
    let (s, lever) = at(&files, 0x10c, pos);
    let f0 = s.objects.tf(lever) >> 10;
    let n = lines_of(&s).len();
    let e = use_on(s, &files, lever, (pos.0 as i16, pos.1 as i16), &[]);
    let out = report(&e, "3-lever", n);
    assert!(frame_of(&e, lever) == f0 ^ 1);
    assert!(!out.is_empty());

    // 4. A lit candle (0x7a frame 1) is doused, then lit again.
    let pos = nearest(&files, 0x7a, Some(1)).expect("a lit candle");
    let (s, candle) = at(&files, 0x7a, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, candle, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "4-candle-douse", n);
    assert!(frame_of(&e, candle) == 0);
    let s = e.state().clone();
    let n = lines_of(&s).len();
    let e = use_on(s, &files, candle, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "4-candle-light", n);
    assert!(frame_of(&e, candle) == 1);

    // 5. A torch (0x5a) on the ground is lit (status 0x80, frame 1, 200 turns); used again it
    // burns out (goes).
    let pos = nearest(&files, 0x5a, Some(0)).expect("a torch");
    let (s, torch) = at(&files, 0x5a, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, torch, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "5-torch-light", n);
    {
        let s = e.state();
        assert!(s.objects.obj_status[torch as usize] & 0x80 == 0x80);
        assert!(s.objects.tf(torch) == (0x5a | 1 << 10));
        assert!(s.objects.qq_of(torch) & 0xff == 200);
    }
    let s = e.state().clone();
    let n = lines_of(&s).len();
    let e = use_on(s, &files, torch, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "5-torch-out", n);
    assert!(e.state().objects.tf(torch) == 0, "the torch slot is free");

    // 6. Food (0x81) is eaten: one fewer, or gone.
    let pos = nearest(&files, 0x81, None).expect("food");
    let (s, food) = at(&files, 0x81, pos);
    let q0 = s.objects.qq_of(food) & 0xff;
    let n = lines_of(&s).len();
    let e = use_on(s, &files, food, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "6-food", n);
    let after = e.state().objects.tf(food);
    if q0 > 1 && after != 0 {
        assert!(e.state().objects.qq_of(food) & 0xff == q0 - 1);
    } else {
        assert!(after == 0, "eaten");
    }

    // 7. Board a ship (0x19c), then leave it.
    let pos = nearest(&files, 0x19c, None).expect("a ship");
    let (mut s, ship) = at(&files, 0x19c, pos);
    // Its deed: since 0x1240fa is ported the Use goes to the ship's head part, which carries
    // the quality the deed must match (0x12f502: otherwise the deed line 0x12cd90).
    let head = crate::combat::leaves::owner(&mut s, ship);
    let q = s.objects.qq_of(head) >> 8;
    if q != 0 {
        let me = lead(&s);
        crate::objects::create_in(&mut s, 0x95, crate::objects::IN_INVENTORY, q << 8 | 1, me);
    }
    let n = lines_of(&s).len();
    let e = use_on(s, &files, ship, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "7-ship-board", n);
    let s = e.state().clone();
    assert!(
        s.actors.party.leader_index == s.actors.party.size,
        "aboard: the leader index is the party size"
    );
    assert!(s.objects.kind(0) == 0x19c, "actor 0 is the ship");
    let c = s.map.centre;
    let n = lines_of(&s).len();
    let e = use_on(s, &files, 0, (c.x as i16, c.y as i16), &[]);
    report(&e, "7-ship-leave", n);

    // 8. The orb of the moons (0x57, carried by the hero). At the start nobody has told the
    // hero how to use it (actor 5's talk flag bit 5, set in actor 5's conversation): the
    // line 0x12ce42. With the flag set it makes a red moongate (0x54) two tiles north.
    for told in [false, true] {
        let mut s = start(&files);
        let me = lead(&s);
        let mut orb = crate::objects::first_child(&mut s.objects, me);
        while orb >= 0 && s.objects.kind(orb) != 0x57 {
            orb = crate::objects::next_child(&mut s.objects);
        }
        assert!(orb >= 0, "the hero carries the orb");
        assert!(
            s.actors.attrs.talk_flags[5] & 0x20 == 0,
            "not told at the start"
        );
        if told {
            s.actors.attrs.talk_flags[5] |= 0x20;
        }
        let gates = |s: &State| {
            s.objects
                .type_frame
                .iter()
                .enumerate()
                .filter(|&(_, &w)| w & 0x3ff == 0x54)
                .map(|(i, _)| i as Slot)
                .collect::<Vec<_>>()
        };
        // The start egg's gargoyles (hatched by the boot since the objects port of 0x1319b2)
        // stand around the party; one on the gate's tile would make it fail (the failed message,
        // as in the original). They go to level 15 (out of the world) for this case.
        {
            let c = s.map.centre;
            let (gx, gy) = (c.x, c.y - 2);
            for a in 0xe0..0x100i16 {
                let p = s.objects.location[a as usize];
                if s.objects.type_frame[a as usize] != 0 && (p.x, p.y, p.z) == (gx, gy, c.z) {
                    crate::objects::move_to(&mut s, a, gx, gy, 0xf);
                }
            }
        }
        let gates0 = gates(&s);
        s.ui.input.targeting = 1;
        s.ui.input.cursor = crate::ui::target::LEADER_CELL;
        s.ui.input.range = 7;
        let c = s.map.centre;
        let n = lines_of(&s).len();
        let up = InputEvent::Key(Key::Up);
        let keys = [up, up, InputEvent::Key(Key::Enter)];
        let e = use_on(s, &files, orb, (-1, -1), &keys);
        let name = if told { "8-orb-told" } else { "8-orb-start" };
        report(&e, name, n);
        let gates1 = gates(&e.state());
        if told {
            assert!(gates1.len() == gates0.len() + 1, "a moongate appeared");
            let g = *gates1.iter().find(|g| !gates0.contains(g)).quiet();
            let s = e.state();
            assert!((s.objects.x(g), s.objects.y(g)) == (c.x, c.y - 2));
            assert!(
                s.objects.qq_of(g) >> 8 == 3,
                "quality = (dy + 2) * 5 + dx + 3"
            );
        } else {
            assert!(gates1 == gates0);
        }
    }
}

/// The first map record of `kind` on any level (surface first), with its level.
fn anywhere(files: &Files, kind: u16) -> Option<(u16, u16, u8)> {
    let s = start(files);
    (0..=5u8).find_map(|z| records(&s, kind, 0, 0, z).first().map(|r| (r.0, r.1, z)))
}

/// Which of `sites` (strings read from the user's `game` at run time) is among `lines`: its
/// index, -1 for none. Compared with the white space taken out (the scroll wraps a long line);
/// the longest that matches, should one line hold another.
fn shown_line(e: &Engine, lines: &[String], sites: &[u32]) -> i32 {
    let bare = |t: &str| t.split_whitespace().collect::<String>();
    let shown = bare(&lines.join(" "));
    sites
        .iter()
        .map(|&a| {
            let text = crate::main_loop::game_string(e.io(), a);
            bare(
                &text
                    .iter()
                    .map(|&b| crate::ui::wide::latin_char(u16::from(b)))
                    .collect::<String>(),
            )
        })
        .enumerate()
        .filter(|(_, t)| !t.is_empty() && shown.contains(t.as_str()))
        .max_by_key(|(_, t)| t.len())
        .map_or(-1, |(i, _)| i as i32)
}

/// The eggs (0x14f) on the map holding a silver serpent (0x19d): their slots.
fn serpent_eggs(s: &State) -> Vec<Slot> {
    let mut s = s.clone();
    let mut v = Vec::new();
    for i in 0x100..crate::objects::OBJECT_SLOTS as Slot {
        if s.objects.kind(i) != 0x14f || s.objects.place(i) != 0 {
            continue;
        }
        let mut c = crate::objects::first_child(&mut s.objects, i);
        while c >= 0 {
            if s.objects.kind(c) == 0x19d {
                v.push(i);
                break;
            }
            c = crate::objects::next_child(&mut s.objects);
        }
    }
    v
}

/// The serpent figure (0x139, 0x12ff50) where the world has it. Use refuses (0x12cee2) and
/// makes nothing while a silver serpent (0x19d) holds one of actor slots 0xe0..0x100; else it
/// lays up to three eggs of a silver serpent round the view, its line 0x12cf00 when it laid one
/// (the count at 0x1301aa), else 0x12ceee; each egg laid hatches (0x1319b2 marks it 0x40 and
/// 0x02) and a silver serpent comes out. A silver serpent out before the first Use (the world's
/// own) is taken away and Use made again, so the laying runs on the world's ground; one out
/// makes the next Use refuse. Prints the line indices and counts only.
#[test]
fn the_serpent_figure_hatches_silver_serpents_and_refuses_while_one_is_out() {
    let Some(files) = files() else { return };
    let (kind, name) = (0x139, "10-serpent-figure");
    let Some((x, y, z)) = anywhere(&files, kind) else {
        eprintln!("[{name}] no {kind:#x} in the world");
        return;
    };
    let out = |s: &State| -> Vec<Slot> {
        (0xe0..0x100)
            .filter(|&a| s.objects.kind(a) == 0x19d)
            .collect()
    };
    let mut s = start(&files);
    goto(&mut s, x, y + 1, z);
    let obj = slot_at(&mut s, x, y, z, kind);
    assert!(obj >= 0, "no {kind:#x} where the records put it");
    let cell = (x as i16, y as i16);
    let lines = [0x0012_cee2, 0x0012_ceee, 0x0012_cf00];
    let used = |s: State, tag: &str| {
        let before = out(&s).len();
        let eggs0 = serpent_eggs(&s);
        let n = lines_of(&s).len();
        let e = use_on(s, &files, obj, cell, &[]);
        let line = shown_line(&e, &report(&e, tag, n), &lines);
        assert!(
            !e.stub_hits().keys().any(|k| k.starts_with("0x12ff50")),
            "[{tag}] 0x12ff50 is ported"
        );
        let s = e.state().clone();
        let after = out(&s).len();
        let new: Vec<Slot> = serpent_eggs(&s)
            .into_iter()
            .filter(|g| !eggs0.contains(g))
            .collect();
        eprintln!(
            "[{tag}] {before} out before, {} eggs laid, line {line}, {after} out after",
            new.len()
        );
        let want = match (before, new.len()) {
            (0, 0) => 1,
            (0, _) => 2,
            _ => 0,
        };
        assert_eq!(line, want, "[{tag}] the line");
        if before > 0 {
            assert!(new.is_empty() && after == before, "[{tag}] none made");
        }
        if !new.is_empty() {
            assert!(
                new.iter().all(|&g| s.objects.status(g) & 0x42 == 0x42),
                "[{tag}] each egg laid hatched"
            );
            assert!(after > before, "[{tag}] a silver serpent came out");
        }
        (s, after)
    };
    let world_s = !out(&s).is_empty();
    let (mut s, mut n) = used(s, name);
    if world_s {
        for a in out(&s) {
            crate::objects::remove(&mut s, a);
        }
        assert!(
            out(&s).is_empty(),
            "[{name}] the serpents out are taken away"
        );
        (s, n) = used(s, &format!("{name}-cleared"));
    }
    if n > 0 {
        used(s, &format!("{name}-again"));
    }
}

/// The drawbridge pieces (0x10d) on the surface in the object slots: (x, y, frame), sorted;
/// with `quality`, only the pieces of that quality.
fn bridge_pieces(s: &State, quality: Option<u16>) -> Vec<(u16, u16, u16)> {
    let o = &s.objects;
    let mut v: Vec<_> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&i| o.kind(i) == 0x10d && o.place(i) == 0 && o.z(i) == 0)
        .filter(|&i| quality.is_none_or(|q| o.qq_of(i) >> 8 == q))
        .map(|i| (o.x(i), o.y(i), o.tf(i) >> 10))
        .collect();
    v.sort();
    v
}

/// Which of 0x12e3f6's lines ([`super::mech::STR_BRIDGE`]) is among `lines`: its result 0..4,
/// -1 for none ([`shown_line`]).
fn bridge_line(e: &Engine, lines: &[String]) -> i32 {
    shown_line(e, lines, &super::mech::STR_BRIDGE)
}

/// Every drawbridge crank (0x120) on the surface, each from a new game: Use lowers the bridge of
/// its quality (its pieces of that quality change; the line of result 2), Use again raises it
/// (every piece where and as it was before; the line of result 1). The leader's followers are
/// parked out of the world first (level 15, as the region leave 0x108fba parks members), so no
/// member but the leader stands where the bridge comes down or lies. A crank with no piece of
/// its quality is counted apart. Prints the crank indices, the result index of each Use's line,
/// pass / fail and counts only. Real data, two Uses of 20 paced view refreshes per crank: run
/// with `--ignored` (the merge gate).
#[test]
#[ignore = "real data: the user's files, run with --ignored"]
fn each_surface_crank_turns_its_bridge_and_back() {
    let Some(files) = files() else { return };
    let pass = |ok: bool| if ok { "pass" } else { "fail" };
    let cranks = records(&start(&files), 0x120, 0, 0, 0);
    let (mut passed, mut failed, mut no_bridge) = (0, 0, 0);
    for (i, &(x, y, _, _)) in cranks.iter().enumerate() {
        let mut s = start(&files);
        goto(&mut s, x, y + 1, 0);
        let me = lead(&s);
        let (lx, ly) = (s.objects.x(me), s.objects.y(me));
        let others: Vec<Slot> = s
            .actors
            .party
            .active()
            .map(|m| m.slot())
            .filter(|&m| m != me)
            .collect();
        for m in others {
            crate::objects::move_to(&mut s, m, lx, ly, 0xf);
        }
        let crank = slot_at(&mut s, x, y, 0, 0x120);
        assert!(crank >= 0, "[crank {i}] no crank where the records put it");
        let q = s.objects.qq_of(crank) >> 8;
        let (all, own) = (bridge_pieces(&s, None), bridge_pieces(&s, Some(q)));
        if own.is_empty() {
            no_bridge += 1;
            eprintln!("[crank {i}] no piece of its quality");
            continue;
        }
        let cell = (x as i16, y as i16);
        let n = lines_of(&s).len();
        let e = use_on(s, &files, crank, cell, &[]);
        let down = bridge_line(&e, &report(&e, &format!("11-crank-{i}-turn"), n));
        let s = e.state().clone();
        let turned = bridge_pieces(&s, Some(q));
        let n = lines_of(&s).len();
        let e = use_on(s, &files, crank, cell, &[]);
        let up = bridge_line(&e, &report(&e, &format!("11-crank-{i}-back"), n));
        let back = bridge_pieces(&e.state(), None);
        let changed = !turned.is_empty() && turned != own && down == 2;
        let restored = back == all && up == 1;
        eprintln!(
            "[crank {i}] turn line {down}: {}, back line {up}: {}",
            pass(changed),
            pass(restored)
        );
        if changed && restored {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    eprintln!(
        "[cranks] {}: {passed} pass, {failed} fail, {no_bridge} without a bridge",
        cranks.len()
    );
    assert!(passed > 0, "a crank turns its bridge");
    assert!(failed == 0, "every crank with a bridge turns it and back");
}
