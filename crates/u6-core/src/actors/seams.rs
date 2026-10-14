//! Calls from the actor code into functions other modules own.
//!
//! Each seam is the narrowest signature the actor code needs; since the integration (WU-P7) each
//! forwards to its owner. The targets outside milestone 1 (sky animation, music) keep a
//! `todo_stub!` with the contract and the fallback. Proposed owner in brackets.

use u6_formats::position::Position;

use std::cell::RefCell;

use crate::engine::{Io, with_current_io};
use crate::main_loop::PrintArg;
use crate::objects::{OBJECT_SLOTS, ObjectId};
use crate::state::State;

/// One object found at a location: its id and the part of a multi-tile object that covers the
/// location (the original leaves it in A4-0x78dc; 0 = the object's own tile).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: ObjectId,
    pub part: u16,
}

/// Type + frame word of an object (0x137988).
pub fn obj_word(s: &State, id: ObjectId) -> u16 {
    s.objects.type_frame.get(id.index()).copied().unwrap_or(0)
}

/// Writes the type + frame word of an object.
pub fn set_obj_word(s: &mut State, id: ObjectId, word: u16) {
    if let Some(w) = s.objects.type_frame.get_mut(id.index()) {
        *w = word;
    }
}

/// Location of an object (0x135588).
pub fn obj_pos(s: &State, id: ObjectId) -> Position {
    s.objects
        .location
        .get(id.index())
        .copied()
        .unwrap_or_default()
}

/// [objects] 0x10ad1a + 0x10aaca: every object at (x, y, z), with the covering part. A read-only
/// walk (the state's cursor is left alone).
pub fn objects_at(s: &State, x: u16, y: u16, z: u16) -> Vec<Found> {
    let pos = Position { x, y, z: z as u8 };
    crate::objects::objects_at_view(s, pos)
        .into_iter()
        .map(|(id, part)| Found { id, part })
        .collect()
}

/// [objects] 0x10b05a + 0x10af74: every object in the window rectangle on the current level.
pub fn objects_in_rect(s: &mut State, x0: u16, y0: u16, x1: u16, y1: u16, z: u16) -> Vec<ObjectId> {
    crate::map::seams::objects_in_rect(s, x0 as i16, y0 as i16, x1 as i16, y1 as i16, z)
}

/// [objects] 0x10c9d2: moves an object or actor to (x, y, z), keeping the region lists;
/// negative = failed (the player step then prints "Object mismatch").
pub fn move_object(s: &mut State, id: ObjectId, x: u16, y: u16, z: u16) -> i16 {
    crate::objects::move_to(s, id.slot(), x, y, z)
}

/// [objects] 0x131196: index of a creature type in the A4-0x8000 list, -1 when not listed.
pub fn creature_index(s: &State, obj_type: u16) -> i16 {
    crate::objects::monster_index(&s.objects, obj_type)
}

/// [objects] 0x10ae52 + 0x10aed6: objects inside or carried by `holder`, in list order.
pub fn contents(s: &mut State, holder: ObjectId) -> Vec<ObjectId> {
    let o = &mut s.objects;
    let mut out = Vec::new();
    let mut c = crate::objects::first_child(o, holder.slot());
    while let Some(id) = ObjectId::from_slot(c) {
        out.push(id);
        if out.len() > OBJECT_SLOTS {
            break;
        }
        c = crate::objects::next_child(o);
    }
    out
}

/// [objects] 0x10c1d6: removes an object from the world.
pub fn remove_object(s: &mut State, id: ObjectId) {
    crate::objects::remove(s, id.slot());
}

/// [map] 0x10996c: map tile at (x, y, z) (window cache, else the chunk); 0 off the map or with no
/// map loaded.
pub fn tile_at(s: &State, x: u16, y: u16, z: u16) -> u8 {
    if s.map.world.surface.is_empty() {
        return 0;
    }
    s.map
        .tile_at(usize::from(x), usize::from(y), usize::from(z))
        .unwrap_or(0)
}

/// [map] origin and coordinate mask of the 40 × 40 map-window tile cache (A4-0x2ef8, A4-0x2ef6,
/// A4-0x796c; set when 0x108d10 rebuilds it).
pub fn window_origin(s: &State) -> (u16, u16, u16) {
    let [x, y] = s.map.window_origin;
    (x, y, s.map.world_mask)
}

