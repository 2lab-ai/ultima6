//! Port-only (spec 20), no original address: smooth motion between cells, the engine's side.
//!
//! With [`Io::set_motion`] on, each framed render of the map window (0x102c90 with D0 = 1, and the
//! cached refresh's present) keeps a [`Shot`]: the scene it drew ([`Scene`], read out of the draw
//! lists 0x10e81e built, each node with its object slot,
//! [`crate::objects::DrawLists::node_slot`]), what it drew in place of a base (the remembered
//! sight), the target cursor and the shown map's picture as a hash. The engine hands a platform,
//! with each publication whose shown map changed, the shot whose picture it is (the newest one
//! kept), or none: a foreign picture (a missile's, a flash's, a quake's, a combat frame played
//! back) ([`crate::Engine::take_motion`]). The platform's [`u6_render::motion::Presenter`] slides
//! between the scenes and [`paint`] draws its plan with the blitter's draws at any pixel.
//!
//! The early picture (spec 20 Design 4, [`preview`]): right after the leader's step the engine
//! builds the view and its draw lists on a copy of the state (0x109e74 and 0x10e81e on the copy,
//! which is dropped) and hands that scene to the platform at once, so the party's slide starts at
//! the key, not at the world turn's picture some 540 ms later.
//!
//! Nothing here writes the game state or draws from the RNG: a scene is read out of the draw lists
//! the render had built, after its present, or out of those of the copy, and the shots live in the
//! [`Io`], so the state, the RNG and the frames the engine publishes are those with the switch off.

use std::rc::Rc;

use u6_formats::gfx::{ANIM_FRAMES, ANIM_PLANE, Span, TILE_H, TILE_W, TileKind};
use u6_render::motion::{Cell, Draw, Node, Placed, Presenter, Rank, Scene, View};
use u6_render::{FLAG_MEMORY, FLAG_OUTLINE, FLAG_OVERLAY, Framebuffer, GREY, Kind, Src};

use super::GfxState;
use super::blit::outline_colour;
use super::cache::Layer;
use super::view::{border_tiles, clock_points};
use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, SHOWN, VIEW_H, VIEW_W};
use crate::objects::{DRAW_NODES, OBJECT_SLOTS};
use crate::state::State;

/// Light overlay tiles (0x10e81e's second pass: 0x1bc + level 1..3).
const OVERLAYS: std::ops::RangeInclusive<u16> = 0x1bd..=0x1bf;
/// The clock's object type (0x102c90 gives its tile hands, [`super::view`]).
const CLOCK_TYPE: usize = 0x9f;

/// One framed render of the map window, as a platform's presenter keeps it.
#[derive(Debug)]
pub struct Shot {
    pub scene: Scene,
    /// Per view cell, the remembered sight's layers drawn in place of its base (spec 18); empty
    /// with the remembered sight off.
    memory: Vec<Vec<Layer>>,
    /// The target cursor (tile, x, y) the render drew over the cells.
    cursor: Option<(u16, u16, u16)>,
    /// The clock's tile (its object gets hands).
    clock: Option<u16>,
    /// The overlay frame the render drew animated tiles in ([`GfxState::frame`]): a fight plays a
    /// render back after later animation ticks.
    frame: u8,
    /// [`shown_hash`] of the screen after the present.
    pub(crate) hash: u64,
}

impl AsRef<Scene> for Shot {
    fn as_ref(&self) -> &Scene {
        &self.scene
    }
}

/// A hash of the colours of the shown map ([`SHOWN`]) of `fb`: FNV-1a's step over its rows taken
/// eight pixels a word.
pub(crate) fn shown_hash(fb: &Framebuffer) -> u64 {
    let (x0, y0, x1, y1) = SHOWN;
    let (xs, ys) = (
        usize::from(x0)..usize::from(x1) + 1,
        usize::from(y0)..=usize::from(y1),
    );
    let w = fb.width();
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    let mut mix = |v: u64| h = (h ^ v).wrapping_mul(0x0100_0000_01b3);
    for y in ys {
        let row = &fb.pixels()[y * w + xs.start..y * w + xs.end];
        let (words, rest) = row.as_chunks::<8>();
        for word in words {
            mix(u64::from_le_bytes(*word));
        }
        for &p in rest {
            mix(u64::from(p));
        }
    }
    h
}

