//! Calls from the actor code into functions other modules own and have not ported yet.
//!
//! Each seam is the narrowest signature the actor code needs, with a `todo_stub!` body and an
//! explicit fallback (common rules; spec 03 §3). The dispatcher replaces the bodies with calls
//! into the owning module at merge; the signatures are the contract. Proposed owner in brackets.
//!
//! Object identities: slots `0..OBJECT_SLOTS` are the object tables. Until objects/ ports the
//! region loader (0x10e1ce) the slots past the actors are empty, so the fallbacks also report the
//! on-map records of the loaded `savegame/objblk??` files, numbered from [`BLOCK_IDS`] upward
//! (block order of `Objects::blocks`, then record order). Such ids are never below 0x100, so
//! the actor code treats them as non-actor objects, as the original does with slot ids.

use u6_formats::objblk::{ObjBlockId, ObjLocation};
use u6_formats::position::Position;

use std::cell::RefCell;

use crate::engine::{Io, Key};
use crate::objects::{ACTOR_SLOTS, OBJECT_SLOTS, ObjectId};
use crate::state::State;
use crate::todo_stub;

/// First id of an objblk record in the fallbacks (one past the object tables).
pub const BLOCK_IDS: u16 = OBJECT_SLOTS as u16;

/// One object found at a location: its id and the part of a multi-tile object that covers the
/// location (the original leaves it in A4-0x78dc; 0 = the object's own tile).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: ObjectId,
    pub part: u16,
}

fn block_record(s: &State, id: ObjectId) -> Option<(usize, usize)> {
    let mut n = usize::from(id.0.checked_sub(BLOCK_IDS)?);
    for (b, (_, block)) in s.objects.blocks.iter().enumerate() {
        if n < block.objects.len() {
            return Some((b, n));
        }
        n -= block.objects.len();
    }
    None
}

/// Type + frame word of an object (0x137988; objblk records in the fallback id range).
pub fn obj_word(s: &State, id: ObjectId) -> u16 {
    if id.index() < OBJECT_SLOTS {
        return s.objects.type_frame[id.index()];
    }
    block_record(s, id).map_or(0, |(b, i)| {
        let r = &s.objects.blocks[b].1.objects[i];
        (r.obj_n & 0x3ff) | u16::from(r.frame & 0x3f) << 10
    })
}

/// Writes the type + frame word of an object.
pub fn set_obj_word(s: &mut State, id: ObjectId, word: u16) {
    if id.index() < OBJECT_SLOTS {
        s.objects.type_frame[id.index()] = word;
    } else if let Some((b, i)) = block_record(s, id) {
        let r = &mut s.objects.blocks[b].1.objects[i];
        r.obj_n = word & 0x3ff;
        r.frame = (word >> 10) as u8;
    }
}

/// Location of an object (0x135588; objblk records in the fallback id range).
pub fn obj_pos(s: &State, id: ObjectId) -> Position {
    if id.index() < OBJECT_SLOTS {
        return s.objects.location[id.index()];
    }
    block_record(s, id).map_or(Position::default(), |(b, i)| {
        s.objects.blocks[b].1.objects[i].pos
    })
}

/// Objects on the map in the rectangle, in fallback order: actor slots, other slots, then the
/// objblk records of the blocks covering it. Dungeon coordinates are masked to 8 bits.
fn scan(s: &State, x0: u16, y0: u16, x1: u16, y1: u16, z: u16) -> Vec<ObjectId> {
    let m = if z == 0 { 0x3ff } else { 0xff };
    let inside = |p: Position| {
        u16::from(p.z) == z
            && (x0 & m..=x1 & m).contains(&(p.x & m))
            && (y0 & m..=y1 & m).contains(&(p.y & m))
    };
    let mut out = Vec::new();
    for a in 0..OBJECT_SLOTS {
        let on_map = a >= ACTOR_SLOTS || s.objects.actor_flags[a] & 0x18 == 0;
        if s.objects.type_frame[a] != 0 && on_map && inside(s.objects.location[a]) {
            out.push(ObjectId(a as u16));
        }
    }
    let covers = |id: ObjBlockId| match id {
        ObjBlockId::Surface { sx, sy } if z == 0 => {
            let (bx, by) = (u16::from(sx) * 128, u16::from(sy) * 128);
            bx <= x1 && x0 < bx + 128 && by <= y1 && y0 < by + 128
        }
        ObjBlockId::Dungeon { z: dz } => u16::from(dz) == z,
        _ => false,
    };
    let mut base = BLOCK_IDS;
    for (id, block) in &s.objects.blocks {
        if covers(*id) {
            for (i, r) in block.objects.iter().enumerate() {
                if r.location() == ObjLocation::Map && inside(r.pos) {
                    out.push(ObjectId(base.saturating_add(i as u16)));
                }
            }
        }
        base = base.saturating_add(block.objects.len() as u16);
    }
    out
}

