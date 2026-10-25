//! Levers, switches, the drawbridge, kegs, the cannon, shovels and moonstones (0x12d096..
//! 0x12e77c, 0x12f1e6).

use super::defs::{self, MSG_FAILED, MSG_NO_EFFECT};
use super::dispatch::FMT_NL_LINE;
use super::{Ux, leader, seams, set_frame};
use crate::engine::Io;
use crate::objects::{Slot, object_of_kind_at};
use crate::state::State;

const STR_BURIED: u32 = 0x0012_c91c;
const STR_CANT_BURY: u32 = 0x0012_c926;
const STR_FIRE: u32 = 0x0012_c9f2;
const STR_NOT_NOW: u32 = 0x0012_c9fa;
const STR_POWDER_LIT: u32 = 0x0012_ca04;
pub(super) const STR_NO_BRIDGE: u32 = 0x0012_ca2a;
/// 0x12e3f6's lines for the results 0..4 of 0x12de3a (the jump table at 0x12e4d6): nothing
/// moved, raised, lowered, something on it, something in the way.
pub(super) const STR_BRIDGE: [u32; 5] = [
    0x0012_ca48,
    0x0012_ca66,
    0x0012_ca7e,
    0x0012_ca96,
    0x0012_cab4,
];
const STR_LEVER: u32 = 0x0012_cad2;
const STR_STRANGE: u32 = 0x0012_cae6;
const STR_NOISE: u32 = 0x0012_cb02;
const STR_CANT_CLOSE: u32 = 0x0012_cb16;
const STR_SWITCH: u32 = 0x0012_cb34;
const STR_DIRECTION: u32 = 0x0012_cd00;
const STR_NOWHERE: u32 = 0x0012_cd0c;
const FMT_DIR: u32 = 0x0012_cd16; // format: the string, a full stop, a newline
const STR_HOLE: u32 = 0x0012_cd1c;
const STR_FOUNTAIN: u32 = 0x0012_cd2e;
const STR_NUGGET: u32 = 0x0012_cd4c;
/// Port-only (spec 08, slice K2; 2026-10-25): the strings these handlers pass to `printf` as its
/// format, for the locale layer's catalogue ([`super::PRINTED`]): the moonstone's two lines, the
/// cannon's, the powder keg's two, the drawbridge crank's (no bridge, and its five results), the
/// lever's and the switch's own lines and the lines of what they did (nothing moved, something
/// moved, an actor in the way), and the shovel's prompt, its no-direction word and its three
/// finds, each on a wrapping line of the message window; and
/// the shovel's direction format, of no word of its own, which the catalogue leaves out (the
/// direction's name goes through `%s`, a label read for display, [`crate::magic::LABELS`]).
pub(super) const PRINTED: &[u32] = &[
    STR_BURIED,
    STR_CANT_BURY,
    STR_FIRE,
    STR_NOT_NOW,
    STR_POWDER_LIT,
    STR_NO_BRIDGE,
    STR_BRIDGE[0],
    STR_BRIDGE[1],
    STR_BRIDGE[2],
    STR_BRIDGE[3],
    STR_BRIDGE[4],
    STR_LEVER,
    STR_STRANGE,
    STR_NOISE,
    STR_CANT_CLOSE,
    STR_SWITCH,
    STR_DIRECTION,
    STR_NOWHERE,
    FMT_DIR,
    STR_HOLE,
    STR_FOUNTAIN,
    STR_NUGGET,
];

/// Objlist block index of A4-0x7b5a (8 moonstone locations, 3 words each).
const MOONSTONES: usize = 0x2a;

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
/// On a grass or swamp tile (1..7, 0x10..0x6f) the line 0x12c91c (else 0x12c926): its location
/// goes into the moonstone table (A4-0x7b5a + 6 × frame: x, y, z), the stone onto the tile and
/// the leader on top of it.
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
/// The line 0x12c9f2; the aim is the first actor 1..5 tiles out (direction from the frame,
/// tables at A4-0x661c / -0x6618), else 6 tiles; the ball (tile 0x18f) flies there (combat
/// 0x1228ba, range 5, leaving where it stopped in A4+0x185e / +0x1860). When 0x1228ba gives 0
/// (the ball ended short of the aim) the explosion (0x1331b4, tile 0x189) is one tile further;
/// an object at the end that is not a counted kind takes 1..60 damage (0x1231be).
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
        let a = super::first_at(&mut s, x as u16, y as u16, z);
        if (0..0x100).contains(&a) {
            break;
        }
        n += 1;
    }
    let (tx, ty) = (cx.wrapping_add(dx * n), cy.wrapping_add(dy * n));
    let (reached, mut hx, mut hy) = seams::missile(&mut s, obj, 5, tx, ty, 0x18f);
    if reached == 0 {
        hx = hx.wrapping_add(dx);
        hy = hy.wrapping_add(dy);
        seams::blast(&mut s, 0x189, hx, hy);
    }
    let a = super::first_at(&mut s, hx as u16, hy as u16, z);
    if a >= 0 && crate::objects::stack_kind(&s.objects, s.objects.kind(a)) == 0 {
        let d = s.rng.range(1, 0x3c) as i16;
        seams::damage(&mut s, a, d);
    }
}