/// Keeps the shot of the framed render just presented, with its remembered layers, cursor and
/// clock tile (what 0x102c90 took before it drew); `snap` when it was presented dissolving or drew
/// XOR plots (the sparkle) its scene does not hold. A render of the window cache that drew no box
/// (`same`: every box's draws those of the last render) keeps the last shot again. Nothing with the
/// switch off.
pub(super) fn record(
    state: &State,
    io: &Io,
    memory: Vec<Vec<Layer>>,
    (cursor, clock): (Option<(u16, u16, u16)>, Option<u16>),
    snap: bool,
    same: bool,
) {
    if !io.motion() {
        return;
    }
    let hash = io.draw(|fb| shown_hash(fb));
    if let Some(last) = io.last_shot().filter(|l| same && l.hash == hash && !snap) {
        io.keep_shot(last);
        return;
    }
    let shot = Shot {
        scene: scene_of(state, snap),
        memory,
        cursor,
        clock,
        frame: state.gfx.frame,
        hash,
    };
    io.keep_shot(Rc::new(shot));
}

/// Spec 20 Design 4: the early picture of the leader's step just made: on a copy of `state`, the
/// view rebuilt ([`crate::map::view::update_view`], 0x109e74, which ends in the draw lists of
/// 0x10e81e: an RNG draw it makes stays on the copy) and the remembered sight taken as a framed
/// render would take it; its shot kept for the engine to hand over ([`Io::keep_preview`]). The
/// copy is dropped; nothing with the switch off, nor in combat mode, where a fight's frames (a
/// missile's flight, a flash) can show the party where it was before the turn's picture does.
pub(crate) fn preview(state: &State, io: &Io) {
    if !io.motion() || state.actors.combat_mode() {
        return;
    }
    let mut copy = state.clone();
    crate::map::view::update_view(&mut copy);
    let window = super::seams::view_window(&copy);
    let memory = if io.sight_memory() {
        super::cache::SightMemory::layers(&mut copy, &window, true)
    } else {
        Vec::new()
    };
    let clock = copy.tables.basetile.base.get(CLOCK_TYPE).copied();
    let shot = Shot {
        scene: scene_of(&copy, false),
        memory,
        cursor: None,
        clock,
        frame: state.gfx.frame,
        hash: 0,
    };
    io.keep_preview(Rc::new(shot));
}

/// The scene of the draw lists as 0x10e81e built them: per view cell its base word and its nodes
/// in list order, each with its slot and rank (a slot's node outside its object's own view cell is
/// a part; a node of no slot a light overlay; a tile with flags2 0x10 on top), and each slot drawn
/// with its world cell (its view cell from the centre, as 0x10e81e places it) and type.
fn scene_of(s: &State, snap: bool) -> Scene {
    let d = &s.objects.draw;
    let c = s.map.centre;
    let origin = (
        i32::from(c.x) - HALF_W as i32,
        i32::from(c.y) - HALF_H as i32,
    );
    let o = &s.objects;
    let view_cell = |slot: i16| {
        let (x, y) = (o.x(slot) as i16, o.y(slot) as i16);
        let cx = x.wrapping_sub(c.x as i16) + HALF_W as i16;
        let cy = y.wrapping_sub(c.y as i16) + HALF_H as i16;
        (i32::from(cx), i32::from(cy))
    };
    let top = |w: u16| {
        let f = s.tables.tileflags.flags2.get(usize::from(w & 0x7ff));
        f.is_some_and(|f| f & 0x10 != 0)
    };
    let mut placed: Vec<Placed> = Vec::new();
    let mut seen = vec![0u64; OBJECT_SLOTS.div_ceil(64)];
    let mut cells = Vec::with_capacity(VIEW_W * VIEW_H);
    for i in 0..VIEW_W * VIEW_H {
        let here = ((i % VIEW_W) as i32, (i / VIEW_W) as i32);
        let mut nodes = Vec::new();
        let mut n = d.cell[i];
        for _ in 0..DRAW_NODES {
            if n == 0 {
                break;
            }
            let k = usize::from(n);
            let (word, slot) = (d.node_tile[k], d.node_slot[k]);
            let rank = if slot >= 0 {
                if view_cell(slot) != here {
                    Rank::Part
                } else if top(word) {
                    Rank::Top
                } else {
                    Rank::Own
                }
            } else if OVERLAYS.contains(&(word & 0x7ff)) {
                Rank::Overlay
            } else if top(word) {
                Rank::Top
            } else {
                Rank::Own
            };
            let slot = u16::try_from(slot).ok();
            if let Some(slot) = slot.filter(|&s| usize::from(s) < OBJECT_SLOTS)
                && seen[usize::from(slot) / 64] & 1 << (slot % 64) == 0
            {
                seen[usize::from(slot) / 64] |= 1 << (slot % 64);
                let (cx, cy) = view_cell(slot as i16);
                placed.push(Placed {
                    slot,
                    cell: (origin.0 + cx, origin.1 + cy),
                    kind: o.kind(slot as i16),
                });
            }
            nodes.push(Node { word, slot, rank });
            n = d.node_next[k];
        }
        cells.push(Cell {
            base: d.base[i],
            nodes,
        });
    }
    placed.sort_unstable_by_key(|p| p.slot);
    Scene {
        leader: s.actors.party.leader().map(|id| id.0),
        centre: (i32::from(c.x), i32::from(c.y), c.z),
        origin,
        cols: VIEW_W,
        rows: VIEW_H,
        corner: (i32::from(MAP_X), i32::from(MAP_Y)),
        cells,
        placed,
        snap,
    }
}