/// [map] byte (col, row) of the map-window tile cache A4+0x4b0a (filled by 0x108d10).
pub fn window_tile(s: &State, col: u16, row: u16) -> u8 {
    let i = crate::map::cell(usize::from(col), usize::from(row));
    match s.map.window_tiles.get(i) {
        Some(&t) if col < crate::map::WINDOW as u16 => t,
        _ => {
            let (x0, y0, m) = window_origin(s);
            let z = u16::from(s.map.centre.z);
            tile_at(s, x0.wrapping_add(col) & m, y0.wrapping_add(row) & m, z)
        }
    }
}

/// [map] 0x109e74: recomputes the view flags (blocking / light / visible).
pub fn refresh_view_flags(s: &mut State) {
    crate::map::view::update_view(s);
}

/// [map] 0x108f90: rebuilds the region and the map-window cache around the leader.
pub fn rebuild_window(s: &mut State) {
    crate::map::region::refresh_region(s);
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
    /// The string the pointer at A4 + offset points to (a name table entry).
    Pointer(i32),
}

/// The bytes of a string argument; `None` for a word.
fn arg_text(s: &State, io: &Io, a: Arg) -> Option<Vec<u8>> {
    Some(match a {
        Arg::PartyName(i) => s
            .actors
            .party
            .names
            .get(usize::from(i))
            .map_or_else(Vec::new, |n| n.as_bytes().to_vec()),
        Arg::Name(id) => crate::objects::display_name(s, id.slot()).into_bytes(),
        Arg::Text(off) => {
            crate::main_loop::game_string(io, crate::image::A4.wrapping_add_signed(off))
        }
        Arg::Image(at) => crate::main_loop::game_string(io, at),
        Arg::Pointer(off) => {
            crate::main_loop::pointed_string(io, crate::image::A4.wrapping_add_signed(off))
        }
        Arg::Word(_) => return None,
    })
}

/// [ui] 0x108658: formats the data-hunk string at A4 + `text` with `args` into the message scroll
/// (strings stay in the user's `game`). Queued for the command loop to print: the actor code is
/// synchronous and the ui's printf may wait on a more prompt.
pub fn print(s: &mut State, text: i32, args: &[Arg]) {
    let line = with_current_io(|io| {
        let mut args = args.iter();
        let fmt = if text == 0 {
            args.next()
                .and_then(|&a| arg_text(s, io, a))
                .unwrap_or_default()
        } else {
            crate::main_loop::game_string(io, crate::image::A4.wrapping_add_signed(text))
        };
        let rest: Vec<PrintArg> = args
            .map(|&a| match a {
                Arg::Word(w) => PrintArg::Word(w as u16),
                other => PrintArg::Str(arg_text(s, io, other).unwrap_or_default()),
            })
            .collect();
        (fmt, rest)
    });
    if let Some((fmt, rest)) = line {
        crate::main_loop::queue_print(s, fmt, rest);
    }
}

/// [ui] 0x108658 with an object's display name (0x10eef8) as the format.
pub fn print_name(s: &mut State, id: ObjectId) {
    print(s, 0, &[Arg::Name(id)]);
}

/// [ui] 0x108658 with a string at an absolute image address (PC-relative strings).
pub fn print_image(s: &mut State, at: u32) {
    print(s, 0, &[Arg::Image(at)]);
}

/// [main] 0x102f2a: after an actor's turn, redraws the view when A4-0x6a8e asks for it.
pub fn after_turn_refresh(s: &mut State) {
    crate::main_loop::after_turn_refresh(s);
}

/// [main] 0x103fd4: makes party member `member` the leader (view centre, window rebuild).
pub fn switch_leader(s: &mut State, member: u8) {
    crate::main_loop::switch_leader(s, member);
}

/// [main] 0x1032b6: hazard of the tile under an actor (the clock runs it once per minute).
pub fn actor_status_tick(s: &mut State, actor: u16) {
    crate::main_loop::actor_status_tick(s, actor);
}

/// [main] 0x10355c: moongate objects appear / vanish with the moons.
pub fn moongates(s: &mut State) {
    crate::main_loop::moongates(s);
}

/// [main] 0x103632: hourly wind change (draws the RNG when `calm` is 0).
pub fn wind_change(s: &mut State, calm: u16) {
    crate::main_loop::wind_change(s, calm);
}

/// [gfx] 0x152cd2: tile `tile` draws `frame`'s picture (the sky tile for the hour).
pub fn sky_tile(s: &mut State, tile: u16, frame: u16) {
    crate::gfx::anim::substitute(&mut s.gfx, tile, frame);
}