/// Original: 0x12d960 (objects: light a powder keg: it explodes when the keg timer runs out)
///
/// Only an unlit keg (frame 0) while no other burns (A4-0x7b2a = 0): frame 1, timer 3, the keg
/// in A4-0x7b28, the line 0x12ca04; a keg on the map redraws the view, a carried one the
/// inventory.
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

/// Original: 0x12dd6e (objects: the first actor at (x, y, z), 0 when none)
pub fn actor_at(s: &mut State, x: u16, y: u16, z: u16) -> Slot {
    let mut c = super::first_at(s, x, y, z);
    while c > 0xff {
        c = super::next_here(s);
    }
    if c == -1 { 0 } else { c }
}

/// Original: 0x12e538 (objects: switch a lever: open or close the portcullises it works)
///
/// The lever's frame bit 0 flips. With a quality q, every portcullis marker (0x12d of quality
/// q) on the level opens (its portcullis 0x136 removed) or closes (one created, frame 1, or 3
/// when the marker's frame has bit 1) unless an actor stands there (the line 0x12cb16 ends the
/// walk). The line 0x12cb02 when something moved, else 0x12cae6.
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
                if (s.objects.tf(m) >> 10) & 2 != 0 {
                    3
                } else {
                    1
                }
            } else {
                0
            };
            let new =
                crate::objects::create_on_map(&mut s.objects, gate | frame << 10, 0, 0, x, y, z);
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
    ux.print(if moved != 0 { STR_NOISE } else { STR_STRANGE }, &[])
        .await;
}

/// The drawbridge piece type.
const BRIDGE: u16 = 0x10d;
/// The tiles 0x12e3f6 animates while the crank turns (0x101ff6 on each, in this order).
const CRANK_TILES: [u16; 2] = [0x3f1, 0x3fc];
/// Port: the most rows [`bridge`] walks down looking for the end of a bridge it lowers.
const BRIDGE_ROWS: u16 = 0x400;

/// Original: 0x12e3f6 (objects: the drawbridge crank: raise or lower the bridge of its quality)
///
/// The first drawbridge piece (0x10d) of the crank's quality on the level is the bridge's (none:
/// the line 0x12ca2a). The tiles 0x3f1 and 0x3fc animate (0x101ff6: backward when that piece
/// has frame 3, else forward) for 20 view refreshes (0x102ef0); then 0x12de3a moves the bridge,
/// its result picks the line ([`STR_BRIDGE`]) and the animation stops (0x101ff6 with 0).
///
/// Port: each refresh waits its time on the engine clock before it is drawn, as X-ray's do
/// ([`crate::magic::xray`]); without a wait the port would publish none of them.
pub async fn drawbridge(ux: Ux<'_>, obj: Slot) {
    let found = {
        let mut s = ux.s();
        let q = super::quality(&s, obj);
        let mut c = crate::objects::begin_area(&mut s, 0, 0, 0x3ff, 0x3ff);
        while c >= 0 && (s.objects.kind(c) != BRIDGE || super::quality(&s, c) != q) {
            c = crate::objects::next_in_area(&mut s.objects);
        }
        c
    };
    if found == -1 {
        ux.print(STR_NO_BRIDGE, &[]).await;
        return;
    }
    {
        let mut s = ux.s();
        let way = if s.objects.tf(found) >> 10 == 3 { 2 } else { 1 };
        for tile in CRANK_TILES {
            crate::main_loop::set_tile_animation(&mut s, tile, way);
        }
    }
    for _ in 0..20 {
        crate::combat::present::pace(ux.state, ux.io, crate::combat::present::REFRESH_MS).await;
        crate::gfx::refresh_view(&mut ux.s(), ux.io);
    }
    let result = bridge(&mut ux.s(), ux.io, found);
    ux.print(STR_BRIDGE[usize::from(result)], &[]).await;
    let mut s = ux.s();
    for tile in CRANK_TILES {
        crate::main_loop::set_tile_animation(&mut s, tile, 0);
    }
}

