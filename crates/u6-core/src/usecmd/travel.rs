//! Vessels, horses, the balloon, the orb of the moons, the telescope and the astrolabe:
//! 0x12f502 (with 0x12f3fa, and hunk 12's 0x11d0a4 / 0x11cd78), 0x12f6ee / 0x12f7ea /
//! 0x12f8fe, 0x12e924 / 0x12eb12, 0x12fb0a, 0x12fdf6, 0x12fca2.

use super::defs::{self, MSG_DONE, MSG_FAILED, MSG_OUT_OF_RANGE};
use super::dispatch::{FMT_BANG, FMT_LINE, FMT_NL_LINE};
use super::{Ux, aboard, dist, leader, member, seams, set_tf};
use crate::objects::{NONE, Slot, object_of_kind_at};
use crate::state::State;

const STR_NOT_SOLO: u32 = 0x0012_cd66;
const STR_ONLY_LAND: u32 = 0x0012_cd7a;
const STR_DEED: u32 = 0x0012_cd90;
const STR_NOT_BOARDABLE: u32 = 0x0012_cdac;
const STR_SHERRY: u32 = 0x0012_cdbe;
const STR_BEHLEM: u32 = 0x0012_cdee;
const STR_ON_HORSE: u32 = 0x0012_ce14;
const STR_REAR_HORSE: u32 = 0x0012_ce32;
const STR_NO_ROOM: u32 = 0x0012_cbd4;
const STR_NO_BAG: u32 = 0x0012_cb62;
const STR_NO_BASKET: u32 = 0x0012_cb80;
const STR_NO_CAULDRON: u32 = 0x0012_cb9c;
const STR_NO_ROPE: u32 = 0x0012_cbb2;
const STR_DONE_BANG: u32 = 0x0012_cbc4;
const STR_DONE_INFLATE: u32 = 0x0012_cbcc;
const STR_CANT_FIGURE: u32 = 0x0012_ce42;
const STR_WHERE: u32 = 0x0012_ce68;
const STR_RED_GATE: u32 = 0x0012_ce70;
const STR_DEGREES: u32 = 0x0012_ce8a;
const STR_LATITUDE: u32 = 0x0012_ceb4;
const STR_ZERO: u32 = 0x0012_cec2;
const STR_LONGITUDE: u32 = 0x0012_cec8;
const STR_DIRECTION: u32 = 0x0012_ced6;
const STR_NOWHERE: u32 = 0x0012_cd0c;
const FMT_DIR: u32 = 0x0012_cd16;
/// Original: A4-0x65ba / A4-0x65b2 (8 signed bytes each: dx / dy around a vessel)
const AROUND_DX: i32 = -0x65ba;
const AROUND_DY: i32 = -0x65b2;

fn z(s: &State) -> u16 {
    u16::from(s.map.centre.z)
}

/// Original: 0x12f3fa (objects: can the Avatar step off at (x, y)?)
///
/// For a ship (0x19c) the two tiles beside it (its frame direction ± 2); for any other vessel
/// the 8 tiles around, from the table at A4-0x65ba / -0x65b2; each through 0x11b1c0 for the
/// first party member.
pub fn can_land(s: &mut State, io: &crate::engine::Io, x: i16, y: i16, vessel: Slot) -> bool {
    let first = u16::from(member(s, 0));
    let around = |i: usize| {
        (
            i16::from(defs::a4_sbyte(io, AROUND_DX, i)),
            i16::from(defs::a4_sbyte(io, AROUND_DY, i)),
        )
    };
    let try_at = |s: &mut State, i: usize| {
        let (dx, dy) = around(i & 7);
        crate::actors::walk::can_move_to(
            s,
            first,
            x.wrapping_add(dx) as u16,
            y.wrapping_add(dy) as u16,
        )
    };
    if s.objects.kind(vessel) == 0x19c {
        let f = usize::from((s.objects.tf(vessel) >> 10) & 2);
        try_at(s, f + 2) || try_at(s, f + 6)
    } else {
        (0..8).any(|i| try_at(s, i))
    }
}

