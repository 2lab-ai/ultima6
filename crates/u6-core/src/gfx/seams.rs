//! What gfx needs from other modules and from the engine, behind the narrowest signatures
//! (03 §6), each forwarding to its owner since the integration (WU-P7).

use super::GfxState;
#[cfg(doc)]
use super::blit;
use super::view::{ViewWindow, WINDOW_TILES};
use crate::engine::Io;
use crate::objects::ObjectId;
use crate::state::State;

/// The window contents as last built by map 0x109e74 and objects 0x10e81e: the tile cache
/// A4-0x1476 (`objects.draw.base`: map tile per cell, 0xff unseen, 0x1bc unlit, flat object
/// tiles over it) and the per-cell object lists (`objects.draw`, first drawn first).
pub(crate) fn view_window(state: &State) -> ViewWindow {
    let d = &state.objects.draw;
    let mut window = ViewWindow::empty();
    for (row, line) in window.tiles.iter_mut().enumerate() {
        for (col, t) in line.iter_mut().enumerate() {
            *t = d.base.get(row * WINDOW_TILES + col).copied().unwrap_or(0);
            window.objects[row * WINDOW_TILES + col] = d.tiles(col, row);
        }
    }
    window
}

/// Animation flag of animdata slot `slot` (A4-0x309b, byte per slot: 0 off, 1 forward,
/// 2 backward). Owner: map ([`crate::map::MapState::anim_enabled`]).
pub(crate) fn anim_mode(map: &crate::map::MapState, slot: usize) -> u8 {
    map.anim_enabled.get(slot).copied().unwrap_or(0)
}

/// 0x102c90's target cursor: when the ui is idle (A4-0x7a32 == 0, A4-0x7a5b == 0) it calls
/// 0x106932(0) and clears A4-0x7a60; when A4-0x7a60 == 1 it draws tile 0x16c (A4-0x7a5d < 0) or
/// 0x16d at (16·A4-0x7a5f, 16·A4-0x7a5e). Returns that (tile, x, y) for the caller to blit
/// into the back buffer. The ui globals live in `ui.input` ([`crate::ui::InputState`]).
pub(crate) fn map_cursor(state: &mut State, io: &Io) -> Option<(u16, u16, u16)> {
    let input = &state.ui.input;
    if input.target_pending == 0 && !input.cursor_by_tab {
        crate::ui::cursor::panel_cursor_in(state, io, 0);
        state.ui.input.targeting = 0;
    }
    let input = &state.ui.input;
    (input.targeting == 1).then(|| {
        let tile = if input.range < 0 { 0x16c } else { 0x16d };
        let (col, row) = input.cursor;
        (tile, (i16::from(col) * 16) as u16, (i16::from(row) * 16) as u16)
    })
}

/// 0x109e74 when 0x102ef0 finds the view flags stale (map, [`crate::map::view::update_view`]).
pub(crate) fn view_flags(state: &mut State) {
    crate::map::view::update_view(state);
}

/// 0x133a1a(`shakes`, 1): 0x102ef0's last step shakes the view `shakes` times by one pixel
/// (ui, [`crate::ui::effects::quake`]), with A4-0x7afc already cleared so its own 0x102ef0 call
/// does not shake again.
pub(crate) fn quake(state: &mut State, io: &Io, shakes: u8) {
    crate::ui::effects::quake(state, io, i16::from(shakes), 1);
}

/// Present the map window: [`blit::dissolve`] over (8, 8)..(0xa7, 0xa7) when `dissolve`, else
/// [`blit::copy_window`] (0x151f5a), from the back buffer into the front (the screen).
pub(crate) fn present_window(io: &Io, gfx: &GfxState, dissolve: bool) {
    io.with_buffers(|back, front| {
        if dissolve {
            super::blit::dissolve(&gfx.tables.dissolve_taps, back, front, (8, 8), (0xa7, 0xa7));
        } else {
            super::blit::copy_window(back, front);
        }
    });
    io.present();
}

/// Objects 0x10ad1a + 0x10aaca: the objects at (x, y, z) in the chain order (multi-tile parts
/// covering the location included).
pub(crate) fn objects_at(state: &mut State, x: i32, y: i32, z: u8) -> Vec<ObjectId> {
    let at = u6_formats::position::Position {
        x: x as u16,
        y: y as u16,
        z,
    };
    crate::objects::objects_at(state, at)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

/// Objects 0x10b922(x, y, z, dir, frame): the serpent body part next to (x, y) in direction
/// `dir` whose frame matches the head's `frame`.
pub(crate) fn serpent_part(
    state: &mut State,
    x: i32,
    y: i32,
    z: u8,
    dir: u16,
    frame: u16,
) -> Option<ObjectId> {
    let slot = crate::objects::find_segment_at(
        state,
        x as u16,
        y as u16,
        u16::from(z),
        usize::from(dir),
        frame,
    );
    ObjectId::from_slot(slot)
}

/// Objects 0x10a5b0(id): the other half of a two-tile creature.
pub(crate) fn other_half(state: &mut State, id: ObjectId) -> Option<ObjectId> {
    ObjectId::from_slot(crate::objects::owner_of_part(state, id.slot()))
}

/// Objects 0x10e81e: rebuild the window object lists after frames changed.
pub(crate) fn rebuild_window_objects(state: &mut State) {
    crate::objects::build_draw_lists(state);
}

/// Type/frame word of `id` (0x137988; objects owns the table).
pub(crate) fn type_frame(state: &State, id: ObjectId) -> u16 {
    state
        .objects
        .type_frame
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Writes the type/frame word of `id` (0x10202e writes the table directly, as here).
pub(crate) fn set_type_frame(state: &mut State, id: ObjectId, word: u16) {
    if let Some(w) = state.objects.type_frame.get_mut(id.index()) {
        *w = word;
    }
}

/// Status byte of actor `id` (A4+0x3d0c; actors owns it).
pub(crate) fn actor_status(state: &State, id: ObjectId) -> u8 {
    state
        .actors
        .attrs
        .status
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Movement-flags byte of actor `id` (0x139588; actors owns it).
pub(crate) fn movement_flags(state: &State, id: ObjectId) -> u8 {
    state
        .actors
        .attrs
        .movement_flags
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Writes the movement-flags byte of actor `id` (0x10202e writes bit 7 directly, as here).
pub(crate) fn set_movement_flags(state: &mut State, id: ObjectId, flags: u8) {
    if let Some(f) = state.actors.attrs.movement_flags.get_mut(id.index()) {
        *f = flags;
    }
}

/// Worktype byte of actor `id` (0x139188; actors owns it).
pub(crate) fn worktype(state: &State, id: ObjectId) -> u8 {
    state.actors.worktype.get(id.index()).copied().unwrap_or(0)
}