/// Original: 0x12de3a (objects: raise or lower the drawbridge whose first piece is `piece`; 0..4)
///
/// `piece`, at (x0, y0), carries the bridge's quality. Frame 3 (lowered): the rows from y0
/// down that have a piece at x0, each walked east while there is one; anything else on one
/// (0x12dd1a) gives 3. Else a frame 6 piece of that quality at (x0 − 1, y0 − 1), the first
/// row's pieces become frame 7 one row up, the last of them frame 8, the other rows go: 1.
/// Frame 6 (raised): n = 1 + the pieces of its row from x0. From (x0 + 1, y0 + 1) down to the
/// row whose map tile at x0 + 1 is an end (0x12ddae), anything on the first n − 4 tiles of a
/// row gives 4. Else the row goes; every row above the end row gets frame 3 (of the quality),
/// n − 4 × frame 4 and frame 5 from x0 + 1, the end row frame 0, n − 4 × frame 1 and frame 2,
/// each with what lies on its tile moved there again (0x10c9d2, ahead of the piece): 2. Both
/// end with the view flags (0x109e74). Any other frame: 0.
///
/// Port: down a column with no end tile the original walks on for ever (the rows wrap round
/// the world); the port stops after [`BRIDGE_ROWS`] rows and leaves the bridge as it is (0).
/// And the original goes on when the object pool cannot make a piece (0x10bc4c: -1), leaving
/// the bridge part built or with no piece of its quality for the crank to find; before it
/// changes anything the port counts the free slots the pieces need, in the order the original
/// makes and removes them, and when they fall short leaves the bridge as it is (0).
fn bridge(s: &mut State, io: &Io, piece: Slot) -> u8 {
    let q = super::quality(s, piece);
    let (x0, y0) = (s.objects.x(piece), s.objects.y(piece));
    match s.objects.tf(piece) >> 10 {
        3 => raise(s, piece, q, x0, y0),
        6 => lower(s, io, piece, q, x0, y0),
        _ => 0,
    }
}

/// The piece at (x, y) on the view's level (0x12dcbe of 0x10d), -1.
fn piece_at(s: &mut State, x: u16, y: u16) -> Slot {
    let z = z(s);
    object_of_kind_at(s, x, y, z, BRIDGE)
}

/// Frame 3 of 0x12de3a: the lowered bridge goes up into one row.
fn raise(s: &mut State, piece: Slot, q: u8, x0: u16, y0: u16) -> u8 {
    let z = z(s);
    let (mut c, mut x, mut y) = (piece, x0, y0);
    while c >= 0 {
        while c >= 0 {
            if on_bridge(s, x, y, z) != 0 {
                return 3;
            }
            x = x.wrapping_add(1);
            c = piece_at(s, x, y);
        }
        (x, y) = (x0, y.wrapping_add(1));
        c = piece_at(s, x, y);
    }
    // Port: the frame 6 piece is made before any piece goes; every piece after it right
    // after one went (a frame 7 after its row's piece, the frame 8 after the last frame 7).
    if s.objects.free.1 < 1 {
        return 0;
    }
    let first = place(s, x0.wrapping_sub(1), y0.wrapping_sub(1), 6);
    super::set_quality(s, first, q);
    let (mut c, mut x, mut last) = (piece, x0, crate::objects::NONE);
    while c >= 0 {
        crate::objects::remove(s, c);
        last = place(s, x, y0.wrapping_sub(1), 7);
        x = x.wrapping_add(1);
        c = piece_at(s, x, y0);
    }
    crate::objects::remove(s, last);
    place(s, x.wrapping_sub(1), y0.wrapping_sub(1), 8);
    let (mut x, mut y) = (x0, y0.wrapping_add(1));
    let mut c = piece_at(s, x, y);
    while c >= 0 {
        while c >= 0 {
            crate::objects::remove(s, c);
            x = x.wrapping_add(1);
            c = piece_at(s, x, y);
        }
        (x, y) = (x0, y.wrapping_add(1));
        c = piece_at(s, x, y);
    }
    crate::map::view::update_view(s);
    1
}