/// Original: 0x11d0a4 (actors: the party leaves its vessel)
///
/// The vessel becomes a map object again at the centre (0x10bcee: the actor's type + frame, its
/// hit points as quantity); unless `boarding`, a skiff or raft carried inside goes to the
/// centre too. The vessel actor turns into "nothing" (type 0x58, worktype 0) and moves to
/// level 14 (out of the world), member 0 leads, the party is placed at the centre (0x10921a
/// mode 2); a carried skiff is boarded at once (0x11cd78); the area song plays. Returns the new
/// vessel object.
pub fn leave_vessel(s: &mut State, boarding: bool) -> Slot {
    let v = member(s, usize::from(s.actors.party.size));
    let vi = usize::from(v);
    let c = s.map.centre;
    let tf = s.objects.type_frame[vi];
    let hp = u16::from(s.actors.attrs.hp[vi]);
    let obj =
        crate::objects::create_on_map(&mut s.objects, tf & 0x3ff, 1, hp, c.x, c.y, u16::from(c.z));
    let mut skiff = NONE;
    if !boarding {
        let mut k = crate::objects::first_child(&mut s.objects, Slot::from(v));
        while k >= 0 {
            if matches!(s.objects.kind(k), 0x19e | 0x19f) {
                crate::objects::move_to(s, k, c.x, c.y, u16::from(c.z));
                break;
            }
            k = crate::objects::next_child(&mut s.objects);
        }
        skiff = k;
    }
    if obj >= 0 {
        s.objects.obj_status[obj as usize] &= !0x20;
        s.objects.type_frame[obj as usize] = tf;
    }
    s.objects.type_frame[vi] = 0x58;
    s.actors.worktype[vi] = 0;
    crate::objects::move_to(s, Slot::from(v), c.x, c.y, 0xe);
    s.actors.party.leader_index = 0;
    crate::map::travel::place_party(s, c.x, c.y, u16::from(c.z), 2);
    if skiff >= 0 {
        board_vessel(s, skiff);
    }
    seams::area_song(s);
    obj
}

/// Original: 0x11cd78 (actors: the party boards vessel `obj`)
///
/// A ship's stern finds its bow (flags1 bits 4-7 give the direction). Combat ends (0x121d78);
/// a party already aboard leaves first, keeping a skiff or raft it had. The centre moves to
/// the vessel, the party leaves the region (0x108fba mode 2); actor 0 becomes the vessel
/// (type + frame, hit points from the quantity, status in party, worktype 2) and the object
/// goes; every member and the kept skiff go into its inventory (0x10cda8 with 0x10); the vessel
/// leads with the Avatar's name; draw lists, view flags and the area song.
pub fn board_vessel(s: &mut State, obj: Slot) {
    let mut v = obj;
    if s.objects.kind(v) == 0x19c {
        let d = match super::flags1_of(s, v) & 0xf0 {
            0x80 => 0,
            0x40 => 2,
            0x20 => 4,
            0x10 => 6,
            _ => -1,
        };
        if d >= 0 {
            let io_dir = crate::engine::with_current_io(|io| defs::direction(io, d));
            let (dx, dy) = io_dir.unwrap_or((0, 0));
            let (x, y) = (s.objects.x(v) as i16 + dx, s.objects.y(v) as i16 + dy);
            let zz = z(s);
            v = super::first_at(s, x as u16, y as u16, zz);
            while v >= 0 && s.objects.kind(v) != 0x19c {
                v = super::next_here(s);
            }
        }
    }
    if v < 0 {
        return;
    }
    seams::end_combat(s);
    let mut kept = NONE;
    if aboard(s) {
        kept = leave_vessel(s, true);
        if !matches!(s.objects.kind(kept), 0x19e | 0x19f) {
            kept = NONE;
        }
    }
    s.map.centre.x = s.objects.x(v);
    s.map.centre.y = s.objects.y(v);
    crate::map::travel::leave_region(s, 2);
    seams::reset_formation(s);
    let o = &mut s.objects;
    o.obj_status[0] &= !0x20;
    o.obj_status[0] &= !0x02;
    s.actors.movement_points[0] = s.actors.attrs.dexterity[0];
    let c = s.map.centre;
    crate::objects::move_to(s, 0, c.x, c.y, u16::from(c.z));
    s.objects.type_frame[0] = s.objects.tf(v);
    s.actors.attrs.hp[0] = super::quantity(s, v);
    let st = &mut s.actors.attrs.status[0];
    *st = (*st & 0x9f) | 0x40 | 0x80;
    s.actors.worktype[0] = 2;
    set_tf(s, v, 0x58);
    crate::objects::remove(s, v);
    for i in 0..usize::from(s.actors.party.size) {
        let m = Slot::from(member(s, i));
        crate::objects::move_into(s, m, 0, 0x10);
    }
    if kept >= 0 {
        crate::objects::move_into(s, kept, 0, 0x10);
    }
    let size = s.actors.party.size;
    s.actors.party.leader_index = size;
    if let Some(m) = s.actors.party.members.get_mut(usize::from(size)) {
        *m = 0;
    }
    let first = s.actors.party.names.first().cloned().unwrap_or_default();
    let i = usize::from(size);
    if s.actors.party.names.len() <= i {
        s.actors.party.names.resize(i + 1, String::new());
    }
    s.actors.party.names[i] = first;
    crate::objects::build_draw_lists(s);
    s.map.view_dirty = 1;
    seams::area_song(s);
}

