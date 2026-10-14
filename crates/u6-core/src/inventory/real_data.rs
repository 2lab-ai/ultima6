//! Real data: the party's inventory at the start, Ready / Unready of a weapon and armour, the
//! inventory and portrait panels rendered headless, and an eight-hour Rest in the wilderness,
//! from the user's archive.org zip. Skips where the zip is missing; the data is read only, never
//! copied; receipts (PNG + raw palette indices) go to /tmp/claude-501/m2-inventory/. The output
//! names slots, types and counts only: no names or lines of the game's text.
#![allow(clippy::disallowed_methods)] // the test reads the zip and writes the receipts

use std::cell::RefCell;
use std::rc::Rc;

use u6_intake::{InputFile, intake};
use u6_render::Framebuffer;

use super::*;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::image::GameImage;
use crate::objects::{READIED, Slot, display_name, first_child, next_child};
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
const OUT: &str = "/tmp/claude-501/m2-inventory";

fn files() -> Option<Files> {
    if !std::path::Path::new(ZIP).exists() {
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
    s
}

fn run<G, F>(s: State, files: &Files, keys: &[Key], game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: std::future::Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, files.clone(), game);
    let ev: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
    e.tick(&ev, 16);
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "the game task ended");
    e
}

/// Writes the frame as a PNG (RGB, stored zlib blocks) and its palette indices (raw 320 × 200).
fn receipt(fb: &Framebuffer, name: &str) {
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
    std::fs::create_dir_all(OUT).unwrap();
    std::fs::write(format!("{OUT}/{name}.png"), out).unwrap();
    std::fs::write(format!("{OUT}/{name}.idx"), fb.pixels()).unwrap();
    println!("receipt {OUT}/{name}.png");
}

/// The palette of the user's `game` into the screen (as the platform's first frame has it).
fn palette(st: &RefCell<State>, io: &Io) {
    io.draw(|fb| {
        for (dst, c) in fb.palette.iter_mut().zip(&st.borrow().tables.palette) {
            *dst = u6_render::Rgb::new(c.r, c.g, c.b);
        }
    });
}

/// The panel of `mode` for member `member` with the prompt, as one command-loop iteration
/// draws it before its key wait.
fn panel(s: State, files: &Files, mode: u16, member: i16) -> Engine {
    let mut s = s;
    s.ui.panel.mode = mode;
    s.ui.panel.member = member;
    s.ui.panel.dirty = true;
    let prompt = GameImage::parse(files.get("game").unwrap())
        .unwrap()
        .cstr(crate::ui::PROMPT)
        .unwrap();
    run(s, files, &[], move |st, io| async move {
        crate::ui::debug_render(&st, &io, &prompt).await;
    })
}

/// The objects directly inside `actor`: (slot, type, place); each has a name (not printed).
fn contents(s: &mut State, actor: Slot) -> Vec<(Slot, u16, u8)> {
    let mut v = Vec::new();
    let mut c = first_child(&mut s.objects, actor);
    while c >= 0 {
        assert!(!display_name(s, c).is_empty(), "object {c:#05x} has a name");
        v.push((c, s.objects.kind(c), s.objects.place(c)));
        c = next_child(&mut s.objects);
    }
    v
}

fn weights(s: &State) -> (i16, i16) {
    (s.inventory.equip_weight, s.inventory.carried_weight)
}