/// Frame 6 of 0x12de3a: the raised bridge comes down to its end row.
fn lower(s: &mut State, io: &Io, piece: Slot, q: u8, x0: u16, y0: u16) -> u8 {
    let z = z(s);
    let (mut n, mut c, mut x) = (1i16, piece, x0);
    while c >= 0 {
        n += 1;
        x = x.wrapping_add(1);
        c = piece_at(s, x, y0);
    }
    let x1 = x0.wrapping_add(1);
    let tile = |s: &State, y: u16| {
        let t = s
            .map
            .tile_at(usize::from(x1), usize::from(y), usize::from(z));
        u16::from(t.unwrap_or(0))
    };
    let (mut y, mut rows) = (y0.wrapping_add(1), 0);
    while !bridge_end(io, tile(s, y)) {
        rows += 1;
        if rows > BRIDGE_ROWS {
            return 0;
        }
        let mut x = x1;
        for _ in 0..n - 4 {
            if on_bridge(s, x, y, z) != 0 {
                return 4;
            }
            x = x.wrapping_add(1);
        }
        y = y.wrapping_add(1);
    }
    // Port: the raised row's n − 1 pieces go first, then each of the rows above the end row
    // and the end row gets 2 + (n − 4 when positive) pieces.
    let made = (i32::from(rows) + 1) * (2 + i32::from(n - 4).max(0));
    if i32::from(s.objects.free.1) + i32::from(n - 1) < made {
        return 0;
    }
    let (mut c, mut x) = (piece, x0);
    while c >= 0 {
        crate::objects::remove(s, c);
        x = x.wrapping_add(1);
        c = piece_at(s, x, y0);
    }
    let mut y = y0.wrapping_add(1);
    while !bridge_end(io, tile(s, y)) {
        let first = place(s, x1, y, 3);
        super::set_quality(s, first, q);
        let mut x = x1.wrapping_add(1);
        for _ in 0..n - 4 {
            place(s, x, y, 4);
            x = x.wrapping_add(1);
        }
        place(s, x, y, 5);
        y = y.wrapping_add(1);
    }
    let mut x = x1;
    for frame in std::iter::once(0)
        .chain(std::iter::repeat_n(1, usize::try_from(n - 4).unwrap_or(0)))
        .chain(std::iter::once(2))
    {
        place(s, x, y, frame);
        let o = on_bridge(s, x, y, z);
        if o != 0 {
            let (ox, oy) = (s.objects.x(o), s.objects.y(o));
            crate::objects::move_to(s, o, ox, oy, z);
        }
        x = x.wrapping_add(1);
    }
    crate::map::view::update_view(s);
    2
}

/// Original: 0x12dd1a (objects: the first object at (x, y, z) that is not a drawbridge piece, 0
/// when none)
fn on_bridge(s: &mut State, x: u16, y: u16, z: u16) -> Slot {
    let mut c = super::first_at(s, x, y, z);
    while c >= 0 && s.objects.kind(c) == BRIDGE {
        c = super::next_here(s);
    }
    if c == -1 { 0 } else { c }
}

/// Original: 0x12ddae (objects: `tile` is one of the 5 bytes at A4-0x6614, the map tiles a
/// lowered drawbridge ends on)
fn bridge_end(io: &Io, tile: u16) -> bool {
    let (off, n) = defs::BRIDGE_ENDS;
    (0..n).any(|i| u16::from(defs::a4_byte(io, off, i)) == tile)
}

/// Original: 0x12ddda (objects: a new drawbridge piece of `frame` at (x, y) on the view's level)
///
/// The type is 0x10d at every call. Made from a record (0x134e1c packs the place, 0x10bc4c
/// makes the object: status, quantity and quality 0), then not temporary (status bit 5
/// cleared). Returns the slot, -1 when the pool is full.
fn place(s: &mut State, x: u16, y: u16, frame: u16) -> Slot {
    let [b0, b1, b2] = crate::objects::pack_xyz(x, y, z(s)).to_packed();
    let [hi, lo] = (BRIDGE | frame << 10).to_be_bytes();
    let p = crate::objects::create_from_record(s, [0, b0, b1, b2, hi, lo, 0, 0]);
    if let Some(st) = usize::try_from(p)
        .ok()
        .and_then(|i| s.objects.obj_status.get_mut(i))
    {
        *st &= !0x20;
    }
    p
}

/// Original: 0x12f1e6 (objects: dig with a shovel or pick in a direction)
///
/// The direction prompt (0x12cd00); a direction key names it, anything else the line 0x12cd0c.
/// On the surface at (0x2c3, 0x343) with no hole there a hole (0x134, quality 0x13) is dug (the
/// line 0x12cd1c). In a dungeon (levels 1-4) on a cave floor tile (0x6c..0x6f) 1 in 32 finds a
/// fountain (0xea), else 1 in 10 a gold nugget (0x59), else the failed message; anywhere else
/// the no-effect message.
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
    // Port (spec 08, slice K2; 2026-10-24): read for display, so the locale pack's label of it
    // when it has one ([`crate::magic::LABELS`], [`crate::locale::shown`]).
    let name = crate::locale::shown(&ux.s(), defs::direction_name(ux.io, dir));
    ux.print(FMT_DIR, &[crate::ui::printf::Arg::Str(&name)])
        .await;
    let (dx, dy) = defs::direction(ux.io, dir);
    let (x, y, z, tile) = {
        let s = ux.s();
        let c = s.map.centre;
        let (x, y) = ((c.x as i16 + dx) as u16, (c.y as i16 + dy) as u16);
        let z = u16::from(c.z);
        let t = s
            .map
            .tile_at(usize::from(x), usize::from(y), usize::from(z));
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
