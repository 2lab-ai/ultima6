//! Unit tests on synthetic data for the scenes of this unit: the storm's clouds and rain, the
//! composite overlays, the story's movement stepper, the transfer's readers and converters,
//! main's argument check and the VBlank server's pointer sprite.

use std::cell::RefCell;
use std::rc::Rc;

use crate::engine::{Engine, Io};
use crate::files::Files;
use crate::intro::exe::{A4, UExe};
use crate::intro::mem::{self, Chip, Ptr, Region, View};
use crate::intro::state::IntroState;
use crate::intro::story::Mover;
use crate::intro::{IntroOutcome, storm, transfer, video::Video};
use crate::state::State;

/// Bytes of the synthetic image: up to the end of u's last hunk (0x11a010).
const IMAGE: usize = 0x1_a100;

fn exe() -> UExe {
    UExe::from_flat(vec![0; IMAGE])
}

fn state() -> IntroState {
    let mut st = IntroState::new(exe(), Files::default());
    st.views.draw = View::screen(Ptr::at(Region::Screen1));
    st
}

/// The next `n` values of the C `rand` from the state's current seed (without advancing it).
fn rands(st: &IntroState, n: usize) -> Vec<i16> {
    let mut copy = st.clone();
    (0..n).map(|_| copy.rand()).collect()
}

#[test]
fn a_movement_steps_every_coordinate_along_one_line_and_then_restarts() {
    let mut m = Mover::new(1);
    let mut cur = [0i16, 0];
    let target = [3i16, -6];
    let mut steps = 0;
    let mut path = Vec::new();
    while m.step(&mut cur, &target) {
        steps += 1;
        path.push(cur);
    }
    assert_eq!(steps, 6, "the longest distance");
    assert_eq!(cur, target);
    // Error terms start at half the length (3): y steps every frame, x every other.
    assert_eq!(path[0], [0, -1]);
    assert_eq!(path[1], [1, -2]);
    assert_eq!(path[5], [3, -6]);
    assert_eq!(m.start, 1, "the next call starts a new movement");
    let mut same = [5i16];
    assert!(!m.step(&mut same, &[5]), "nothing to move ends at once");
    assert_eq!(m.start, 1);
}

#[test]
fn ultima_iv_attributes_are_compressed_into_ultima_vi_range() {
    let map = transfer::ultima_iv_attribute;
    assert_eq!(
        [map(0), map(9), map(10), map(11), map(29)],
        [0, 9, 10, 11, 20]
    );
    assert_eq!(
        [map(30), map(33), map(34), map(50), map(99)],
        [20, 20, 21, 25, 37]
    );
}

/// Pokes a NUL-terminated string into the synthetic image.
fn poke_str(e: &mut UExe, addr: u32, s: &[u8]) {
    let mut b = s.to_vec();
    b.push(0);
    e.poke(addr, &b);
}

fn transfer_state() -> IntroState {
    let mut e = exe();
    poke_str(&mut e, 0x0010_99a2, b"Mm");
    poke_str(&mut e, 0x0010_99a8, b"Ff");
    for (i, c) in [&b"Cl0"[..], b"Cl1", b"Cl2"].iter().enumerate() {
        poke_str(&mut e, A4 + 0x94b + 9 * i as u32, c);
    }
    poke_str(&mut e, A4 + 0x94b + 9 * 8, b"ClA");
    poke_str(&mut e, A4 + 0x993, b"ClA");
    IntroState::new(e, Files::default())
}

#[test]
fn an_ultima_iv_roster_maps_into_the_creation_globals() {
    let mut st = transfer_state();
    let mut r = vec![0u8; 0x200];
    r[..5].copy_from_slice(b"Abel\0");
    r[0x10] = 0x5c;
    r[0x11] = 1;
    r[0x13] = 50;
    r[0x14] = 25;
    r[0x15] = 9;
    r[0x16] = 7;
    r[0x18..0x1a].copy_from_slice(&350u16.to_be_bytes());
    r[0x1a..0x1c].copy_from_slice(&400u16.to_be_bytes());
    r[0x1c..0x1e].copy_from_slice(&2500u16.to_be_bytes());
    r[0x100..0x108].copy_from_slice(b"Quintus\0");
    st.transfer.save = r;
    let bad = transfer::read_ultima_iv(&mut st);
    assert!(!bad);
    let f = st.transfer.found.clone();
    assert_eq!(f.name_str(), b"Abel");
    assert_eq!((f.str, f.dex, f.int, f.magic), (50, 25, 9, 7));
    assert_eq!((f.hp, f.max_hp, f.exp, f.level), (350, 400, 2500, 4));
    assert_eq!(
        (f.sex.as_slice(), f.class.as_slice()),
        (&b"Mm"[..], &b"Cl1"[..])
    );
    assert_eq!(f.avatar, 0);
    transfer::convert_ultima_iv(&mut st);
    let h = &st.hero;
    assert_eq!(h.name_str(), b"Abel");
    assert_eq!((h.str, h.dex, h.int), (25, 18, 9), "0x109ba8 on each");
    // 2500 / 1000 = 2: two halvings to 0, level 3; experience / 10; 30 hit points a level.
    assert_eq!(
        (h.exp, h.level, h.hp, h.max_hp, h.magic),
        (250, 3, 90, 90, 9)
    );
    assert_eq!(
        (h.class.as_slice(), h.status, h.transferred),
        (&b"ClA"[..], b'G', 1)
    );
}