/// [objects] 0x10ad1a + 0x10aaca: every object at (x, y, z), with the covering part.
///
/// Contract: the original is a stateful cursor (begin, then next until negative) and also
/// yields the parts of multi-tile objects whose other tiles cover the location. Fallback: the
/// [`scan`] of that one tile, part 0 (multi-tile coverage missing).
pub fn objects_at(s: &State, x: u16, y: u16, z: u16) -> Vec<Found> {
    let ids = scan(s, x, y, x, y, z);
    let found = ids.into_iter().map(|id| Found { id, part: 0 }).collect();
    todo_stub!(
        "0x10ad1a objects at a location (fallback: table scan)",
        found
    )
}

/// [objects] 0x10b05a + 0x10af74: every object in the window rectangle on the current level.
/// Fallback: the [`scan`] of the rectangle.
pub fn objects_in_rect(s: &State, x0: u16, y0: u16, x1: u16, y1: u16, z: u16) -> Vec<ObjectId> {
    let ids = scan(s, x0, y0, x1, y1, z);
    todo_stub!(
        "0x10b05a objects in a rectangle (fallback: table scan)",
        ids
    )
}

/// [objects] 0x10c9d2: moves an object or actor to (x, y, z), keeping the region lists;
/// negative = failed (the player step then prints "Object mismatch"). Fallback: writes the
/// location only (an actor slot or a fallback objblk record), returns 0.
pub fn move_object(s: &mut State, id: ObjectId, x: u16, y: u16, z: u16) -> i16 {
    let to = Position {
        x: x & 0x3ff,
        y: y & 0x3ff,
        z: (z & 0xf) as u8,
    };
    if id.index() < OBJECT_SLOTS {
        s.objects.location[id.index()] = to;
    } else if let Some((b, i)) = block_record(s, id) {
        s.objects.blocks[b].1.objects[i].pos = to;
    }
    todo_stub!("0x10c9d2 move an object (fallback: location only)", 0)
}

/// [objects] 0x131196: index of a creature type in the A4-0x8000 list, -1 when not listed.
/// Fallback: the same search on the list extracted from `game` ([`super::GameTables`]).
pub fn creature_index(s: &State, obj_type: u16) -> i16 {
    let i = s.actors.game.creature_index(obj_type);
    todo_stub!("0x131196 creature index (fallback: extracted list)", i)
}

/// [objects] 0x10ae52 + 0x10aed6: objects inside or carried by `holder`. Fallback: none (the
/// slot tables hold no inventories before 0x10e1ce), so carried-item effects do not run.
pub fn contents(_s: &State, _holder: ObjectId) -> Vec<ObjectId> {
    todo_stub!(
        "0x10ae52 contents of an object (fallback: none)",
        Vec::new()
    )
}

/// [objects] 0x10c1d6: removes an object from the world. Fallback: nothing removed.
pub fn remove_object(_s: &mut State, _id: ObjectId) {
    todo_stub!("0x10c1d6 remove an object (fallback: kept)", ())
}

/// [map] 0x10996c: map tile at (x, y, z) (window cache, else the chunk). Fallback: the tile
/// from the whole map, 0 off the map or with no map loaded (`WorldMap::chunk_at` indexes an
/// empty grid without a check, so the fallback does not call it then).
pub fn tile_at(s: &State, x: u16, y: u16, z: u16) -> u8 {
    let loaded = !s.map.world.surface.is_empty();
    let t = loaded
        .then(|| {
            s.map
                .tile_at(usize::from(x), usize::from(y), usize::from(z))
        })
        .flatten()
        .unwrap_or(0);
    todo_stub!("0x10996c tile at (fallback: whole map)", t)
}

/// [map] origin and coordinate mask of the 40 × 40 map-window tile cache (A4-0x2ef8, A4-0x2ef6,
/// A4-0x796c; set when 0x108f90 rebuilds it). Fallback: the window the turn code itself uses,
/// ((leader x − 16) & 0x3f8, (leader y − 16) & 0x3f8), mask 0x3ff on the surface, 0xff below.
pub fn window_origin(s: &State) -> (u16, u16, u16) {
    let c = s.map.centre;
    let o = (
        c.x.wrapping_sub(16) & 0x3f8,
        c.y.wrapping_sub(16) & 0x3f8,
        if c.z == 0 { 0x3ff } else { 0xff },
    );
    todo_stub!("0x108f90 window-cache origin (fallback: turn window)", o)
}