/// Original: 0x12f502 (objects: board or leave a vessel: ship, skiff, raft, balloon)
///
/// Party mode only ("Not in solo mode!"). Used on the vessel the party is in: it leaves when the
/// Avatar can step off (0x12f3fa; a balloon deflates into its object 0x1a4), a ship beside
/// land with a skiff aboard puts the party into the skiff, else "Only next to land.". Another
/// vessel: a ship or skiff with a quality needs its deed (a member carrying 0x95 of that
/// quality, converse 0x11304a); next to the leader it is boarded (from a skiff onto a ship the
/// skiff comes along), else "Out of range".
pub async fn board(ux: Ux<'_>, obj: Slot) {
    let (v, vt, lead, lt, vx, vy) = {
        let mut s = ux.s();
        let v = seams::actor_of(&mut s, obj);
        let lead = leader(&s);
        (
            v,
            s.objects.kind(v),
            lead,
            s.objects.kind(Slot::from(lead)),
            s.objects.x(v) as i16,
            s.objects.y(v) as i16,
        )
    };
    if crate::ui::globals::solo_member(&ux.s().actors) != -1 {
        ux.print(STR_NOT_SOLO, &[]).await;
        return;
    }
    if aboard(&ux.s()) && Slot::from(lead) == v {
        let can = {
            let mut s = ux.s();
            let c = s.map.centre;
            can_land(&mut s, ux.io, c.x as i16, c.y as i16, v)
        };
        if can {
            let mut s = ux.s();
            let out = leave_vessel(&mut s, false);
            if vt == 0x1a7 {
                set_tf(&mut s, out, 0x1a4);
                seams::inventory_contents(&mut s);
            }
        } else {
            let skiff = {
                let mut s = ux.s();
                vt == 0x19c && crate::objects::find_child(&mut s.objects, v, 0x19e, 0) != -1
            };
            if skiff {
                leave_vessel(&mut ux.s(), true);
            } else {
                ux.print(STR_ONLY_LAND, &[]).await;
            }
        }
        ux.flush().await;
        return;
    }
    if matches!(vt, 0x19c | 0x19e) && super::quality(&ux.s(), v) != 0 {
        let q = super::quality(&ux.s(), v);
        if seams::member_with(&mut ux.s(), 0x95, i16::from(q)) == -1 {
            ux.print(STR_DEED, &[]).await;
            return;
        }
    }
    let far = {
        let s = ux.s();
        let c = s.map.centre;
        dist(c.x as i16, vx) > 1 || dist(c.y as i16, vy) > 1
    };
    if far {
        ux.say(FMT_BANG, MSG_OUT_OF_RANGE).await;
        return;
    }
    {
        let mut s = ux.s();
        if vt == 0x19c && lt == 0x19e {
            crate::objects::move_into(&mut s, Slot::from(lead), v, 0x10);
        }
        board_vessel(&mut s, v);
    }
    ux.flush().await;
}