#[test]
fn an_ultima_iv_avatar_record_becomes_class_8_and_reads_as_out_of_range() {
    let mut st = transfer_state();
    let mut r = vec![0u8; 0x200];
    r[..6].copy_from_slice(b"Zhuge\0");
    r[0x10] = 0x5d;
    r[0x13] = 20;
    st.transfer.save = r;
    let bad = transfer::read_ultima_iv(&mut st);
    let f = &st.transfer.found;
    assert_eq!((f.avatar, f.class.as_slice()), (1, &b"ClA"[..]));
    assert_eq!(st.transfer.save[0x11], 8, "written back into the roster");
    assert!(bad, "class 8 > 7");
    assert_eq!(f.sex, b"Ff");
}

#[test]
fn an_ultima_v_save_maps_its_first_character_unchanged() {
    let mut st = transfer_state();
    let mut b = vec![0u8; 0x40];
    b[2..7].copy_from_slice(b"Corin");
    b[2 + 9] = 0xc;
    b[2 + 0xc] = 31;
    b[2 + 0xd] = 12;
    b[2 + 0xe] = 15;
    b[2 + 0x14..2 + 0x16].copy_from_slice(&999u16.to_be_bytes());
    st.transfer.save = b;
    assert!(transfer::read_ultima_v(&mut st), "strength 31 > 30");
    let f = &st.transfer.found;
    assert_eq!(
        (f.name_str().as_slice(), f.sex.as_slice()),
        (&b"Corin"[..], &b"Ff"[..])
    );
    transfer::convert_ultima_v(&mut st);
    let h = &st.hero;
    assert_eq!(
        (h.str, h.dex, h.int, h.exp, h.level, h.hp),
        (31, 12, 15, 99, 1, 30)
    );
}

#[test]
fn the_composite_is_overlaid_where_its_planes_are_not_zero() {
    let mut chip = Chip::new();
    let comp = Ptr::at(Region::Composite);
    let dst = Ptr::at(Region::Screen0);
    mem::fill_rect(&mut chip, dst, (0, 0), (320, 2), 0b10101);
    chip.set_pixel(comp, 3, 0, 0b00010);
    chip.set_pixel(comp, 11, 1, 0b01000);
    mem::composite_mask(&mut chip);
    mem::overlay_composite(&mut chip, Region::Screen0);
    assert_eq!(chip.pixel(dst, 3, 0), 0b00010);
    assert_eq!(chip.pixel(dst, 4, 0), 0b10101, "colour 0 is see-through");
    assert_eq!(chip.pixel(dst, 11, 1), 0b01000);
    // Moved left by whole bytes: x = 11 lands on 3, and nothing for x = 320.
    mem::fill_rect(&mut chip, dst, (0, 0), (320, 2), 0b10101);
    mem::overlay_composite_left(&mut chip, Region::Screen0, 8 + 7);
    assert_eq!(chip.pixel(dst, 3, 1), 0b01000);
    assert_eq!(chip.pixel(dst, 3, 0), 0b10101);
    mem::overlay_composite_left(&mut chip, Region::Screen0, 320);
    assert_eq!(chip.pixel(dst, 3, 0), 0b10101);
}

#[test]
fn clouds_start_left_of_the_view_and_come_back_when_they_leave_it() {
    let mut st = state();
    st.srand(7);
    let r = rands(&st, 3);
    storm::clouds_init(&mut st, 1, Ptr::at(Region::Files));
    let c = &st.storm.clouds;
    assert_eq!(c.x[0], r[0] % 0x13f - 0x104 - 0x8c);
    assert_eq!((c.y[0], c.item[0]), (r[1] % 0x1e, (r[2] & 1) + 2));
    assert_eq!((c.bank_x, c.bank_y), (-400, -5));
    st.storm.clouds.speed = 4;
    st.storm.clouds.x[0] = 0x140;
    let r = rands(&st, 3);
    storm::clouds_step(&mut st);
    let c = &st.storm.clouds;
    assert_eq!(
        c.x[0],
        r[0] % 0x13f - 0x140 - 0x8c + 4,
        "placed again, then moved"
    );
    assert_eq!(c.bank_x, -398, "the bank moves half the speed");
    st.storm.clouds.bank_x = 0x13e;
    storm::clouds_step(&mut st);
    assert_eq!(st.storm.clouds.bank_x, 0, "wraps at 320");
}

