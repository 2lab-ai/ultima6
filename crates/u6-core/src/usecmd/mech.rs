//! Levers, switches, the drawbridge, kegs, the cannon, shovels and moonstones (0x12d096..
//! 0x12e77c, 0x12f1e6).

use super::defs::{self, MSG_FAILED, MSG_NO_EFFECT};
use super::dispatch::FMT_NL_LINE;
use super::{Ux, leader, seams, set_frame};
use crate::objects::{NONE, Slot, object_of_kind_at};
use crate::state::State;
use crate::todo_stub;

const STR_BURIED: u32 = 0x0012_c91c;
const STR_CANT_BURY: u32 = 0x0012_c926;
const STR_FIRE: u32 = 0x0012_c9f2;
const STR_NOT_NOW: u32 = 0x0012_c9fa;
const STR_POWDER_LIT: u32 = 0x0012_ca04;
const STR_DOOR_BLOWN: u32 = 0x0012_ca12;
const STR_NO_BRIDGE: u32 = 0x0012_ca2a;
const STR_LEVER: u32 = 0x0012_cad2;
const STR_STRANGE: u32 = 0x0012_cae6;
const STR_NOISE: u32 = 0x0012_cb02;
const STR_CANT_CLOSE: u32 = 0x0012_cb16;
const STR_SWITCH: u32 = 0x0012_cb34;
const STR_DIRECTION: u32 = 0x0012_cd00;
const STR_NOWHERE: u32 = 0x0012_cd0c;
const FMT_DIR: u32 = 0x0012_cd16; // "%s.\n"
const STR_HOLE: u32 = 0x0012_cd1c;
const STR_FOUNTAIN: u32 = 0x0012_cd2e;
const STR_NUGGET: u32 = 0x0012_cd4c;

/// Objlist block index of A4-0x7b5a (8 moonstone locations, 3 words each).
const MOONSTONES: usize = 0x2a;

/// printf of a game string from synchronous code (queued for the next await point).
fn queue(s: &mut State, fmt: u32) {
    let text = crate::engine::with_current_io(|io| crate::main_loop::game_string(io, fmt));
    crate::main_loop::queue_print(s, text.unwrap_or_default(), Vec::new());
}

/// 0x109e74 then 0x102ef0 from synchronous code.
fn redraw(s: &mut State) {
    crate::map::view::update_view(s);
    crate::engine::with_current_io(|io| crate::gfx::refresh_view(s, io));
}

fn z(s: &State) -> u16 {
    u16::from(s.map.centre.z)
}