/// Unready, then Ready again, `obj` in equipment slot `slot` of the member the panel shows;
/// returns the state after, with the weights before / unreadied / readied again.
fn unready_ready(files: &Files, mut s: State, obj: Slot, slot: i16, tag: &str) -> State {
    let before = weights(&s);
    s.ui.input.selection = obj;
    let e = run(s, files, &[], |st, io| async move {
        palette(&st, &io);
        st.borrow_mut().ui.panel.dirty = true;
        crate::ui::panels::redraw_panel(&st, &io).await;
        unready(&st, &io).await;
        crate::ui::print::put_raw(&st, &io, -1).await;
        io.present();
    });
    let mut s = e.state().clone();
    let off = weights(&s);
    assert_eq!(
        s.objects.place(obj),
        crate::objects::IN_INVENTORY,
        "{obj:#05x}"
    );
    assert_eq!(s.inventory.equip(slot), 0);
    println!("Unready object {obj:#05x} (slot {slot}): equip/carried tenths {before:?} -> {off:?}");
    receipt(e.frame(), &format!("unready-{tag}"));
    s.ui.input.selection = obj;
    let e = run(s, files, &[], |st, io| async move {
        ready(&st, &io).await;
    });
    let s = e.state().clone();
    assert_eq!(s.objects.place(obj), READIED, "{obj:#05x} readied again");
    assert_eq!(s.inventory.equip(slot), obj);
    assert_eq!(weights(&s), before);
    println!(
        "Ready object {obj:#05x}: equip/carried tenths {off:?} -> {:?}",
        weights(&s)
    );
    s
}

fn list(s: &mut State, member: usize) {
    let actor = Slot::from(s.actors.party.members[member]);
    assert!(
        !s.actors.party.names[member].is_empty(),
        "member {member} has a name"
    );
    println!(
        "member {member} = actor {actor}, strength {}",
        s.actors.attrs.strength[actor as usize]
    );
    for (c, ty, place) in contents(s, actor) {
        println!(
            "  {c:#05x} type {ty:#05x} {} slot {:2} weight {:3}",
            if place == READIED {
                "readied"
            } else {
                "carried"
            },
            ready_slot(s, c),
            weight::object_weight(s, c)
        );
    }
}

#[test]
fn inventory_ready_unready_and_panels() {
    let Some(files) = files() else { return };
    let mut s = start(&files);
    list(&mut s, 0);
    list(&mut s, 1);
    // The Avatar's inventory panel (F1 in the roster), then the weapon off and on.
    let e = panel(s, &files, 0x92, 0);
    receipt(e.frame(), "inventory-member0");
    let s = e.state().clone();
    println!(
        "member 0 panel: equip/carried tenths {:?}, list {} objects, cells {:?}; stubs {:?}",
        weights(&s),
        s.ui.panel.inventory_count,
        s.inventory.cells,
        e.stub_hits().keys().collect::<Vec<_>>()
    );
    let sword = s.inventory.equip(2);
    assert!(sword > 1, "member 0 starts with a weapon in hand");
    let s = unready_ready(&files, s, sword, 2, "member0-weapon");
    // Member 1's panel (F2) and the armour (slot 4) off and on.
    let e = panel(s, &files, 0x92, 1);
    receipt(e.frame(), "inventory-member1");
    let s = e.state().clone();
    println!(
        "member 1 panel: equip/carried tenths {:?}, list {} objects, equipment {:?}",
        weights(&s),
        s.ui.panel.inventory_count,
        &s.inventory.cells[EQUIP..]
    );
    let armour = s.inventory.equip(4);
    assert!(armour > 1, "member 1 starts in armour");
    let s = unready_ready(&files, s, armour, 4, "member1-armour");
    println!("scroll: {} lines", s.ui.messages.len());

    // The portrait / equipment panel 0x128ce8 of member 1 from the roster, as Look shows it (one
    // key ends it).
    let mut s = s;
    s.ui.panel.mode = 0x91;
    let member1 = Slot::from(s.actors.party.members[1]);
    let e = run(s, &files, &[Key::Space], move |st, io| async move {
        palette(&st, &io);
        show_actor(&st, &io, member1).await;
        io.present();
    });
    receipt(e.frame(), "portrait-member1");
    let mut s = e.state().clone();
    assert_eq!(s.ui.panel.mode, 0x91, "the panel mode comes back");
    println!("stubs hit: {:?}", e.stub_hits().keys().collect::<Vec<_>>());
    // The Avatar's, as Look at the Avatar shows it, with the oracle's game-first patch of the
    // avatar portrait byte (objlist 0x1C72 = 1, re/notes/oracle.md §D) so the two compare.
    s.actors.objlist_tail[0x81] = 1;
    let e = run(s, &files, &[Key::Space], move |st, io| async move {
        palette(&st, &io);
        show_actor(&st, &io, 1).await;
        io.present();
    });
    receipt(e.frame(), "portrait-actor1");
}

