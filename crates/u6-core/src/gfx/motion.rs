//! Port-only (spec 20), no original address: smooth motion between cells, the engine's side.
//!
//! With [`crate::Engine::set_motion`] on, each framed render of the map window (0x102c90 with D0 = 1, and the
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
//! Nothing here writes the game state or draws from the authoritative RNG (the early picture's
//! draws, if any, are the copy's): a scene is read out of the draw lists the render had built, after
//! its present, or out of those of the copy, and the shots live in the [`Io`], so the state, the
//! RNG and the frames the engine publishes are those with the switch off.

use std::rc::Rc;

use u6_formats::gfx::{ANIM_FRAMES, ANIM_PLANE, Span, TILE_W, TileKind};
use u6_render::motion::{
    Cell, Draw, LayerPixels, Layers, Node, Op, Part, Placed, Presenter, Rank, SNAP_PX, Scene, View,
};
use u6_render::{FLAG_OUTLINE, FLAG_OVERLAY, Framebuffer, Kind, Src};

use super::GfxState;
use super::blit::outline_colour;
use super::cache::Shown as SightShown;
use super::view::{CLOCK_TYPE, border_tiles, clock_points};
use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, SHOWN, VIEW_H, VIEW_W};
use crate::objects::{DRAW_NODES, OBJECT_SLOTS};
use crate::state::State;

/// Light overlay tiles (0x10e81e's second pass: 0x1bc + level 1..3).
const OVERLAYS: std::ops::RangeInclusive<u16> = 0x1bd..=0x1bf;

/// One framed render of the map window, as a platform's presenter keeps it.
#[derive(Debug)]
pub struct Shot {
    pub scene: Scene,
    /// Per view cell, what the remembered sight draws in grey over its base, with its fade (spec
    /// 18: no layers where it draws none); empty with the remembered sight off.
    pub(super) memory: Vec<SightShown>,
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

impl Shot {
    /// A shot of `scene` with no remembered sight, cursor or clock, its animated tiles in overlay
    /// frame 0.
    fn of_scene(scene: Scene) -> Shot {
        Shot {
            scene,
            memory: Vec::new(),
            cursor: None,
            clock: None,
            frame: 0,
            hash: 0,
        }
    }