/// [map] byte (col, row) of the map-window tile cache A4+0x4b0a (filled by 0x108d10).
/// Fallback: the map tile under that cell of [`window_origin`].
pub fn window_tile(s: &State, col: u16, row: u16) -> u8 {
    let (x0, y0, m) = window_origin(s);
    let z = u16::from(s.map.centre.z);
    tile_at(s, x0.wrapping_add(col) & m, y0.wrapping_add(row) & m, z)
}

/// [map] 0x109e74: recomputes the view flags (blocking / light / visible). Fallback: none.
pub fn refresh_view_flags(_s: &mut State) {
    todo_stub!("0x109e74 view-window flags (fallback: not recomputed)", ())
}

/// [map] 0x108f90: rebuilds the map-window cache around the leader. Fallback: none (the
/// fallbacks above read the map directly).
pub fn rebuild_window(_s: &mut State) {
    todo_stub!("0x108f90 rebuild the map-window cache (fallback: none)", ())
}

/// A `printf` argument the actor code passes to 0x108658. With format offset 0 the first
/// argument is the format itself (the original passes a name or a PC-relative string as the
/// format).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// A party member's name (A4+0x470c + 14 × index).
    PartyName(u8),
    Word(i16),
    /// The display name of an object (0x10eef8).
    Name(ObjectId),
    /// A string in the data hunk at A4 + offset.
    Text(i32),
    /// A string at an absolute image address (PC-relative strings in code hunks).
    Image(u32),
}

/// [ui] 0x108658: formats the data-hunk string at A4 + `text` with `args` into the message
/// scroll (strings are game data and stay in `game`). Fallback: nothing printed.
pub fn print(_s: &mut State, _text: i32, _args: &[Arg]) {
    todo_stub!(
        "0x108658 print to the message scroll (fallback: not shown)",
        ()
    )
}

/// [ui] 0x108658 with an object's display name (0x10eef8) as the format. Fallback: nothing
/// printed.
pub fn print_name(_s: &mut State, _id: ObjectId) {
    todo_stub!(
        "0x10eef8 display name of an object (fallback: not shown)",
        ()
    )
}

/// [main] 0x102f2a: after an actor's turn, redraws the view when 0x6a8e asks for it.
/// Fallback: clears the request (A4-0x6a8e) as the original does after the redraw.
pub fn after_turn_refresh(s: &mut State) {
    s.map.view_dirty = 0;
    todo_stub!(
        "0x102f2a refresh after an actor turn (fallback: no redraw)",
        ()
    )
}

/// [main] 0x103fd4: makes party member `member` the leader (view centre, window rebuild).
/// Fallback: the leader index and the view centre only.
pub fn switch_leader(s: &mut State, member: u8) {
    if s.actors.party.leader_index != member {
        s.actors.party.leader_index = member;
        if let Some(pos) = s.leader_position() {
            s.map.centre = pos;
        }
        s.gfx.window_shown = 1;
        s.actors.work.prompt = 1;
    }
    todo_stub!(
        "0x103fd4 switch the leader (fallback: index and centre)",
        ()
    )
}

/// [main] 0x1032b6: per-actor status tick the clock runs once per minute (invisibility near
/// light sources, then 0x102f3c for party members). Fallback: none.
pub fn actor_status_tick(_s: &mut State, _actor: u16) {
    todo_stub!("0x1032b6 per-actor status tick (fallback: none)", ())
}

/// [main] 0x10355c: moongate objects appear / vanish with the moons. Fallback: none.
pub fn moongates(_s: &mut State) {
    todo_stub!("0x10355c moongates follow the moons (fallback: none)", ())
}

/// [main] 0x103632: hourly wind change (draws the RNG). Fallback: no change, no draw.
pub fn wind_change(_s: &mut State, _calm: u16) {
    todo_stub!(
        "0x103632 hourly wind change (fallback: unchanged, no RNG draw)",
        ()
    )
}

/// [gfx] 0x152cd2: sets the animated sky tile for the hour. Fallback: none.
pub fn sky_tile(_s: &mut State, _tile: u16, _frame: u16) {
    todo_stub!("0x152cd2 sky tile for the hour (fallback: none)", ())
}

/// [ui] 0x133b28: redraws the sun/moon strip. Fallback: none.
pub fn draw_sky_strip(_s: &mut State) {
    todo_stub!("0x133b28 sun and moon strip (fallback: none)", ())
}

/// [ui] 0x107dd8 / 0x107e00: saves / positions / restores the status-line text cursor around
/// the date line. Fallback: none.
pub fn status_cursor(_s: &mut State) {
    todo_stub!("0x107dd8 status-line cursor (fallback: none)", ())
}

/// [gfx] 0x102c90: presents the map window. Fallback: none.
pub fn present_map(_s: &mut State) {
    todo_stub!("0x102c90 present the map window (fallback: none)", ())
}