/// Original: 0x12f6ee (objects: put `rider` on a horse at (x, y) facing `frame`)
///
/// The rider moves there and becomes the mounted horse (0x1af, `frame`), the view centres on
/// it; the rear half one tile behind is either the object already there re-typed (`here`) or
/// a new 0x1af with frame + 8. Tile cache and view are refreshed.
pub fn mount_horse(s: &mut State, rider: Slot, x: u16, y: u16, frame: u16, here: u16) {
    let zz = z(s);
    crate::objects::move_to(s, rider, x, y, zz);
    set_tf(s, rider, frame << 10 | 0x1af);
    s.map.centre.x = x;
    s.map.centre.y = y;
    let (dx, dy) = crate::engine::with_current_io(|io| defs::direction(io, (frame & 6) as i16))
        .unwrap_or((0, 0));
    let (bx, by) = ((x as i16 - dx) as u16, (y as i16 - dy) as u16);
    if here != 0 {
        let rear = super::first_at(s, bx, by, zz);
        let tf = s.objects.tf(rear);
        set_tf(s, rear, (tf & 0xfc00) | 0x1af);
    } else {
        crate::objects::create_on_map(&mut s.objects, 0x1af | (frame + 8) << 10, 0, 0, bx, by, zz);
    }
    crate::map::region::refresh_region(s);
    crate::engine::with_current_io(|io| crate::gfx::refresh_view(s, io));
}

/// Original: 0x12f7ea (objects: mount a horse 0x1ae)
///
/// Only a temporary actor (≥ 0xe0) is boardable; Sherry and Beh Lem refuse, a rider is
/// already on one. The horse turns into "nothing" and goes, the leader takes its place
/// mounted (0x12f6ee with the horse's frame, its rear re-typed).
pub async fn mount(ux: Ux<'_>, obj: Slot) {
    let (h, lead) = {
        let mut s = ux.s();
        (seams::actor_of(&mut s, obj), leader(&s))
    };
    let msg = if h < 0xe0 {
        Some(STR_NOT_BOARDABLE)
    } else if lead == 9 {
        Some(STR_SHERRY)
    } else if lead == 0xa4 {
        Some(STR_BEHLEM)
    } else if ux.s().objects.kind(Slot::from(lead)) == 0x1af {
        Some(STR_ON_HORSE)
    } else {
        None
    };
    if let Some(m) = msg {
        ux.print(m, &[]).await;
        return;
    }
    let mut s = ux.s();
    let (x, y, tf) = (s.objects.x(h), s.objects.y(h), s.objects.tf(h));
    set_tf(&mut s, h, (tf & 0xfc00).wrapping_add(0x58));
    crate::objects::remove(&mut s, h);
    mount_horse(&mut s, Slot::from(lead), x, y, tf >> 10, 1);
}

/// Original: 0x12f8fe (objects: dismount: the rider gets its own shape back, a horse appears
/// beside it, "No room!" when none fits)
pub fn dismount(s: &mut State, obj: Slot) {
    let r = super::seams::actor_of(s, obj);
    if r < 0 {
        return;
    }
    let ri = r as usize & 0xff;
    let (x, y, zz) = (s.objects.x(r), s.objects.y(r), z(s));
    let dir = (s.objects.tf(r) >> 10) & 6;
    let base = s.actors.attrs.base_type_frame[ri] & 0x3ff;
    let face = s.actors.attrs.movement_flags[ri] & 7;
    let frame = crate::objects::actor_frame(base, i16::from(face)) as u16;
    set_tf(s, r, frame << 10 | base);
    let (dx, dy) =
        crate::engine::with_current_io(|io| defs::direction(io, dir as i16)).unwrap_or((0, 0));
    let (bx, by) = ((x as i16 - dx) as u16, (y as i16 - dy) as u16);
    let mut rear = super::first_at(s, bx, by, zz);
    while rear >= 0 && s.objects.kind(rear) != 0x1af {
        rear = super::next_here(s);
    }
    if rear >= 0 {
        crate::objects::remove(s, rear);
    } else {
        queue(s, STR_REAR_HORSE);
    }
    let horse = crate::objects::spawn_monster(s, 0x1ae, 0, 0, zz);
    let mut placed = false;
    'out: for i in 0..3u16 {
        for j in 0..3u16 {
            if (i, j) == (1, 1) || horse < 0 {
                continue;
            }
            let (hx, hy) = ((x + i).wrapping_sub(1), (y + j).wrapping_sub(1));
            if crate::actors::walk::can_move_to(s, horse as u16, hx, hy) {
                crate::objects::move_to(s, horse, hx, hy, zz);
                placed = true;
                break 'out;
            }
        }
    }
    if !placed {
        queue(s, STR_NO_ROOM);
        crate::objects::remove(s, horse);
    }
    crate::engine::with_current_io(|io| crate::gfx::refresh_view(s, io));
}