#[test]
fn rain_drops_start_at_the_top_fall_and_start_again_below_the_view() {
    let mut st = state();
    st.srand(3);
    storm::rain_init(&mut st, 3, 1);
    st.storm.rain.chance = 1;
    let r = rands(&st, 4);
    storm::rain_step(&mut st);
    let d = &st.storm.rain;
    assert_eq!(d.count, 1);
    assert_eq!(d.item[0], ((r[1] & 3) + 4) as u8);
    assert_eq!(d.x[0], r[2] % (0x13f + 0x15) - 0xa);
    assert_eq!(d.y[0], r[3] % 9 - 4);
    assert_eq!(d.wait[0], 0, "waited its frame in the same call");
    let (x, y) = (d.x[0], d.y[0]);
    st.storm.rain.max = 1;
    storm::rain_step(&mut st);
    let d = &st.storm.rain;
    assert_eq!(d.count, 1, "count = max: no new drop");
    assert_eq!((d.x[0], d.y[0], d.wait[0]), (x + 2, y + 10, 1));
    st.storm.rain.wait[0] = 0;
    st.storm.rain.y[0] = 0xc7;
    let r = rands(&st, 3);
    storm::rain_step(&mut st);
    let d = &st.storm.rain;
    assert_eq!(
        (d.y[0], d.x[0]),
        (r[0] % 9 - 4, r[1] % (0x13f + 0x15) - 0xa)
    );
    assert_eq!(d.item[0], ((r[2] & 3) + 4) as u8);
}

/// An image with a 2-row pointer at u 0x11432e and the mouse at (10, 20).
fn pointer_exe() -> UExe {
    let mut e = exe();
    e.poke(
        0x0011_432e,
        &[0x80, 0x00, 0x00, 0x00, 0x40, 0x00, 0x40, 0x00],
    );
    e.poke(0x0011_9e16, &[0, 10, 0, 20]);
    e.poke(0x0011_9fe8, &[0xff, 0xff]);
    e
}

#[test]
fn the_pointer_sprite_follows_its_control_words_and_hides_below_the_screen() {
    let e = pointer_exe();
    let mut v = Video::new(&e);
    assert_eq!(v.sprite_rows, vec![(0x8000, 0), (0x4000, 0x4000)]);
    v.server();
    assert!(
        v.pointer_pixels().is_empty(),
        "off: y 0xc9, below the window"
    );
    assert_eq!(
        v.beamcon0, None,
        "BEAMCON0 not written while the word is negative"
    );
    v.pointer = 1;
    v.server();
    // HSTART 10 + 0x80, VSTART 20 + 0x2c: colour 1 at the sprite's column 0, colour 3 below.
    assert_eq!(v.pointer_pixels(), vec![(10, 20, 17), (11, 21, 19)]);
    v.beamcon = 0x20;
    v.server();
    assert_eq!(v.beamcon0, Some(0x20));
}

#[test]
fn main_without_the_launcher_argument_exits_before_any_setup() {
    let mut e = exe();
    poke_str(&mut e, 0x0010_2c12, b"JJS");
    for args in [&[&b"u"[..]][..], &[b"u", b"JJ"], &[b"u", b"JJS", b"x"]] {
        let out: Rc<RefCell<Option<(IntroOutcome, u64)>>> = Rc::new(RefCell::new(None));
        let (o, exe) = (Rc::clone(&out), e.clone());
        let args: Vec<Vec<u8>> = args.iter().map(|a| a.to_vec()).collect();
        let mut eng = Engine::with_game(
            State::empty(),
            Files::default(),
            move |_s, io: Rc<Io>| async move {
                let mut st = IntroState::new(exe, Files::default());
                let a: Vec<&[u8]> = args.iter().map(Vec::as_slice).collect();
                let r = crate::intro::main(&mut st, &io, 1, &a).await;
                *o.borrow_mut() = Some((r, st.video.frames));
            },
        );
        eng.tick(&[], 20);
        assert_eq!(*out.borrow(), Some((IntroOutcome::Quit, 0)));
    }
}