    /// A shot of an empty scene whose shown map hashes to `hash` (tests).
    #[cfg(test)]
    pub(crate) fn for_test(hash: u64) -> Rc<Shot> {
        let scene = Scene {
            leader: None,
            centre: (0, 0, 0),
            origin: (0, 0),
            cols: 0,
            rows: 0,
            corner: (0, 0),
            cells: Vec::new(),
            placed: Vec::new(),
            snap: false,
        };
        Rc::new(Shot {
            hash,
            ..Shot::of_scene(scene)
        })
    }
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
/// XOR plots (the sparkle) its scene does not hold, or a load is not yet followed by a published
/// scene ([`Io::break_motion`]). A render of the window cache that drew no box
/// (`same`: every box's draws those of the last render) keeps the last shot again. Nothing with the
/// switch off.
pub(super) fn record(
    state: &State,
    io: &Io,
    memory: Vec<SightShown>,
    (cursor, clock): (Option<(u16, u16, u16)>, Option<u16>),
    snap: bool,
    same: bool,
) {
    if !io.motion() {
        return;
    }
    if io.warm_early_picture() {
        drop(std::hint::black_box(state.clone()));
    }
    let snap = snap | io.motion_break();
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

/// Port-only (a platform's synthetic scenes, tests): keeps `scene` as the shot of the render just
/// presented, as [`record`] keeps a framed render's (its picture the shown map of the back buffer
/// now), with no remembered sight, cursor or clock; nothing with the switch off. The engine then
/// hands it over with the publication of that picture ([`crate::Engine::take_motion`]).
pub fn keep_scene(io: &Io, scene: Scene) {
    if !io.motion() {
        return;
    }
    let hash = io.draw(|fb| shown_hash(fb));
    io.keep_shot(Rc::new(Shot {
        hash,
        ..Shot::of_scene(scene)
    }));
}

/// Spec 20 Design 4: the early picture of the leader's step just made: on a copy of `state`, the
/// view rebuilt ([`crate::map::view::update_view`], 0x109e74, which ends in the draw lists of
/// 0x10e81e: an RNG draw it makes stays on the copy) and the remembered sight read as a framed
/// render of the copy would draw it ([`super::cache::SightMemory::shown_in`] of the state's
/// memory, unchanged: the cells in sight are drawn live, so what this render would remember
/// changes nothing it shows); its shot kept for the engine to hand over ([`Io::keep_preview`]),
/// snapping while a load is not yet followed by a published scene ([`Io::break_motion`]). The
/// copy is dropped; nothing with the switch off, nor in combat mode, where a fight's frames (a
/// missile's flight, a flash) can show the party where it was before the turn's picture does.
pub(crate) fn preview(state: &State, io: &Io) {
    if !io.motion() || state.actors.combat_mode() {
        return;
    }
    let mut copy = state.clone();
    crate::map::view::update_view(&mut copy);
    let window = super::seams::view_window(&copy);
    // The copy's cache holds no remembered sight (a clone of the cache drops it): what the
    // render would draw in grey is read from the state's, round the copy's view and at its time.
    let memory = if io.sight_memory() {
        state.gfx.cache.memory.shown_in(&copy, &window)
    } else {
        Vec::new()
    };
    let clock = copy.tables.basetile.base.get(CLOCK_TYPE).copied();
    let shot = Shot {
        scene: scene_of(&copy, io.motion_break()),
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

/// What the draws at any pixel write into: the shown map of a frame ([`Shown`]), or a layer of a
/// slide ([`LayerPixels`]).
pub(super) trait Canvas {
    /// Writes pixel (x, y), in frame pixels, as `f` of what it holds, naming `src`, where the
    /// canvas holds it.
    fn put(&mut self, at: (i32, i32), src: Src, f: impl FnOnce(u8) -> u8);
}

/// A whole frame (the window render's back buffer).
impl Canvas for Framebuffer {
    fn put(&mut self, (x, y): (i32, i32), src: Src, f: impl FnOnce(u8) -> u8) {
        if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
            && let Some(p) = self.get(x, y)
        {
            self.set_src(x, y, f(p), src);
        }
    }
}

/// A frame drawn inside its shown map only.
struct Shown<'a>(&'a mut Framebuffer);

impl Canvas for Shown<'_> {
    fn put(&mut self, (x, y): (i32, i32), src: Src, f: impl FnOnce(u8) -> u8) {
        if Clip::SHOWN.holds(x, y) {
            let (x, y) = (x as usize, y as usize);
            let p = self.0.get(x, y).unwrap_or(0);
            self.0.set_src(x, y, f(p), src);
        }
    }
}

impl Canvas for LayerPixels {
    fn put(&mut self, at: (i32, i32), src: Src, f: impl FnOnce(u8) -> u8) {
        if let Some(i) = self.index(at) {
            self.pixels[i] = f(self.pixels[i]);
            self.src[i] = src;
            self.drawn[i] = true;
        }
    }
}

fn put(fb: &mut impl Canvas, at: (i32, i32), src: Src, f: impl FnOnce(u8) -> u8) {
    fb.put(at, src, f);
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
/// only where `fb` holds them.
fn blit_at(gfx: &GfxState, fb: &mut impl Canvas, word: u16, (x, y): (i32, i32), frame: u8) {
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

/// The clock hands (0x102bf2, [`clock_points`]) of a clock tile at any pixel, in the clock's
/// colour (a plot sets the pixel; a colour past 15 plots nothing). The game's plot colour is not
/// touched.
fn hands_at(state: &State, fb: &mut impl Canvas, x: i32, y: i32) {
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
    let c = Clip::SHOWN;
    for y in c.y0..=c.y1 {
        for x in c.x0..=c.x1 {
            fb.set(x as usize, y as usize, 0);
        }
    }
    let fb = &mut Shown(fb);
    draw_ops(state, presenter, &presenter.plan(view), fb);
    frame_over(state, newest, fb);
}

/// The ops of a plan or a layer ([`Presenter::plan`], [`Presenter::layers`]) into `fb`, in the
/// newest render's overlay frame.
fn draw_ops(state: &State, presenter: &Presenter<Shot>, ops: &[Op], fb: &mut impl Canvas) {
    let Some(newest) = presenter.history().first() else {
        return;
    };
    let gfx = &state.gfx;
    let frame = newest.frame;
    for op in ops {
        let at = (op.x, op.y);
        match op.draw {
            Draw::Base { scene, cell } => {
                // As the window render's `draw_remembered`: the base (black for a remembered
                // cell), then the remembered layers the fade keeps.
                let shot = &presenter.history()[scene];
                blit_at(gfx, fb, shot.scene.cells[cell].base, at, frame);
                if let Some(m) = shot.memory.get(cell) {
                    for &layer in &m.layers {
                        super::view::blit_memory(gfx, fb, layer, at, m.fade);
                    }
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
}

/// What 0x102c90 draws last over the cells of `shot`'s render: its target cursor, then the border.
fn frame_over(state: &State, shot: &Shot, fb: &mut impl Canvas) {
    let gfx = &state.gfx;
    if let Some((tile, x, y)) = shot.cursor {
        blit_at(gfx, fb, tile, (i32::from(x), i32::from(y)), shot.frame);
    }
    for (tile, x, y) in border_tiles() {
        blit_at(gfx, fb, tile, (i32::from(x), i32::from(y)), shot.frame);
    }
}

/// Spec 20 Design 5: what a layer of a slide shows ([`slide_layers`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The static words, at the camera's offset.
    Static,
    /// Mover `i`'s own words ([`u6_render::motion::Layers::movers`]), at its offset.
    Mover(usize),
    /// The words over mover `i`, at the camera's offset.
    Over(usize),
    /// The cursor and the border, at rest.
    Frame,
}

/// Spec 20 Design 5: one layer of a slide: what it shows and its pixels at rest.
#[derive(Clone, Debug)]
pub struct SlideLayer {
    pub role: Role,
    /// Shared: a platform keeps the static and frame layers' pixels of the layers it made last
    /// to tell what the next ones change.
    pub pixels: Rc<LayerPixels>,
}

/// Spec 20 Design 5: a slide's layers ([`slide_layers`]) and the parts of the presenter's layers
/// they were made of ([`Presenter::layers`]: the movers' places order them).
#[derive(Clone, Debug)]
pub struct Slide {
    pub layers: Layers,
    pub parts: Vec<SlideLayer>,
}

/// Spec 20 Design 5: the slide's layers for the HD path, rasterized once per generation of
/// `presenter` ([`Presenter::generation`]) by the draws [`paint`] makes, at rest
/// ([`Presenter::layers`]), in this order: the static words over the newest scene's view and a
/// cell round it, over colour 0 (as [`paint`] clears the shown map), every pixel drawn (one rect
/// whatever cells the scenes hold, so the next layers' changes are found against it); the cursor
/// and the border over the drawn grid; per mover its own words over clear pixels, and the words
/// over it drawn over the static layer's pixels there (an overlay's OR takes the colour under it),
/// only those words' pixels kept. None without a scene.
pub fn slide_layers(state: &State, presenter: &Presenter<Shot>) -> Option<Slide> {
    // The static layer covers the shown map wherever the camera is drawn: it reaches a cell past
    // the view, and the shown map lies half a cell inside the view ([`SHOWN`]), against a camera
    // drawn at most [`SNAP_PX`] from rest.
    const _: () = assert!(
        SNAP_PX <= (SHOWN.0 - MAP_X + 16) as f64
            && SNAP_PX <= (SHOWN.1 - MAP_Y + 16) as f64
            && SNAP_PX <= (MAP_X as usize + VIEW_W * 16 + 15 - SHOWN.2 as usize) as f64
            && SNAP_PX <= (MAP_Y as usize + VIEW_H * 16 + 15 - SHOWN.3 as usize) as f64,
        "the static layer covers the shown map at the camera's farthest"
    );
    let newest = presenter.history().first()?;
    let layers = presenter.layers();
    let n = &newest.scene;
    let raster = |ops: &[Op], under: Option<&LayerPixels>| {
        let (x, y, w, h) = LayerPixels::bounds(ops)?;
        let mut px = LayerPixels::clear(x, y, w, h);
        if let Some(under) = under {
            for (i, p) in px.pixels.iter_mut().enumerate() {
                let at = (x + (i % w) as i32, y + (i / w) as i32);
                if let Some(j) = under.index(at) {
                    *p = under.pixels[j];
                }
            }
        }
        draw_ops(state, presenter, ops, &mut px);
        Some(px)
    };
    let mut statics = LayerPixels::clear(
        n.corner.0 - 16,
        n.corner.1 - 16,
        (n.cols + 2) * 16,
        (n.rows + 2) * 16,
    );
    statics.drawn.fill(true);
    draw_ops(state, presenter, &layers.statics, &mut statics);
    let (w, h) = (VIEW_W * 16, VIEW_H * 16);
    let mut frame = LayerPixels::clear(i32::from(MAP_X), i32::from(MAP_Y), w, h);
    frame_over(state, newest, &mut frame);
    let mut out = Vec::new();
    for (i, m) in layers.movers.iter().enumerate() {
        if let Some(pixels) = raster(&m.ops, None) {
            out.push(SlideLayer {
                role: Role::Mover(i),
                pixels: Rc::new(pixels),
            });
        }
        if let Some(pixels) = raster(&m.over, Some(&statics)) {
            out.push(SlideLayer {
                role: Role::Over(i),
                pixels: Rc::new(pixels),
            });
        }
    }
    out.splice(
        0..0,
        [
            SlideLayer {
                role: Role::Static,
                pixels: Rc::new(statics),
            },
            SlideLayer {
                role: Role::Frame,
                pixels: Rc::new(frame),
            },
        ],
    );
    Some(Slide { layers, parts: out })
}

/// Spec 20 Design 5: where each layer of `slide` is drawn for `view`, in draw order: (index in
/// [`Slide::parts`], frame pixel of its top left). The static layer at `-camera`, the movers by
/// their in-between y then x at `-camera` plus their offset, the words over each at `-camera`, the
/// cursor and border at rest; the order and the offsets [`Presenter::plan`] places its parts at.
pub fn slide_placements(slide: &Slide, view: &View) -> Vec<(usize, i32, i32)> {
    let layers = &slide.parts;
    let find = |role: Role| layers.iter().position(|l| l.role == role);
    let mut out = Vec::new();
    for (part, (dx, dy)) in slide.layers.placed(view) {
        let role = match part {
            Part::Static => Role::Static,
            Part::Mover(i) => Role::Mover(i),
            Part::Over(i) => Role::Over(i),
        };
        if let Some(k) = find(role) {
            let p = &layers[k].pixels;
            out.push((k, p.x + dx, p.y + dy));
        }
    }
    if let Some(k) = find(Role::Frame) {
        let p = &layers[k].pixels;
        out.push((k, p.x, p.y));
    }
    out
}