fn queue(s: &mut State, fmt: u32) {
    let text = crate::engine::with_current_io(|io| crate::main_loop::game_string(io, fmt));
    crate::main_loop::queue_print(s, text.unwrap_or_default(), Vec::new());
}

/// Original: 0x12e924 (objects: assemble a balloon from its parts)
///
/// The party needs a mammoth silk bag (0x1a5), a balloon basket (0x1a6), a cauldron (0x93) and
/// a rope (0x11c), each missing one named; with all four they are taken (magic 0x116526 finds
/// each, 0x10c1d6 removes it) and a deflated balloon (0x1a4, takeable) lies at the leader's
/// feet, "Done!".
pub async fn balloon(ux: Ux<'_>) {
    ux.put(b'\n').await;
    let parts = [
        (0x1a5, STR_NO_BAG),
        (0x1a6, STR_NO_BASKET),
        (0x93, STR_NO_CAULDRON),
        (0x11c, STR_NO_ROPE),
    ];
    let mut owners = [NONE; 4];
    for (i, (kind, msg)) in parts.into_iter().enumerate() {
        owners[i] = seams::member_with(&mut ux.s(), kind, 0);
        if owners[i] == -1 {
            ux.print(msg, &[]).await;
        }
    }
    if owners.contains(&-1) {
        return;
    }
    {
        let mut s = ux.s();
        for (i, (kind, _)) in parts.into_iter().enumerate() {
            let o = seams::find_carried(&mut s, owners[i], kind);
            crate::objects::remove(&mut s, o);
        }
        let lead = Slot::from(leader(&s));
        let (x, y, zz) = (s.objects.x(lead), s.objects.y(lead), z(&s));
        let b = crate::objects::create_on_map(&mut s.objects, 0x1a4, 0, 0, 0, 0, zz);
        if b >= 0 {
            crate::objects::mark_takeable(&mut s.objects, b);
            s.objects.obj_status[b as usize] &= !0x20;
            crate::objects::move_to(&mut s, b, x, y, zz);
        }
    }
    ux.print(STR_DONE_BANG, &[]).await;
}

/// Original: 0x12eb12 (objects: inflate a balloon: 0x1a4 becomes the balloon 0x1a7 next to the
/// leader, "No room!" when none of the tiles around fits)
pub async fn inflate(ux: Ux<'_>, obj: Slot) {
    let done = {
        let mut s = ux.s();
        crate::objects::remove(&mut s, obj);
        let lead = Slot::from(leader(&s));
        let (x, y, zz) = (s.objects.x(lead), s.objects.y(lead), z(&s));
        let b = crate::objects::create_on_map(&mut s.objects, 0x1a7 | 3 << 10, 0, 0, x, y, zz);
        if b >= 0 {
            s.objects.obj_status[b as usize] &= !0x20;
        }
        let mut done = false;
        'out: for i in 0..3u16 {
            for j in 0..3u16 {
                if (i, j) == (0, 0) || b < 0 {
                    continue;
                }
                let (bx, by) = ((x + i).wrapping_sub(1), (y + j).wrapping_sub(1));
                if crate::actors::walk::can_move_to(&mut s, b as u16, bx, by) {
                    crate::objects::move_to(&mut s, b, bx, by, zz);
                    done = true;
                    break 'out;
                }
            }
        }
        done
    };
    ux.print(if done { STR_DONE_INFLATE } else { STR_NO_ROOM }, &[])
        .await;
    let mut s = ux.s();
    if s.ui.panel.mode == 0x92 {
        seams::inventory_contents(&mut s);
    }
}

