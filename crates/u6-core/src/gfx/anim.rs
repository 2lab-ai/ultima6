//! Tile animation: frame substitution 0x152cd2, overlay frame 0x152dca, shore merge 0x152dac and
//! the per-frame tick 0x10202e.

use u6_formats::gfx::{PLANES, TILE_COUNT, TILE_W, TileKind};

use super::GfxState;
use super::idle::actor_idle_frames;
use super::seams;
use super::tables::SHORE_MERGES;
use crate::state::State;

/// Original: 0x152cd2 (tile substitution: tile `target` draws what tile `source` draws)
///
/// Skipped unless `source < 0x200` (`cmp.w #0x200,D1w` / `bcc`). Copies the masktype byte and the
/// record pointer, so a later substitution from `target` copies the record `source` had then.
/// For object tiles the original also makes the record resident (0x152c44) and copies the cache
/// slot pointer (0x14f2c8) and its LRU word (0x161bd0); the port holds every record, so those
/// copies change nothing drawn. A `target` past the 2048-entry tables (the original would write
/// beyond them) is ignored.
pub fn substitute(gfx: &mut GfxState, target: u16, source: u16) {
    if source >= 0x200 {
        return;
    }
    let (t, s) = (usize::from(target), usize::from(source));
    if t >= TILE_COUNT {
        return;
    }
    gfx.masktype[t] = gfx.masktype[s];
    gfx.record_of[t] = gfx.record_of[s];
}

/// Original: 0x152dca (overlay frame: store `value & 7` at 0x15803e)
pub fn set_frame(gfx: &mut GfxState, value: u8) {
    gfx.frame = value & 7;
}

/// Original: 0x152dac (shore merge: 0x152d52 once per plane, D6 stepping by the plane size)
///
/// Merges the animated water tiles into the 32 shore tiles. The five passes differ only in the
/// plane they touch; on chunky records each pass rewrites one colour bit. A record whose pixels
/// it changes gets a new version ([`super::cache::RenderCache::record_changed`]).
pub fn merge_shores(gfx: &mut GfxState) {
    for plane in 0..PLANES as u8 {
        merge_plane(gfx, plane);
    }
}

/// Original: 0x152d52 (one shore pass: 32 masked merges in one plane)
///
/// Merge `i` takes the record of tile `shore_src[i] / 4` (the word is an offset into the pointer
/// table 0x15559e) into the record of tile `shore_dst[i] / 4`. Its mask record is at
/// `block2 + (word i of block 2, LE) + 0x400` (16-bit sum, sign-extended `lea`).
fn merge_plane(gfx: &mut GfxState, plane: u8) {
    for i in 0..SHORE_MERGES {
        let tile =
            |w: u16| (w.is_multiple_of(4) && usize::from(w / 4) < TILE_COUNT).then_some(w / 4);
        let (Some(src), Some(dst)) = (tile(gfx.tables.shore_src[i]), tile(gfx.tables.shore_dst[i]))
        else {
            continue;
        };
        let Some((skip, masks)) = shore_mask(&gfx.animmask.block2, i) else {
            continue;
        };
        let (src, dst) = (
            usize::from(gfx.record_of[usize::from(src)]),
            usize::from(gfx.record_of[usize::from(dst)]),
        );
        merge_rows(gfx, src, dst, plane, skip, &masks);
    }
}

/// The mask record of merge `i` in animmask block 2: `[skip bytes][count]` + `count` BE words.
pub(crate) fn shore_mask(block2: &[u8], i: usize) -> Option<(u8, Vec<u16>)> {
    let w = u16::from_le_bytes([*block2.get(2 * i)?, *block2.get(2 * i + 1)?]);
    let off = usize::try_from(w.wrapping_add(0x400) as i16).ok()?;
    let (&skip, &count) = (block2.get(off)?, block2.get(off + 1)?);
    let body = block2.get(off + 2..off + 2 + 2 * usize::from(count))?;
    let masks = body
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    Some((skip, masks))
}