/// Original: 0x12d096 (objects: bury a moonstone where the leader stands)
///
/// On a grass or swamp tile (1..7, 0x10..0x6f) "buried.": its location goes into the moonstone
/// table (A4-0x7b5a + 6 × frame: x, y, z), the stone onto the tile and the leader on top of it.
pub async fn bury(ux: Ux<'_>, obj: Slot) {
    let (lx, ly, z, tile) = {
        let s = ux.s();
        let l = Slot::from(leader(&s));
        let (x, y, z) = (s.objects.x(l), s.objects.y(l), z(&s));
        let t = s
            .map
            .tile_at(usize::from(x), usize::from(y), usize::from(z));
        (x, y, z, t.unwrap_or(0))
    };
    if !(1..=7).contains(&tile) && !(0x10..=0x6f).contains(&tile) {
        ux.print(STR_CANT_BURY, &[]).await;
        return;
    }
    ux.print(STR_BURIED, &[]).await;
    let mut s = ux.s();
    let i = MOONSTONES + 6 * usize::from(s.objects.tf(obj) >> 10);
    if let Some(b) = s.actors.objlist_tail.get_mut(i..i + 6) {
        for (k, w) in [lx, ly, z].into_iter().enumerate() {
            b[2 * k..2 * k + 2].copy_from_slice(&w.to_be_bytes());
        }
    }
    let m = s.actors.party.members[(s.ui.panel.member as usize) & 0xf];
    seams::ready_changed(&mut s, Slot::from(m));
    crate::objects::move_to(&mut s, obj, lx, ly, z);
    let l = Slot::from(leader(&s));
    crate::objects::move_to(&mut s, l, lx, ly, z);
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

/// Original: 0x12d7d4 (objects: fire a cannon the way its frame points)
///
/// "Fire!"; the aim is the first actor 1..5 tiles out (direction from the frame, tables at
/// A4-0x661c / -0x6618), else 6 tiles; the ball (tile 0x18f) flies there (combat 0x1228ba,
/// range 5, leaving where it stopped in A4+0x185e / +0x1860). Unstopped, the explosion (0x1331b4,
/// tile 0x189) is one tile further; an object at the end that is not a counted kind takes
/// 1..60 damage (0x1231be).
pub async fn cannon(ux: Ux<'_>, obj: Slot) {
    ux.print(STR_FIRE, &[]).await;
    let mut s = ux.s();
    let f = usize::from(s.objects.tf(obj) >> 10);
    let dx = i16::from(defs::a4_sbyte(ux.io, defs::CANNON_DX, f));
    let dy = i16::from(defs::a4_sbyte(ux.io, defs::CANNON_DY, f));
    let (cx, cy, z) = (s.objects.x(obj) as i16, s.objects.y(obj) as i16, z(&s));
    let mut n = 1i16;
    while n < 6 {
        let (x, y) = (cx.wrapping_add(dx * n), cy.wrapping_add(dy * n));
        let a = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
        if (0..0x100).contains(&a) {
            break;
        }
        n += 1;
    }
    let (tx, ty) = (cx.wrapping_add(dx * n), cy.wrapping_add(dy * n));
    let (stopped, mut hx, mut hy) = seams::missile(&mut s, obj, 5, tx, ty, 0x18f);
    if stopped == 0 {
        hx = hx.wrapping_add(dx);
        hy = hy.wrapping_add(dy);
        seams::blast(&mut s, 0x189, hx, hy);
    }
    let a = crate::objects::begin_at(&mut s.objects, &s.tables, hx as u16, hy as u16, z);
    if a >= 0 && crate::objects::stack_kind(&s.objects, s.objects.kind(a)) == 0 {
        let d = s.rng.range(1, 0x3c) as i16;
        seams::damage(&mut s, a, d);
    }
}

/// Original: 0x12d960 (objects: light a powder keg: it explodes when the keg timer runs out)
///
/// Only an unlit keg (frame 0) while no other burns (A4-0x7b2a = 0): frame 1, timer 3, the keg
/// in A4-0x7b28, "Powder lit!"; a keg on the map redraws the view, a carried one the inventory.
pub async fn light_keg(ux: Ux<'_>, obj: Slot) {
    use crate::actors::tail::{KEG_OBJECT, KEG_TIMER};
    let (frame, timer) = {
        let s = ux.s();
        (s.objects.tf(obj) >> 10, s.actors.tail_word(KEG_TIMER))
    };
    if frame != 0 {
        ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
        return;
    }
    if timer != 0 {
        ux.print(STR_NOT_NOW, &[]).await;
        return;
    }
    {
        let mut s = ux.s();
        set_frame(&mut s, obj, 1);
        s.actors.set_tail_word(KEG_TIMER, 3);
        s.actors.set_tail_word(KEG_OBJECT, obj as u16);
    }
    ux.print(STR_POWDER_LIT, &[]).await;
    let mut s = ux.s();
    if s.objects.place(obj) == 0 {
        redraw(&mut s);
    } else {
        seams::redraw_inventory(&mut s);
    }
}

/// Original: 0x12db7e (objects: a powder keg explodes)
///
/// The keg goes; the explosion (0x1331b4, tile 0x189) centres on the keg's tile, or on the
/// tile of whoever carries it; everything it hit that is not a counted object takes 1..60
/// damage; kegs and doors around catch it (0x12d9ee); the view is redrawn.
pub fn explode_keg(s: &mut State, keg: Slot) {
    let mut at = keg;
    for _ in 0..crate::objects::OBJECT_SLOTS {
        if s.objects.place(at) == 0 {
            break;
        }
        at = s.objects.parent(at);
    }
    let (x, y) = (s.objects.x(at) as i16, s.objects.y(at) as i16);
    crate::objects::remove(s, keg);
    for hit in seams::blast(s, 0x189, x, y) {
        if crate::objects::stack_kind(&s.objects, s.objects.kind(hit)) == 0 {
            let d = s.rng.range(1, 0x3c) as i16;
            seams::damage(s, hit, d);
        }
    }
    blast_doors(s, x, y);
    redraw(s);
}

/// Original: 0x12d9ee (objects: blow up kegs and doors around (x, y))
///
/// For x − 2..=x + 2, y − 2..y + 2 (the original's loop stops before y + 2): a keg (0xdf) is lit
/// and explodes at once; the first door of 0x12c, 0x129, 0x12a, 0x12b there that is not
/// magically locked (frame bits 2-3 both set) is removed, "The door is blown up!".
pub fn blast_doors(s: &mut State, x: i16, y: i16) {
    for cx in x.wrapping_sub(2)..=x.wrapping_add(2) {
        for cy in y.wrapping_sub(2)..y.wrapping_add(2) {
            let (ux, uy, z) = (cx as u16, cy as u16, z(s));
            let keg = object_of_kind_at(s, ux, uy, z, 0xdf);
            if keg >= 0 {
                set_frame(s, keg, 1);
                redraw(s);
                explode_keg(s, keg);
            }
            let sealed = |s: &State, d: Slot| (s.objects.tf(d) >> 10) & 0xc == 0xc;
            let mut door = NONE;
            for kind in [0x12c, 0x129, 0x12a, 0x12b] {
                if door != -1 && !sealed(s, door) {
                    break;
                }
                door = object_of_kind_at(s, ux, uy, z, kind);
            }
            if door >= 0 && !sealed(s, door) {
                crate::objects::remove(s, door);
                redraw(s);
                queue(s, STR_DOOR_BLOWN);
            }
        }
    }
}

/// Original: 0x12dd6e (objects: the first actor at (x, y, z), 0 when none)
pub fn actor_at(s: &mut State, x: u16, y: u16, z: u16) -> Slot {
    let mut c = crate::objects::begin_at(&mut s.objects, &s.tables, x, y, z);
    while c > 0xff {
        c = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    if c == -1 { 0 } else { c }
}

/// Original: 0x12e538 (objects: switch a lever: open or close the portcullises it works)
///
/// The lever's frame bit 0 flips. With a quality q, every portcullis marker (0x12d of quality
/// q) on the level opens (its portcullis 0x136 removed) or closes (one created, frame 1, or 3
/// when the marker's frame has bit 1) unless an actor stands there ("can't close the
/// portcullis." ends the walk). "you hear a noise." when something moved, else "strange,
/// nothing happened.".
pub async fn lever(ux: Ux<'_>, obj: Slot) {
    ux.print(STR_LEVER, &[]).await;
    let outcome = {
        let mut s = ux.s();
        let f = s.objects.tf(obj) >> 10;
        set_frame(&mut s, obj, f ^ 1);
        operate(&mut s, obj, 0x136)
    };
    let msg = match outcome {
        0 => STR_STRANGE,
        1 => STR_NOISE,
        _ => STR_CANT_CLOSE,
    };
    ux.print(msg, &[]).await;
}

/// The marker walk of 0x12e538 (portcullis 0x136) and 0x12e77c (force field 0xaf): 0 nothing,
/// 1 moved, 2 blocked by an actor (lever only).
fn operate(s: &mut State, obj: Slot, gate: u16) -> u8 {
    let q = super::quality(s, obj);
    if q == 0 {
        return 0;
    }
    let mut moved = 0;
    let mut m = crate::objects::begin_area(s, 0, 0, 0x3ff, 0x3ff);
    while m >= 0 {
        while m >= 0 && (s.objects.kind(m) != 0x12d || super::quality(s, m) != q) {
            m = crate::objects::next_in_area(&mut s.objects);
        }
        if m < 0 {
            break;
        }
        let (x, y, z) = (s.objects.x(m), s.objects.y(m), z(s));
        let there = object_of_kind_at(s, x, y, z, gate);
        if there == -1 {
            let frame = if gate == 0x136 {
                if actor_at(s, x, y, z) != 0 {
                    return 2;
                }
                if (s.objects.tf(m) >> 10) & 2 != 0 { 3 } else { 1 }
            } else {
                0
            };
            let new = crate::objects::create_on_map(
                &mut s.objects,
                gate | frame << 10,
                0,
                0,
                x,
                y,
                z,
            );
            if let Some(st) = usize::try_from(new)
                .ok()
                .and_then(|i| s.objects.obj_status.get_mut(i))
            {
                *st &= !0x20;
            }
        } else {
            crate::objects::remove(s, there);
        }
        moved = 1;
        m = crate::objects::next_in_area(&mut s.objects);
    }
    moved
}

/// Original: 0x12e77c (objects: operate a switch: force fields 0xaf on its markers come and go)
pub async fn switch(ux: Ux<'_>, obj: Slot) {
    ux.print(STR_SWITCH, &[]).await;
    let moved = {
        let mut s = ux.s();
        let f = s.objects.tf(obj) >> 10;
        set_frame(&mut s, obj, f ^ 1);
        operate(&mut s, obj, 0xaf)
    };
    ux.print(if moved != 0 { STR_NOISE } else { STR_STRANGE }, &[]).await;
}

/// Original: 0x12e3f6 (objects: the drawbridge crank: find the bridge of the crank's quality)
/// [partial]
///
/// Ported: the search for a drawbridge piece (0x10d) of the crank's quality on the level ("No
/// correspondent drawbridge" when there is none). The raising / lowering 0x12de3a (with its
/// helpers 0x12dd1a, 0x12ddae, 0x12ddda) is a stub point.
pub async fn drawbridge(ux: Ux<'_>, obj: Slot) {
    let found = {
        let mut s = ux.s();
        let q = super::quality(&s, obj);
        let mut c = crate::objects::begin_area(&mut s, 0, 0, 0x3ff, 0x3ff);
        while c >= 0 && (s.objects.kind(c) != 0x10d || super::quality(&s, c) != q) {
            c = crate::objects::next_in_area(&mut s.objects);
        }
        c
    };
    if found == -1 {
        ux.print(STR_NO_BRIDGE, &[]).await;
        return;
    }
    // Contract: 0x101ff6 ×2, 0x102ef0 ×20, then 0x12de3a raises or lowers the bridge pieces and
    // one of five messages ("Strange, nothing happened." .. "Can't open the drawbridge.").
    // Fallback: the bridge stays as it is, no message.
    todo_stub!("0x12de3a raise or lower the drawbridge", ());
}

/// Original: 0x12f1e6 (objects: dig with a shovel or pick in a direction)
///
/// "Direction:"; a direction key names it, anything else "nowhere.". On the surface at
/// (0x2c3, 0x343) with no hole there a hole (0x134, quality 0x13) is dug ("You dig a hole.").
/// In a dungeon (levels 1-4) on a cave floor tile (0x6c..0x6f) 1 in 32 finds a fountain (0xea),
/// else 1 in 10 a gold nugget (0x59), else "Failed"; anywhere else "No effect".
pub async fn dig(ux: Ux<'_>) {
    ux.print(STR_DIRECTION, &[]).await;
    {
        let mut s = ux.s();
        s.ui.input.target_pending = 0;
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, ux.io, 0);
            s.ui.input.targeting = 1;
        }
    }
    if ux.key().await != 0x80 {
        ux.print(STR_NOWHERE, &[]).await;
        return;
    }
    let dir = ux.s().ui.input.direction;
    let name = defs::direction_name(ux.io, dir);
    ux.print(FMT_DIR, &[crate::ui::printf::Arg::Str(&name)]).await;
    let (dx, dy) = defs::direction(ux.io, dir);
    let (x, y, z, tile) = {
        let s = ux.s();
        let c = s.map.centre;
        let (x, y) = ((c.x as i16 + dx) as u16, (c.y as i16 + dy) as u16);
        let z = u16::from(c.z);
        let t = s.map.tile_at(usize::from(x), usize::from(y), usize::from(z));
        (x, y, z, t.unwrap_or(0))
    };
    let hole = z == 0 && x == 0x2c3 && y == 0x343 && {
        let mut s = ux.s();
        object_of_kind_at(&mut s, x, y, z, 0x134) == -1
    };
    if hole {
        {
            let mut s = ux.s();
            let h = crate::objects::create_on_map(&mut s.objects, 0x134, 0, 0, x, y, z);
            super::set_quality(&mut s, h, 0x13);
        }
        ux.print(STR_HOLE, &[]).await;
        return;
    }
    if !(1..=4).contains(&z) || !(0x6c..=0x6f).contains(&tile) {
        ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
        return;
    }
    let found = {
        let mut s = ux.s();
        if s.rng.range(0, 0x1f) == 0 {
            crate::objects::create_on_map(&mut s.objects, 0xea, 0, 0, x, y, z);
            Some(STR_FOUNTAIN)
        } else if s.rng.range(0, 9) == 0 {
            crate::objects::create_on_map(&mut s.objects, 0x59, 0, 1, x, y, z);
            Some(STR_NUGGET)
        } else {
            None
        }
    };
    match found {
        Some(msg) => ux.print(msg, &[]).await,
        None => ux.say(FMT_NL_LINE, MSG_FAILED).await,
    }
}