/// Finds a wilderness spot near the start where Rest has nothing to refuse: a 16-tile grid of
/// surface places around the leader, nearest rings first.
fn camp_site(files: &Files) -> Option<State> {
    let base = start(files);
    let c = base.map.centre;
    let mut spots: Vec<(i32, u16, u16)> = Vec::new();
    for dy in -8..=8i32 {
        for dx in -8..=8i32 {
            let (x, y) = (i32::from(c.x) + 16 * dx, i32::from(c.y) + 16 * dy);
            if (0..1024).contains(&x) && (0..1024).contains(&y) && (dx, dy) != (0, 0) {
                spots.push((dx.abs().max(dy.abs()), x as u16, y as u16));
            }
        }
    }
    spots.sort_unstable();
    for (ring, x, y) in spots {
        let mut s = base.clone();
        crate::map::travel::teleport(&mut s, x, y, 0, 0);
        s.main.pending_print.clear();
        let mut t = s.clone();
        let refused = crate::actors::rest::not_wilderness(&t)
            || crate::actors::rest::foes_near(&t)
            || crate::actors::rest::others_near(&t)
            || crate::actors::rest::no_room(&mut t);
        if !refused {
            println!("camp site {ring} rings of 16 tiles from the start");
            return Some(s);
        }
    }
    None
}

#[test]
fn rest_eight_hours_in_the_wilderness() {
    let Some(files) = files() else { return };
    let Some(mut s) = camp_site(&files) else {
        panic!("no camp site among the candidates");
    };
    // Wounds to heal, and the rest counter at 0 (a full rest heals).
    let n = usize::from(s.actors.party.size);
    for i in 0..n {
        let a = usize::from(s.actors.party.members[i]);
        s.actors.attrs.hp[a] = s.actors.attrs.hp[a].saturating_sub(20).max(1);
    }
    s.actors.rest_counter = 0;
    s.actors.set_tail(crate::actors::tail::COMBAT_MODE, 0);
    let hp = |s: &State| -> Vec<u8> {
        (0..n)
            .map(|i| s.actors.attrs.hp[usize::from(s.actors.party.members[i])])
            .collect()
    };
    let max: Vec<u16> = (0..n)
        .map(|i| crate::actors::leaves::max_hp(&s, u16::from(s.actors.party.members[i])))
        .collect();
    println!("max hp {max:?}");
    let before = (hp(&s), s.actors.calendar);
    println!("before: hp {:?}, clock {:?}", before.0, before.1);
    // R, 8 hours, no guard (space).
    let e = run(
        s,
        &files,
        &[Key::Char('8'), Key::Space],
        |st, io| async move {
            palette(&st, &io);
            crate::actors::rest::rest(&st, &io).await;
            crate::main_loop::flush_prints(&st, &io).await;
            crate::ui::print::put_raw(&st, &io, -1).await;
            io.present();
        },
    );
    receipt(e.frame(), "rest-8h");
    let s = e.state();
    let after = (hp(&s), s.actors.calendar);
    println!("after:  hp {:?}, clock {:?}", after.0, after.1);
    println!("scroll: {} lines", s.ui.messages.len());
    println!("stubs hit: {:?}", e.stub_hits());
    let minutes = |c: crate::actors::Calendar| {
        (u32::from(c.day) * 24 + u32::from(c.hour)) * 60 + u32::from(c.minute)
    };
    let slept = minutes(after.1) - minutes(before.1);
    assert!(slept <= 8 * 60, "at most eight hours ({slept} minutes)");
    if slept == 8 * 60 {
        assert!(after.0.iter().zip(&before.0).all(|(a, b)| a >= b));
        assert!(
            after.0.iter().zip(&before.0).any(|(a, b)| a > b),
            "someone healed"
        );
    }
}