/// Original: 0x152d30 (masked row merge: `skip` bytes into the stored rows, then per row word
/// `dst = (src & mask) | (dst & !mask)`)
///
/// Stored row `skip/2 + k` of a map record is pixel row `first_row + skip/2 + k`. Rows past
/// either record's stored rows (the original would read or write the following record), an odd
/// `skip` (rows straddling words) and non-map records are left alone.
fn merge_rows(gfx: &mut GfxState, src: usize, dst: usize, plane: u8, skip: u8, masks: &[u16]) {
    if !skip.is_multiple_of(2) {
        return;
    }
    let (s, d) = (&gfx.records[src], &gfx.records[dst]);
    if s.kind != TileKind::Map || d.kind != TileKind::Map {
        return;
    }
    let stored = |t: &u6_formats::gfx::Tile, j: usize| {
        (j < usize::from(t.rows)).then(|| usize::from(t.first_row) + j)
    };
    let bit = 1u8 << plane;
    let mut out = d.pixels.clone();
    let mut changed = false;
    for (k, &mask) in masks.iter().enumerate() {
        let j = usize::from(skip / 2) + k;
        let (Some(rs), Some(rd)) = (stored(s, j), stored(d, j)) else {
            continue;
        };
        for c in 0..TILE_W {
            if mask & (0x8000 >> c) != 0 {
                let from = s.pixels[rs * TILE_W + c] & bit;
                let p = &mut out[rd * TILE_W + c];
                let merged = (*p & !bit) | from;
                changed |= merged != *p;
                *p = merged;
            }
        }
    }
    if changed {
        gfx.records[dst].pixels = out;
        gfx.cache.record_changed(dst);
    }
}

/// Original: 0x10202e (animation tick: animdata frames, shore merge, actor idle frames)
///
/// Skipped while [`GfxState::anim_paused`] (A4-0x7b64). Otherwise: counter A4-0x7b00 += 1; for
/// each of the `count` animdata slots the frame tile is `first_anim_frame`, plus
/// `(counter & and_mask) >> shift` when the slot's flag (A4-0x309b) is 1, or
/// `(!counter & and_mask) >> shift` when it is 2 (`asr.w`; the value is below 0x100), and
/// 0x152cd2 makes `tile_to_animate` draw it; then the shore merge on even counters (bit 0 of the
/// counter's low byte A4-0x7aff clear); then the actor idle frames ([`super::idle`]), which run
/// only while [`GfxState::actor_anim_due`] (A4-0x7afa) is set; the ui's input wait sets it on
/// each idle step (0x105c8e). The flag is cleared, and if a frame changed the window object
/// lists are rebuilt (0x10e81e).
pub fn anim_tick(state: &mut State) {
    let State {
        gfx, tables, map, ..
    } = state;
    if gfx.anim_paused != 0 {
        return;
    }
    gfx.anim_counter = gfx.anim_counter.wrapping_add(1);
    let counter = gfx.anim_counter;
    let anim = &tables.anim;
    for (i, slot) in anim.slots.iter().enumerate().take(usize::from(anim.count)) {
        let mask = u16::from(slot.and_mask);
        let step = |v: u16| v.checked_shr(u32::from(slot.shift % 64)).unwrap_or(0);
        let frame = match seams::anim_mode(map, i) {
            1 => slot.first_anim_frame.wrapping_add(step(counter & mask)),
            2 => slot.first_anim_frame.wrapping_add(step(!counter & mask)),
            _ => slot.first_anim_frame,
        };
        substitute(gfx, slot.tile_to_animate, frame);
    }
    if counter & 1 == 0 {
        merge_shores(gfx);
    }
    let changed = actor_idle_frames(state);
    state.gfx.actor_anim_due = 0;
    if changed {
        seams::rebuild_window_objects(state);
    }
}
