//! Real data: Use on objects of the shipped world, from the user's archive.org zip. Skips where
//! the zip is missing; prints the message lines only (no game text in the repo). With
//! `U6_USE_FRAMES=<dir>` each case also writes the frame after the command as a PNG there.
#![allow(clippy::disallowed_methods)] // tests may read files and the environment

use std::path::Path;

use u6_intake::{InputFile, intake};

use super::use_command;
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::objects::Slot;
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

fn start(files: &Files) -> State {
    let mut s = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
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
    let mut last: String = t.transcript_line.iter().map(|&b| char::from(b)).collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| char::from(b)));
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
            let s = st.borrow();
            crate::main_loop::draw_paper(&s, &io);
        }
        crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
        crate::ui::panels::redraw_panel(&st, &io).await;
        use_command(&st, &io).await;
        crate::main_loop::flush_prints(&st, &io).await;
        crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
        io.present();
    });
    let mut queued = keys.iter();
    for _ in 0..400 {
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
    assert!(
        e.finished(),
        "Use did not finish: {:?}",
        lines_of(&e.state())
    );
    e
}

fn save_frame(e: &Engine, name: &str) {
    let Ok(dir) = std::env::var("U6_USE_FRAMES") else {
        return;
    };
    let mut rgba = Vec::new();
    e.frame().write_rgba(&mut rgba);
    let path = Path::new(&dir).join(format!("{name}.png"));
    std::fs::write(&path, png(320, 200, &rgba)).unwrap();
    eprintln!("frame: {}", path.display());
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

/// Prints the lines the command added and the stub points it reached; writes its frame.
fn report(e: &Engine, name: &str, before: usize) -> Vec<String> {
    let lines = lines_of(&e.state());
    let new: Vec<String> = lines
        .into_iter()
        .skip(before.saturating_sub(1))
        .filter(|l| !l.trim().is_empty())
        .collect();
    eprintln!("[{name}] {new:?}");
    eprintln!(
        "[{name}] stubs: {:?}",
        e.stub_hits().keys().collect::<Vec<_>>()
    );
    save_frame(e, name);
    new
}

#[test]
fn where_the_use_objects_are() {
    let Some(files) = files() else { return };
    let s = start(&files);
    let c = s.map.centre;
    eprintln!("start {:?} leader {:?}", c, s.actors.party.leader());
    for kind in [
        0x62, 0x5a, 0x7a, 0x81, 0x10c, 0x12c, 0x12a, 0x19c, 0x19e, 0x57, 0x9a, 0xdf, 0x13c,
    ] {
        let r = records(&s, kind, c.x, c.y, c.z);
        eprintln!(
            "{kind:#x}: {} on this level, nearest {:?}",
            r.len(),
            r.first()
        );
    }
    let lead = s.actors.party.leader().unwrap().slot();
    let mut o = s.objects.clone();
    let mut k = crate::objects::first_child(&mut o, lead);
    let mut inv = Vec::new();
    while k >= 0 {
        inv.push((k, o.kind(k), o.place(k)));
        k = crate::objects::next_child(&mut o);
    }
    eprintln!("leader carries {inv:x?}");
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
    assert!(slot >= 0, "no {kind:#x} at ({x}, {y})");
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
    assert_eq!(e.state().objects.kind(chest), 0x62);
    assert_eq!(frame_of(&e, chest), 0, "the chest is open");

    // 2. A closed door (0x12c, frames 4-7) opens; used again it closes.
    let pos = nearest(&files, 0x12c, Some(5)).expect("a closed door");
    let (s, door) = at(&files, 0x12c, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, door, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "2-door-open", n);
    assert_eq!(frame_of(&e, door), 1, "frame 5 ^ 4: open");
    let s = e.state().clone();
    let n = lines_of(&s).len();
    let e = use_on(s, &files, door, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "2-door-close", n);
    assert_eq!(frame_of(&e, door), 5, "closed again");

    // 3. A lever (0x10c) flips its frame bit 0 and works the portcullises of its quality.
    let pos = nearest(&files, 0x10c, None).expect("a lever");
    let (s, lever) = at(&files, 0x10c, pos);
    let f0 = s.objects.tf(lever) >> 10;
    let gates = |s: &State| {
        s.objects
            .type_frame
            .iter()
            .filter(|&&w| w & 0x3ff == 0x136)
            .count()
    };
    let g0 = gates(&s);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, lever, (pos.0 as i16, pos.1 as i16), &[]);
    let out = report(&e, "3-lever", n);
    assert_eq!(frame_of(&e, lever), f0 ^ 1);
    eprintln!("[3-lever] portcullises {g0} -> {}", gates(&e.state()));
    assert!(!out.is_empty());

    // 4. A lit candle (0x7a frame 1) is doused, then lit again.
    let pos = nearest(&files, 0x7a, Some(1)).expect("a lit candle");
    let (s, candle) = at(&files, 0x7a, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, candle, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "4-candle-douse", n);
    assert_eq!(frame_of(&e, candle), 0);
    let s = e.state().clone();
    let n = lines_of(&s).len();
    let e = use_on(s, &files, candle, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "4-candle-light", n);
    assert_eq!(frame_of(&e, candle), 1);

    // 5. A torch (0x5a) on the ground is lit (status 0x80, frame 1, 200 turns); used again it
    // burns out (goes).
    let pos = nearest(&files, 0x5a, Some(0)).expect("a torch");
    let (s, torch) = at(&files, 0x5a, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, torch, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "5-torch-light", n);
    {
        let s = e.state();
        assert_eq!(s.objects.obj_status[torch as usize] & 0x80, 0x80);
        assert_eq!(s.objects.tf(torch), 0x5a | 1 << 10);
        assert_eq!(s.objects.qq_of(torch) & 0xff, 200);
    }
    let s = e.state().clone();
    let n = lines_of(&s).len();
    let e = use_on(s, &files, torch, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "5-torch-out", n);
    assert_eq!(e.state().objects.tf(torch), 0, "the torch slot is free");

    // 6. Food (0x81) is eaten: one fewer, or gone.
    let pos = nearest(&files, 0x81, None).expect("food");
    let (s, food) = at(&files, 0x81, pos);
    let q0 = s.objects.qq_of(food) & 0xff;
    let n = lines_of(&s).len();
    let e = use_on(s, &files, food, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "6-food", n);
    let after = e.state().objects.tf(food);
    if q0 > 1 && after != 0 {
        assert_eq!(e.state().objects.qq_of(food) & 0xff, q0 - 1);
    } else {
        assert_eq!(after, 0, "eaten");
    }

    // 7. Board a ship (0x19c), then leave it.
    let pos = nearest(&files, 0x19c, None).expect("a ship");
    let (s, ship) = at(&files, 0x19c, pos);
    let n = lines_of(&s).len();
    let e = use_on(s, &files, ship, (pos.0 as i16, pos.1 as i16), &[]);
    report(&e, "7-ship-board", n);
    let s = e.state().clone();
    assert_eq!(
        s.actors.party.leader_index, s.actors.party.size,
        "aboard: the leader index is the party size"
    );
    assert_eq!(s.objects.kind(0), 0x19c, "actor 0 is the ship");
    let c = s.map.centre;
    let n = lines_of(&s).len();
    let e = use_on(s, &files, 0, (c.x as i16, c.y as i16), &[]);
    let out = report(&e, "7-ship-leave", n);
    let s = e.state();
    eprintln!(
        "[7-ship-leave] leader index {} size {} actor0 type {:#x} lines {out:?}",
        s.actors.party.leader_index,
        s.actors.party.size,
        s.objects.kind(0)
    );

    // 8. The orb of the moons (0x57, carried by the Avatar). At the start nobody has told the
    // Avatar how to use it (actor 5's talk flag bit 5, set in Lord British's conversation):
    // "You can't figure out how to use it.". With the flag set it makes a red moongate (0x54)
    // two tiles north.
    for told in [false, true] {
        let mut s = start(&files);
        let me = lead(&s);
        let mut orb = crate::objects::first_child(&mut s.objects, me);
        while orb >= 0 && s.objects.kind(orb) != 0x57 {
            orb = crate::objects::next_child(&mut s.objects);
        }
        assert!(orb >= 0, "the Avatar carries the orb");
        assert_eq!(
            s.actors.attrs.talk_flags[5] & 0x20,
            0,
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
        let gates0 = gates(&s);
        s.ui.input.targeting = 1;
        s.ui.input.cursor = (5, 5);
        s.ui.input.range = 7;
        let c = s.map.centre;
        let n = lines_of(&s).len();
        let up = InputEvent::Key(Key::Up);
        let keys = [up, up, InputEvent::Key(Key::Enter)];
        let e = use_on(s, &files, orb, (-1, -1), &keys);
        let name = if told { "8-orb-told" } else { "8-orb-start" };
        report(&e, name, n);
        let gates1 = gates(&e.state());
        eprintln!("[{name}] red gates {gates0:?} -> {gates1:?}");
        if told {
            assert_eq!(gates1.len(), gates0.len() + 1, "a moongate appeared");
            let g = *gates1.iter().find(|g| !gates0.contains(g)).unwrap();
            let s = e.state();
            assert_eq!((s.objects.x(g), s.objects.y(g)), (c.x, c.y - 2));
            assert_eq!(
                s.objects.qq_of(g) >> 8,
                3,
                "quality = (dy + 2) * 5 + dx + 3"
            );
        } else {
            assert_eq!(gates1, gates0);
        }
    }
}