/// [ui] 0x133b28: redraws the sun and moon strip, on the polling engine's screen.
pub fn draw_sky_strip(s: &mut State) {
    with_current_io(|io| crate::ui::panels::sky_bar(s, io));
}

/// [ui] 0x107dd8 + 0x107e00 around the date line (0x10368a): before it the screen window
/// (A4-0x2f2e) becomes current with the cursor at column 2, row 0x15 (under the map window);
/// after it the message scroll (A4-0x2f13). Queued in order with the line.
pub fn date_line_window(s: &mut State, before: bool) {
    use crate::main_loop::Pending;
    use crate::ui::WindowId;
    let q = &mut s.main.pending_print;
    if before {
        q.push(Pending::Window(WindowId::Screen));
        q.push(Pending::Cursor(2, 0x15));
    } else {
        q.push(Pending::Window(WindowId::Messages));
    }
}

/// [gfx] 0x102c90: renders and presents the map window, on the polling engine's screen.
pub fn present_map(s: &mut State) {
    with_current_io(|io| crate::gfx::render_window(s, io, true));
}

/// [main] 0x101ff6: sets the animation flag of the animdata slot animating `tile` (1 = on).
pub fn set_tile_animation(s: &mut State, tile: u16, on: bool) {
    crate::main_loop::set_tile_animation(s, tile, u8::from(on));
}

/// [objects] 0x10a5b0: the other half of a two-tile creature, -1 if none.
pub fn other_half(s: &mut State, actor: u16) -> i16 {
    crate::objects::owner_of_part(s, actor as i16)
}

/// [objects] quality byte of an object (low byte of the word at A4+0x250c + 2 × object: torch
/// burn time).
pub fn obj_quality(s: &State, id: ObjectId) -> u8 {
    s.objects.qq_of(id.slot()) as u8
}

/// [objects] writes the quality byte of an object.
pub fn set_obj_quality(s: &mut State, id: ObjectId, q: u8) {
    if let Some(w) = s.objects.qq.get_mut(id.index()) {
        *w = (*w & 0xff00) | u16::from(q);
    }
}

/// [combat] 0x121b08: distance from `actor` to (x, y) (the larger axis distance).
pub fn distance(s: &State, actor: u16, x: u16, y: u16) -> i16 {
    crate::combat::leaves::distance(s, actor as i16, x as i16, y as i16)
}

/// [ui] 0x10790a: waits for a command key and returns its game code; the queued lines are
/// printed first so the question shows before the wait.
pub async fn read_key(state: &RefCell<State>, io: &Io) -> u16 {
    crate::main_loop::flush_prints(state, io).await;
    crate::ui::wait::read_key(state, io).await
}

/// [converse] 0x1127c4 (Talk: `speaker` = party member, `npc` = who talks; `announce` prints
/// the NPC's name first).
pub async fn talk(state: &RefCell<State>, io: &Io, speaker: u16, npc: u16, announce: bool) {
    crate::main_loop::flush_prints(state, io).await;
    let cx = crate::converse::Cx::new(state, io);
    crate::converse::talk(&cx, speaker as u8, npc as i16, announce).await;
}

/// [ui] 0x133b0e: full redraw of the view (the window caches reset 0x133a96, the flags, the
/// map window, the tile cache, the view flags).
pub fn full_redraw(s: &mut State) {
    if with_current_io(|io| crate::ui::effects::redraw_view(s, io)).is_none() {
        crate::ui::effects::reset_window(s);
        s.map.view_dirty = 0;
        s.gfx.dissolve = 1;
        crate::map::region::rebuild_window(s);
        crate::map::view::update_view(s);
    }
}

/// [main] 0x102ef0: post-step refresh (rebuild the view and present).
pub fn refresh_view(s: &mut State) {
    with_current_io(|io| crate::gfx::refresh_view(s, io));
}

/// [sound] 0x110912: picks and starts the song for the current area.
pub fn area_music(s: &mut State) {
    crate::sound::play_area_song(s);
}

/// [map] 0x1092fc: moves the party to (x, y, z) (jail).
pub fn jail_party(s: &mut State, x: u16, y: u16, z: u16) {
    crate::map::travel::teleport(s, x, y, z, 0);
}

/// [map] 0x11d1d4: after a party step, enter or leave a dungeon level / the gargoyle world
/// when the leader stands on a ladder, stairs or hole.
pub fn level_transition(s: &mut State) {
    crate::map::travel::after_step(s);
}