/// The shown map, inclusive, as signed pixels.
#[derive(Clone, Copy)]
struct Clip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Clip {
    const SHOWN: Clip = Clip {
        x0: SHOWN.0 as i32,
        y0: SHOWN.1 as i32,
        x1: SHOWN.2 as i32,
        y1: SHOWN.3 as i32,
    };

    fn holds(&self, x: i32, y: i32) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }
}

/// Writes pixel (x, y) as `f` of what it holds, naming `src`, inside the shown map only.
fn put(fb: &mut Framebuffer, (x, y): (i32, i32), src: Src, f: impl FnOnce(u8) -> u8) {
    if Clip::SHOWN.holds(x, y) {
        let (x, y) = (x as usize, y as usize);
        let p = fb.get(x, y).unwrap_or(0);
        fb.set_src(x, y, f(p), src);
    }
}

fn tile_src(id: u16, ver: u32, (u, v): (usize, usize), flags: u8) -> Src {
    Src {
        ver,
        id,
        u: u as u16,
        v: v as u16,
        kind: Kind::Tile,
        flags,
    }
}

/// The blitter's body 0x153008 ([`super::blit::blit_back`]) at any pixel (x, y), the record's
/// pixels, overlay (at overlay frame `frame`) and outline as it draws them, and their sources;
/// only inside the shown map.
fn blit_at(gfx: &GfxState, fb: &mut Framebuffer, word: u16, (x, y): (i32, i32), frame: u8) {
    let (rec, overlay, masktype) = gfx.record(word);
    let id = gfx.record_of[usize::from(word & 0x7ff)];
    let ver = gfx.cache.version(usize::from(id));
    let pixels = rec.pixels.iter().zip(&rec.transparent).enumerate();
    if masktype & 3 != 0 && word & 0x1800 != 0 {
        let colour = outline_colour(word);
        let shift = match rec.kind {
            TileKind::Object(Span::Right) => 8,
            _ => 0,
        };
        for (i, (&p, &clear)) in pixels {
            let (r, c) = (i / TILE_W, i % TILE_W);
            if !clear && p == 0 && c >= shift {
                let src = tile_src(id, ver, (c, r), FLAG_OUTLINE);
                put(fb, (x + (c - shift) as i32, y + r as i32), src, |_| colour);
            }
        }
        return;
    }
    for (i, (&p, &clear)) in pixels {
        let (r, c) = (i / TILE_W, i % TILE_W);
        if !clear {
            let src = tile_src(id, ver, (c, r), 0);
            put(fb, (x + c as i32, y + r as i32), src, |_| p);
        }
    }
    if masktype & 4 != 0
        && let Some(ov) = overlay
    {
        let rows = &ov.frames[usize::from(frame) % ANIM_FRAMES];
        for (k, bits) in rows.iter().enumerate() {
            let r = usize::from(ov.first_row) + k;
            for c in (0..TILE_W).filter(|c| bits & (0x8000 >> c) != 0) {
                let src = tile_src(id, ver, (c, r), FLAG_OVERLAY);
                put(fb, (x + c as i32, y + r as i32), src, |p| {
                    p | 1 << ANIM_PLANE
                });
            }
        }
    }
}