/// [main] 0x101ff6: sets the animation flag of the animdata slot animating `tile` (1 = on).
pub fn set_tile_animation(s: &mut State, tile: u16, on: bool) {
    crate::main_loop::set_tile_animation(s, tile, u8::from(on));
}

/// [objects] 0x10a5b0: the other half of a two-tile creature, -1 if none. Fallback: none (the
/// half stays where it was; no two-tile creature is in the start area).
pub fn other_half(_s: &State, _actor: u16) -> i16 {
    todo_stub!(
        "0x10a5b0 other half of a two-tile creature (fallback: none)",
        -1
    )
}

/// [objects] quality byte of an object (low byte of the word at A4+0x250c + 2 × object: torch
/// burn time). Fallback: 0 (no object slots are filled before 0x10e1ce).
pub fn obj_quality(_s: &State, _id: ObjectId) -> u8 {
    todo_stub!("0x15dbfc object quality table A4+0x250c (fallback: 0)", 0)
}

/// [objects] writes the quality byte of an object. Fallback: nothing written.
pub fn set_obj_quality(_s: &mut State, _id: ObjectId, _q: u8) {
    todo_stub!(
        "0x15dbfc object quality table A4+0x250c write (fallback: none)",
        ()
    )
}

/// [combat] 0x121b08: Chebyshev distance from `actor` to (x, y) (max of the axis distances;
/// combat stub point that only calls the runtime `abs` 0x101ea8). Fallback: the same distance.
pub fn distance(s: &State, actor: u16, x: u16, y: u16) -> i16 {
    let p = s.objects.location[usize::from(actor)];
    let dx = (p.x as i16).wrapping_sub(x as i16).wrapping_abs();
    let dy = (p.y as i16).wrapping_sub(y as i16).wrapping_abs();
    todo_stub!(
        "0x121b08 distance to a tile (fallback: max axis distance)",
        dx.max(dy)
    )
}

/// [ui] 0x10790a: waits for a command key and returns its game code (letters upper case).
/// Fallback: the next queued key, letters as their upper-case ASCII code, Enter 0x0d, Escape
/// 0x1b, space 0x20, arrows the direction code 0x80 (panel and mouse handling missing).
pub async fn read_key(io: &Io) -> u16 {
    let k = io.next_key().await;
    let code = match k {
        Key::Char(c) if c.is_ascii() => u16::from(c.to_ascii_uppercase() as u8),
        Key::Char(_) => 0,
        Key::Enter => 0x0d,
        Key::Escape => 0x1b,
        Key::Space => 0x20,
        Key::Backspace => 0x08,
        Key::Up | Key::Down | Key::Left | Key::Right => 0x80,
    };
    todo_stub!(
        "0x10790a read a command key (fallback: plain key map)",
        code
    )
}

/// [converse] 0x1127c4 (Talk: `speaker` = party member, `npc` = who talks; `announce` prints
/// the NPC's name first). Fallback: no conversation (the turn continues).
pub async fn talk(_state: &RefCell<State>, _io: &Io, _speaker: u16, _npc: u16, _announce: bool) {
    todo_stub!("0x1127c4 talk to an actor (fallback: no conversation)", ())
}

/// [ui] 0x133b0e: full redraw of the view (map, objects, flags). Fallback: none.
pub fn full_redraw(_s: &mut State) {
    todo_stub!("0x133b0e full redraw (fallback: none)", ())
}

/// [main] 0x102ef0: post-step refresh (rebuild the view and present). Fallback: none.
pub fn refresh_view(_s: &mut State) {
    todo_stub!("0x102ef0 post-step refresh (fallback: none)", ())
}

/// [sound] 0x110912: picks and starts the song for the current area. Fallback: silence.
pub fn area_music(_s: &mut State) {
    todo_stub!("0x110912 area music (fallback: silence)", ())
}

/// [map] 0x1092fc: moves the party to (x, y, z) (jail). Fallback: the leader and the view
/// centre only.
pub fn jail_party(s: &mut State, x: u16, y: u16, z: u16) {
    if let Some(l) = s.actors.party.leader() {
        let _ = move_object(s, l, x, y, z);
        s.map.centre = s.objects.location[l.index()];
    }
    todo_stub!("0x1092fc move the party (fallback: leader only)", ())
}

/// [ui] 0x108658 with a string at an absolute image address (PC-relative strings). Fallback:
/// nothing printed.
pub fn print_image(s: &mut State, at: u32) {
    print(s, 0, &[Arg::Image(at)]);
}

/// [map] 0x11d1d4: after a party step, enter or leave a dungeon level / the gargoyle world
/// when the leader stands on a ladder, stairs or hole (the dungeon names). Fallback: no level
/// change.
pub fn level_transition(_s: &mut State) {
    todo_stub!(
        "0x11d1d4 level transition after a step (fallback: none)",
        ()
    )
}