/// Original: 0x12fb0a (objects: the orb of the moons: a red moongate where the player points)
///
/// Only the Avatar knows how (actor 5's talk flags bit 5, A4+0x3e11). "Where:" and a target
/// within 2 tiles of the centre: a red moongate (0x54) is made whose quality is the cell
/// (0 next to the centre on its row, else (dy + 2) · 5 + dx + 3); where the Avatar could stand
/// and no gate (0x12d / 0x12e marker) is, "a red moon gate appears." and it moves there with a
/// dissolve; else "Failed" and it goes.
pub async fn orb(ux: Ux<'_>) {
    let knows = ux
        .s()
        .actors
        .attrs
        .talk_flags
        .get(5)
        .is_some_and(|&f| f & 0x20 != 0);
    if !knows {
        ux.print(STR_CANT_FIGURE, &[]).await;
        return;
    }
    ux.print(STR_WHERE, &[]).await;
    {
        let mut s = ux.s();
        s.ui.input.target_pending = 1;
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, ux.io, 0);
            s.ui.input.targeting = 1;
            s.ui.input.range = 7;
        }
    }
    ux.key().await;
    let made = {
        let mut s = ux.s();
        let c = s.map.centre;
        let (cx, cy) = (c.x as i16, c.y as i16);
        let (tx, ty) = s.ui.input.target;
        if !(cx - 2..=cx + 2).contains(&tx) || !(cy - 2..=cy + 2).contains(&ty) {
            None
        } else {
            let cell = if ty == cy && (cx - 1..=cx + 1).contains(&tx) {
                0
            } else {
                (ty - cy + 2) * 5 + (tx - cx) + 3
            };
            let gate = crate::objects::create_on_map(
                &mut s.objects,
                0x54,
                0,
                (cell as u16) << 8 | 1,
                0,
                0,
                0,
            );
            let zz = z(&s);
            let ok = gate >= 0
                && crate::actors::walk::can_move_to(&mut s, gate as u16, tx as u16, ty as u16)
                && object_of_kind_at(&mut s, tx as u16, ty as u16, zz, 0x12d) == -1
                && object_of_kind_at(&mut s, tx as u16, ty as u16, zz, 0x12e) == -1;
            Some((gate, ok, tx, ty))
        }
    };
    match made {
        Some((gate, true, tx, ty)) => {
            ux.print(STR_RED_GATE, &[]).await;
            {
                let mut s = ux.s();
                let zz = z(&s);
                crate::objects::move_to(&mut s, gate, tx as u16, ty as u16, zz);
                s.gfx.dissolve = 1;
                crate::map::view::update_view(&mut s);
            }
            ux.refresh();
        }
        Some((gate, false, _, _)) => {
            ux.say(FMT_LINE, MSG_FAILED).await;
            crate::objects::remove(&mut ux.s(), gate);
        }
        None => ux.say(FMT_LINE, MSG_FAILED).await,
    }
}