/// The window render's grey draw of a remembered layer (spec 18, `blit_memory`) at any pixel.
fn memory_at(gfx: &GfxState, fb: &mut Framebuffer, layer: Layer, x: i32, y: i32) {
    let r = usize::from(layer.record);
    let (Some((pixels, transparent, id, ver)), Some(overlay)) =
        (layer.image(gfx), gfx.overlays.get(r))
    else {
        return;
    };
    let src = |c: usize, row: usize, flags: u8| tile_src(id, ver, (c, row), flags | FLAG_MEMORY);
    let pixels = pixels.iter().zip(transparent).take(TILE_W * TILE_H);
    for (i, (&p, &clear)) in pixels.enumerate() {
        let (row, c) = (i / TILE_W, i % TILE_W);
        if !clear {
            put(fb, (x + c as i32, y + row as i32), src(c, row, 0), |_| {
                p | GREY
            });
        }
    }
    if layer.masktype & 4 != 0
        && let Some(ov) = overlay
    {
        let rows = &ov.frames[usize::from(layer.frame) % ANIM_FRAMES];
        for (k, bits) in rows.iter().enumerate() {
            let row = usize::from(ov.first_row) + k;
            if row >= TILE_H {
                break;
            }
            for c in (0..TILE_W).filter(|c| bits & (0x8000 >> c) != 0) {
                let at = (x + c as i32, y + row as i32);
                put(fb, at, src(c, row, FLAG_OVERLAY), |p| p | 1 << ANIM_PLANE);
            }
        }
    }
}

/// The clock hands (0x102bf2, [`clock_points`]) of a clock tile at any pixel, in the clock's
/// colour (a plot sets the pixel; a colour past 15 plots nothing). The game's plot colour is not
/// touched.
fn hands_at(state: &State, fb: &mut Framebuffer, x: i32, y: i32) {
    const AT: u16 = 0x4000;
    let t = &state.gfx.tables;
    let Ok(colour) = u8::try_from(t.clock_colour) else {
        return;
    };
    if colour > 15 {
        return;
    }
    for (px, py) in clock_points(&state.actors.calendar, t, AT, AT) {
        let (dx, dy) = (i32::from(px) - i32::from(AT), i32::from(py) - i32::from(AT));
        put(fb, (x + dx, y + dy), Src::CLASSIC, |_| colour);
    }
}

/// Draws `presenter`'s plan for `view` over the shown map of `fb` (a copy of the published frame):
/// the shown map cleared, the cells and objects at their pixels (a base the remembered sight drew
/// in grey drawn so again), then the newest render's target cursor and the border at their places,
/// as 0x102c90 draws them last. Animated tiles show the newest render's overlay frame.
pub fn paint(state: &State, presenter: &Presenter<Shot>, view: &View, fb: &mut Framebuffer) {
    let Some(newest) = presenter.history().first() else {
        return;
    };
    let gfx = &state.gfx;
    let c = Clip::SHOWN;
    for y in c.y0..=c.y1 {
        for x in c.x0..=c.x1 {
            fb.set(x as usize, y as usize, 0);
        }
    }
    let frame = newest.frame;
    for op in presenter.plan(view) {
        let at = (op.x, op.y);
        match op.draw {
            Draw::Base { scene, cell } => {
                let shot = &presenter.history()[scene];
                match shot.memory.get(cell).filter(|m| !m.is_empty()) {
                    Some(layers) => {
                        for &layer in layers {
                            memory_at(gfx, fb, layer, op.x, op.y);
                        }
                    }
                    None => blit_at(gfx, fb, shot.scene.cells[cell].base, at, frame),
                }
            }
            Draw::Word(w) => {
                blit_at(gfx, fb, w, at, frame);
                if Some(w & 0x7ff) == newest.clock {
                    hands_at(state, fb, op.x, op.y);
                }
            }
        }
    }
    if let Some((tile, x, y)) = newest.cursor {
        blit_at(gfx, fb, tile, (i32::from(x), i32::from(y)), frame);
    }
    for (tile, x, y) in border_tiles() {
        blit_at(gfx, fb, tile, (i32::from(x), i32::from(y)), frame);
    }
}