/// Original: 0x12fdf6 (objects: look through a telescope)
///
/// The view centres on the telescope and redraws; "Direction:" and a direction key: with the
/// whole window revealed (A4-0x794c) the view slides that way up to 40 tiles, stopping on a
/// tile ≥ 0xfc, each step redrawn; then back on the party with a dissolve and "Done". Any other
/// key "nowhere.".
pub async fn telescope(ux: Ux<'_>, obj: Slot) {
    let (ox, oy) = {
        let mut s = ux.s();
        let c = s.map.centre;
        s.map.centre.x = s.objects.x(obj);
        s.map.centre.y = s.objects.y(obj);
        view_at_centre(&mut s, ux.io);
        (c.x, c.y)
    };
    ux.print(STR_DIRECTION, &[]).await;
    ux.s().ui.input.target_pending = 0;
    if ux.key().await != 0x80 {
        ux.print(STR_NOWHERE, &[]).await;
        let mut s = ux.s();
        s.map.centre.x = ox;
        s.map.centre.y = oy;
        return;
    }
    let dir = ux.s().ui.input.direction;
    let name = defs::direction_name(ux.io, dir);
    ux.print(FMT_DIR, &[crate::ui::printf::Arg::Str(&name)])
        .await;
    let (dx, dy) = defs::direction(ux.io, dir);
    {
        let mut s = ux.s();
        s.map.reveal_all = 1;
        s.gfx.dissolve = 1;
        view_at_centre(&mut s, ux.io);
        let (mut n, mut tile) = (0x28i16, 0u8);
        while n >= 0 && tile < 0xfc {
            s.map.centre.x = (s.map.centre.x as i16).wrapping_add(dx) as u16;
            s.map.centre.y = (s.map.centre.y as i16).wrapping_add(dy) as u16;
            view_at_centre(&mut s, ux.io);
            n -= 1;
            let c = s.map.centre;
            tile = s
                .map
                .tile_at(usize::from(c.x), usize::from(c.y), usize::from(c.z))
                .unwrap_or(0);
        }
        s.map.reveal_all = 0;
        s.map.centre.x = ox;
        s.map.centre.y = oy;
        crate::objects::build_draw_lists(&mut s);
        crate::map::region::refresh_region(&mut s);
        s.gfx.dissolve = 1;
        crate::gfx::refresh_view(&mut s, ux.io);
    }
    ux.say(FMT_NL_LINE, MSG_DONE).await;
}

/// 0x10e81e, 0x108f90, 0x102ef0 for the current centre.
fn view_at_centre(s: &mut State, io: &crate::engine::Io) {
    crate::objects::build_draw_lists(s);
    crate::map::region::refresh_region(s);
    crate::gfx::refresh_view(s, io);
}

/// Original: 0x12fca2 (objects: the astrolabe: gate travel to a typed latitude and longitude)
///
/// The leader's intelligence check (0x12c092) fails: 1..10 damage (combat 0x1231be, 0x12395a).
/// Else "Enter degrees followed by N, S, E or W", "At latitude=" and a line (0x107cba, parsed by
/// 0x128b98 mode 1) within −44..80, "longitude=" within −37..88: gate travel (magic 0x119afc)
/// with the sound off; out of range shows "0".
pub async fn sextant_travel(ux: Ux<'_>) {
    let (m, fails) = {
        let mut s = ux.s();
        let li = usize::from(s.actors.party.leader_index);
        let m = crate::objects::party_member(&s, li);
        let int = crate::actors::leaves::intelligence(&s, u16::from(m)) as i16;
        (m, super::hunk15::fumbles(&mut s, int))
    };
    if fails {
        {
            let mut s = ux.s();
            let d = s.rng.range(1, 10) as i16;
            seams::damage(&mut s, Slot::from(m), d);
        }
        ux.put(b'\n').await;
        seams::check_deaths(&mut ux.s());
        return;
    }
    {
        let mut s = ux.s();
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, ux.io, 0);
            s.ui.input.targeting = 0;
        }
    }
    ux.print(STR_DEGREES, &[]).await;
    ux.print(STR_LATITUDE, &[]).await;
    let line = seams::read_line(ux.state, ux.io, 4).await;
    let lat = crate::objects::parse_answer(&String::from_utf8_lossy(&line), 1);
    if !(-0x2c..0x51).contains(&lat) {
        ux.print(STR_ZERO, &[]).await;
        return;
    }
    ux.print(STR_LONGITUDE, &[]).await;
    let line = seams::read_line(ux.state, ux.io, 4).await;
    let lon = crate::objects::parse_answer(&String::from_utf8_lossy(&line), 2);
    if !(-0x25..0x59).contains(&lon) {
        ux.print(STR_ZERO, &[]).await;
        return;
    }
    ux.put(b'\n').await;
    let mut s = ux.s();
    let sound = s.main.sound;
    s.main.sound = 0;
    seams::gate_travel(&mut s);
    s.main.sound = sound;
}
