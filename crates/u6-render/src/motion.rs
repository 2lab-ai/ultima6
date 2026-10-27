//! Port-only (spec 20): smooth motion between cells, no original address.
//!
//! The engine shows a turn's moves in one picture of the map window. A platform with the switch on
//! is handed, with each such picture, its map scene ([`Scene`]: the view's cells as the window
//! render drew them, per cell the base word and the node words with their object slots), and a
//! [`Presenter`] slides from the previous scene to it over [`SLIDE_MS`] of the presentation clock
//! (engine time plus the pacer's carried fraction): the camera when the leader moved one cell, and
//! each object drawn in both scenes one cell apart. [`Presenter::plan`] gives what to draw at a
//! time as tile words at frame pixels; it holds no art, so the platform paints the plan with the
//! engine's blitter. The engine's state, RNG and timing never see any of it: a scene is a copy,
//! and with the switch off the frames are the engine's.
//!
//! - **Camera:** slides when the leader (the same slot, on the same level) moved by one cell
//!   (Chebyshev 1); any other change snaps every slide: a teleport, a ladder or a level change, a
//!   switch of leader, a scene that cannot slide ([`Scene::snap`]: a dissolve).
//! - **Movers:** a slot drawn in both scenes, one cell apart, of the same type slides; anything else
//!   appears, disappears or changes in place.
//! - **Order:** the static words (every node but the movers'), then the movers by their in-between
//!   y then x, then the occluders over them: in a cell the mover is drawn in, the words after it;
//!   in every other cell its draws cover from the slide's start to its end (its old cell, a
//!   diagonal's side cells, the cells a restart up to 1.5 cells away sweeps) the words from the
//!   first top tile, multi-tile part or light overlay on ([`Rank`]). At the end of a slide the plan
//!   draws the engine's picture exactly: the occluders are each a cell's last words, so a static
//!   pass and the occluders drawn again over the movers give each pixel the colour of the cell's
//!   draws in list order (opaque, transparent, OR and outline pixels are all the same drawn twice).
//! - **Layers** ([`Presenter::layers`], spec 20 Design 5): the same draws at rest, split by how
//!   they move (the static words and the occluders with the camera, each mover with itself), for a
//!   renderer that composes each part once a publication and only places them each display frame
//!   ([`Layers::placed`]); [`Presenter::plan`] is them placed.
//! - **New publications during a slide:** a foreign one ([`Presenter::foreign`]: a missile, a
//!   flash, a quake) ends every slide; a scene with the same positions lets the slides run on; a
//!   move restarts from where the object is drawn, or snaps when that is more than 1.5 cells from
//!   its new cell.
//! - **Previews** ([`Presenter::preview`], spec 20 Design 4): a scene the engine built on a copy
//!   of its state right after the leader's step, before its own picture of the turn. The party's
//!   slide starts there, and the preview is held, drawn at rest, until the engine publishes a
//!   picture (or [`PREVIEW_MS`] passed): a scene of the same places lets the party stand and
//!   slides the objects the turn moved since.

use std::collections::BTreeSet;
use std::rc::Rc;

/// The slide's length on the presentation clock, D: 18 PAL frames of engine time (spec 20 Design
/// 1). Under the 427 ms between two world-turn pictures outside a fight, never under the 18 frames
/// after a fight's move the start fight showed, 55–58 % of a quiet castle step (31–33 frames).
pub const SLIDE_MS: f64 = 360.0;
/// Pixels per map cell.
pub const CELL: i32 = 16;
/// The fewest display frames a slide is shown in; at a speed that would show it in fewer it snaps.
pub const MIN_FRAMES: f64 = 2.0;
/// A move during a slide restarts from where the object is drawn when that is at most this far
/// (Chebyshev, in pixels) from its new cell: 1.5 cells. So the camera is never drawn farther than
/// this from rest: a slide starts one cell off (a step of the leader, [`Presenter::publish`]'s
/// `follows`) and a restart at most this far (its `far` check), else it snaps.
pub const SNAP_PX: f64 = 1.5 * CELL as f64;
/// Scenes kept for the cells a slide shows and the newest scene lacks (the strip leaving the view).
const HISTORY: usize = 3;
/// How long a preview ([`Presenter::preview`]) is held, on the presentation clock, when no
/// picture of the engine's follows it: past a step's 540–860 ms to its picture.
pub const PREVIEW_MS: f64 = 2000.0;

/// Whether slides are shown at `rate` engine ms per wall ms (`None`: Max) on a display of
/// `interval_ms` wall ms per frame: a [`SLIDE_MS`] slide lasts `SLIDE_MS / rate` wall ms, at least
/// [`MIN_FRAMES`] display frames. A pointer button held paces at 1×, so its caller passes 1.
pub fn slides_shown(rate: Option<f64>, interval_ms: f64) -> bool {
    rate.is_some_and(|r| r.is_finite() && r > 0.0 && SLIDE_MS / r >= MIN_FRAMES * interval_ms)
}

/// Where a node sits in its cell's list, as the window render's list builder linked it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    /// An object's own tile in its own cell (the list's front part).
    Own,
    /// A top tile (flags2 0x10), linked behind the cell's last non-top tile.
    Top,
    /// The other part of a multi-tile object (or a doorway's side tile), at the list's end.
    Part,
    /// A light overlay, at the list's end.
    Overlay,
}

/// One node of a cell's draw list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// The tile word drawn (outline bits 11..12 included).
    pub word: u16,
    /// The object slot it is drawn for; `None` for a light overlay.
    pub slot: Option<u16>,
    pub rank: Rank,
}

/// One view cell: its base word, then its nodes in draw order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub base: u16,
    pub nodes: Vec<Node>,
}

/// An object drawn in a scene: its slot, its own world cell and its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub slot: u16,
    pub cell: (i32, i32),
    pub kind: u16,
}

/// The map window as one framed render drew it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    /// The leader's actor slot.
    pub leader: Option<u16>,
    /// The view centre's world cell and level.
    pub centre: (i32, i32, u8),
    /// The world cell of view cell (0, 0).
    pub origin: (i32, i32),
    pub cols: usize,
    pub rows: usize,
    /// The frame pixel of view cell (0, 0)'s top left.
    pub corner: (i32, i32),
    /// `rows` × `cols` cells, row-major.
    pub cells: Vec<Cell>,
    /// Every object with a node in `cells`, once, by slot (ascending: [`Scene::find`] searches it).
    pub placed: Vec<Placed>,
    /// No slide into this scene: it was presented dissolving, or with draws the scene does not
    /// hold (XOR plots).
    pub snap: bool,
}

impl AsRef<Scene> for Scene {
    fn as_ref(&self) -> &Scene {
        self
    }
}

impl Scene {
    /// The index of world cell `at` in `cells`, if the view holds it.
    pub fn index(&self, (x, y): (i32, i32)) -> Option<usize> {
        let (c, r) = (x - self.origin.0, y - self.origin.1);
        ((0..self.cols as i32).contains(&c) && (0..self.rows as i32).contains(&r))
            .then(|| r as usize * self.cols + c as usize)
    }

    /// The world cell of `cells[i]`.
    pub fn cell_at(&self, i: usize) -> (i32, i32) {
        (
            self.origin.0 + (i % self.cols) as i32,
            self.origin.1 + (i / self.cols) as i32,
        )
    }

    /// Where `slot` is drawn, if it is.
    pub fn find(&self, slot: u16) -> Option<&Placed> {
        let i = self.placed.binary_search_by_key(&slot, |p| p.slot).ok()?;
        self.placed.get(i)
    }

    /// The frame pixel of world cell `at`'s top left, the camera at rest.
    fn pixel(&self, (x, y): (i32, i32)) -> (i32, i32) {
        (
            self.corner.0 + CELL * (x - self.origin.0),
            self.corner.1 + CELL * (y - self.origin.1),
        )
    }
}

/// A cell's world pixel (its top left).
fn px((x, y): (i32, i32)) -> (f64, f64) {
    (f64::from(x * CELL), f64::from(y * CELL))
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// Farther apart than [`SNAP_PX`].
fn far(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs().max((a.1 - b.1).abs()) > SNAP_PX
}

/// One slide: linear from `from` to `to` (world pixels) over [`SLIDE_MS`] from `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    from: (f64, f64),
    to: (f64, f64),
    start: f64,
}

impl Track {
    fn at(&self, t: f64) -> (f64, f64) {
        let p = ((t - self.start) / SLIDE_MS).clamp(0.0, 1.0);
        (
            self.from.0 + (self.to.0 - self.from.0) * p,
            self.from.1 + (self.to.1 - self.from.1) * p,
        )
    }

    fn done(&self, t: f64) -> bool {
        t >= self.start + SLIDE_MS
    }

    /// Where it is drawn at `t`, in whole pixels from `to`.
    fn offset(&self, t: f64) -> (i32, i32) {
        let (x, y) = self.at(t);
        (
            (x - self.to.0).round() as i32,
            (y - self.to.1).round() as i32,
        )
    }
}

/// An object sliding: its slot and where it is drawn, in pixels from its cell in the newest scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mover {
    pub slot: u16,
    pub offset: (i32, i32),
}

/// The slides at one time: the camera's offset (pixels the view is drawn from its place in the
/// newest scene, the other way round: the world shows at `-camera`) and the movers'.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub camera: (i32, i32),
    pub movers: Vec<Mover>,
}

/// What one op of a plan draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Draw {
    /// The base of cell `cell` of scene `scene` ([`Presenter::history`]'s index): its base word,
    /// or what the platform draws for it in place (the remembered sight).
    Base { scene: usize, cell: usize },
    /// A tile word.
    Word(u16),
}

/// One draw of a plan, its top left at frame pixel (x, y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub draw: Draw,
    pub x: i32,
    pub y: i32,
}

/// The scenes published and the slides between them (the module docs). `T` is what the platform
/// keeps with a scene (the engine's: what it draws in place of a base).
pub struct Presenter<T> {
    /// Newest first, at most [`HISTORY`].
    history: Vec<Rc<T>>,
    camera: Option<Track>,
    movers: Vec<(u16, Track)>,
    /// The newest scene is a preview the engine has not shown yet, held until this time.
    held: Option<f64>,
    /// [`Presenter::generation`].
    generation: u64,
}

impl<T: AsRef<Scene>> Default for Presenter<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AsRef<Scene>> Presenter<T> {
    /// No scene, no slide.
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            camera: None,
            movers: Vec::new(),
            held: None,
            generation: 0,
        }
    }

    /// Forgets every scene and slide (a new game).
    pub fn clear(&mut self) {
        self.history.clear();
        self.end_all();
    }

    /// The scenes kept, newest first ([`Draw::Base`] names them by index).
    pub fn history(&self) -> &[Rc<T>] {
        &self.history
    }

    fn end_all(&mut self) {
        self.camera = None;
        self.movers.clear();
        self.held = None;
        self.generation += 1;
    }

    /// Drops the slides ended at `t` (each then stands where it slid to).
    pub fn end(&mut self, t: f64) {
        if self.camera.is_some_and(|c| c.done(t)) {
            self.camera = None;
        }
        self.movers.retain(|(_, m)| !m.done(t));
        if self.held.is_some_and(|until| t >= until) {
            self.held = None;
        }
    }

    /// A scene the engine built ahead of its picture (the early picture of the leader's step):
    /// slid to as [`Presenter::publish`] does, then held at rest until the engine publishes a
    /// picture, at most [`PREVIEW_MS`].
    pub fn preview(&mut self, shot: Rc<T>, time: f64, slides: bool) {
        if slides {
            self.publish(shot, time, true);
            self.held = Some(time + PREVIEW_MS);
        }
    }

    /// A publication with no scene of its own (a missile, a flash, a quake): every slide ends, so
    /// the platform shows the engine's frame.
    pub fn foreign(&mut self) {
        self.end_all();
    }

    /// The engine published `shot`'s scene at engine time `time`; `slides`: whether slides are
    /// shown now ([`slides_shown`]).
    pub fn publish(&mut self, shot: Rc<T>, time: f64, slides: bool) {
        self.end(time);
        self.held = None;
        let before = (self.camera, self.movers.clone());
        let new = (*shot).as_ref();
        let follows = |prev: &Scene| {
            prev.leader == new.leader
                && prev.centre.2 == new.centre.2
                && (prev.cols, prev.rows, prev.corner) == (new.cols, new.rows, new.corner)
                && chebyshev((prev.centre.0, prev.centre.1), (new.centre.0, new.centre.1)) <= 1
        };
        let prev = match self.history.first() {
            Some(p) if slides && !new.snap && follows((**p).as_ref()) => Rc::clone(p),
            _ => return self.restart(shot),
        };
        let prev = (*prev).as_ref();
        let centre = |s: &Scene| (s.centre.0, s.centre.1);
        if centre(prev) != centre(new) {
            let from = self.camera.map_or(px(centre(prev)), |c| c.at(time));
            let to = px(centre(new));
            if far(from, to) {
                return self.restart(shot);
            }
            self.camera = Some(Track {
                from,
                to,
                start: time,
            });
        }
        let mut movers = Vec::new();
        for p in &new.placed {
            let Some(q) = prev.find(p.slot).filter(|q| q.kind == p.kind) else {
                continue;
            };
            let to = px(p.cell);
            let running = (self.movers.iter())
                .find(|(s, _)| *s == p.slot)
                .map(|&(_, m)| m);
            if q.cell == p.cell {
                movers.extend(running.filter(|m| m.to == to).map(|m| (p.slot, m)));
                continue;
            }
            if chebyshev(q.cell, p.cell) != 1 {
                continue;
            }
            let from = running.map_or(px(q.cell), |m| m.at(time));
            if !far(from, to) {
                let start = time;
                movers.push((p.slot, Track { from, to, start }));
            }
        }
        self.movers = movers;
        self.history.insert(0, shot);
        self.history.truncate(HISTORY);
        if (self.camera, &self.movers) != (before.0, &before.1) {
            self.generation += 1;
        }
    }

    /// `shot` alone, nothing sliding: what the engine shows.
    fn restart(&mut self, shot: Rc<T>) {
        self.history.clear();
        self.history.push(shot);
        self.end_all();
    }

    /// Whether a slide runs at `t`, or a preview is held (the platform draws the plan then, else
    /// the engine's frame).
    pub fn active(&self, t: f64) -> bool {
        self.camera.is_some_and(|c| !c.done(t))
            || self.movers.iter().any(|(_, m)| !m.done(t))
            || self.held.is_some_and(|until| t < until)
    }

    /// The slides as drawn at `t` (at their end once `t` passed it, until [`Presenter::end`]),
    /// and a preview held at rest; `None` when neither is.
    pub fn view(&self, t: f64) -> Option<View> {
        let idle = self.camera.is_none() && self.movers.is_empty();
        if idle && self.held.is_none_or(|until| t >= until) || self.history.is_empty() {
            return None;
        }
        Some(View {
            camera: self.camera.map_or((0, 0), |c| c.offset(t)),
            movers: (self.movers.iter())
                .map(|&(slot, m)| Mover {
                    slot,
                    offset: m.offset(t),
                })
                .collect(),
        })
    }

    /// What to draw for `view`, in order (the module docs): [`Presenter::layers`] placed for it,
    /// the static words with the camera, the movers by their in-between y then x, each with its
    /// offset, then the words that cover each mover with the camera.
    pub fn plan(&self, view: &View) -> Vec<Op> {
        let layers = self.layers();
        let mut ops: Vec<Op> = Vec::new();
        for (layer, (dx, dy)) in layers.placed(view) {
            let from = match layer {
                Part::Static => &layers.statics,
                Part::Mover(i) => &layers.movers[i].ops,
                Part::Over(i) => &layers.movers[i].over,
            };
            ops.extend(from.iter().map(|op| Op {
                x: op.x + dx,
                y: op.y + dy,
                ..*op
            }));
        }
        ops
    }

    /// Bumped whenever what slides changes: a camera slide starts or restarts, a mover joins,
    /// restarts or leaves the slides, the presenter starts again ([`Presenter::foreign`],
    /// [`Presenter::clear`], a snap). A publication that only lets the slides run on (the same
    /// places: an animation, the turn's picture after its early one) keeps it, and the layers
    /// ([`Presenter::layers`]) of a generation stay those it was made with: what changed in place
    /// shows at the slide's end, in the engine's frame.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Spec 20 Design 5: what the slides draw, at rest (the camera and every mover at its place in
    /// the newest scene), split by how it moves: the static words (the cells of the newest scene
    /// and a cell round it, each from the newest scene that holds it; one held only by an older
    /// scene without the objects the newest one draws), which move with the camera; per mover its
    /// own words, which move with it, and the words that cover it, which move with the camera: in a
    /// cell it is drawn in the words after the first mover's, in the cells it passes besides (every
    /// cell its draws cover from the slide's start to its end: its old cell, a diagonal's side cells,
    /// a restart's cells) the words from the first top tile, multi-tile part or light overlay on.
    /// Each is a cell's last words, so drawn again over the static words they leave the cell as it
    /// was.
    pub fn layers(&self) -> Layers {
        let Some(newest) = self.history.first() else {
            return Layers::default();
        };
        let n = (**newest).as_ref();
        let moving = |slot: Option<u16>| slot.is_some_and(|s| self.movers.iter().any(|m| m.0 == s));
        let shown_here = |k: usize, slot: Option<u16>| {
            moving(slot) || (k > 0 && slot.is_some_and(|s| n.find(s).is_some()))
        };
        let source = |at: (i32, i32)| {
            self.history
                .iter()
                .enumerate()
                .find_map(|(k, s)| (**s).as_ref().index(at).map(|i| (k, i)))
        };
        let scene = |k: usize| (*self.history[k]).as_ref();
        let word = |w: u16, (x, y): (i32, i32)| Op {
            draw: Draw::Word(w),
            x,
            y,
        };
        let mut statics = Vec::new();
        let (xs, ys) = (
            n.origin.0 - 1..=n.origin.0 + n.cols as i32,
            n.origin.1 - 1..=n.origin.1 + n.rows as i32,
        );
        for y in ys {
            for x in xs.clone() {
                let Some((k, i)) = source((x, y)) else {
                    continue;
                };
                let at = n.pixel((x, y));
                statics.push(Op {
                    draw: Draw::Base { scene: k, cell: i },
                    x: at.0,
                    y: at.1,
                });
                for node in scene(k).cells[i].nodes.iter() {
                    if !shown_here(k, node.slot) {
                        statics.push(word(node.word, at));
                    }
                }
            }
        }
        let mut movers = Vec::new();
        for &(slot, track) in &self.movers {
            // An object's nodes lie in its own cell and the cells west and north of it (its other
            // parts), or east (a doorway's side tile).
            let Some(home) = n.find(slot).map(|p| p.cell) else {
                continue;
            };
            // The pixels the mover is drawn from its place, start to end: each cell it covers on
            // the way, a restart's start up to 1.5 cells off and a diagonal's side cells included;
            // drawn at whole pixels (`Track::offset` rounds), so from the way's floor to its ceil.
            let way = (track.from.0 - track.to.0, track.from.1 - track.to.1);
            let swept = |c: i32, d: f64| {
                let (lo, hi) = (
                    (f64::from(c * CELL) + d.min(0.0)).floor(),
                    (f64::from(c * CELL) + d.max(0.0)).ceil(),
                );
                let first = (lo / f64::from(CELL)).floor() as i32;
                let last = ((hi + f64::from(CELL) - 1.0) / f64::from(CELL)).floor() as i32;
                first..=last
            };
            let (mut ops, mut cells) = (Vec::new(), BTreeSet::new());
            for (cx, cy) in
                (home.1 - 1..=home.1).flat_map(|y| (home.0 - 1..=home.0 + 1).map(move |x| (x, y)))
            {
                let Some(i) = n.index((cx, cy)) else {
                    continue;
                };
                let mine = n.cells[i].nodes.iter().filter(|nd| nd.slot == Some(slot));
                let mut any = false;
                for node in mine {
                    ops.push(word(node.word, n.pixel((cx, cy))));
                    any = true;
                }
                if any {
                    for y in swept(cy, way.1) {
                        for x in swept(cx, way.0) {
                            cells.insert((y, x));
                        }
                    }
                }
            }
            let mut over = Vec::new();
            for (y, x) in cells {
                let Some((k, i)) = source((x, y)) else {
                    continue;
                };
                let at = n.pixel((x, y));
                let nodes = &scene(k).cells[i].nodes;
                let from = match nodes.iter().position(|nd| moving(nd.slot)) {
                    Some(f) => Some(f + 1),
                    None => nodes.iter().position(|nd| nd.rank != Rank::Own),
                };
                for node in &nodes[from.unwrap_or(nodes.len())..] {
                    if !shown_here(k, node.slot) {
                        over.push(word(node.word, at));
                    }
                }
            }
            movers.push(MoverLayer {
                slot,
                home: n.pixel(home),
                ops,
                over,
            });
        }
        Layers { statics, movers }
    }
}

/// Spec 20 Design 5: the slides' draws by how they move ([`Presenter::layers`]), every op at rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layers {
    /// The static words: drawn at the camera's offset.
    pub statics: Vec<Op>,
    /// The movers, in the order of [`Presenter`]'s slides.
    pub movers: Vec<MoverLayer>,
}

/// One mover's draws ([`Layers`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoverLayer {
    pub slot: u16,
    /// The frame pixel of its own cell at rest (the draw order sorts by it, slid).
    pub home: (i32, i32),
    /// Its own words: drawn at the camera's offset plus its own.
    pub ops: Vec<Op>,
    /// The words that cover it: drawn at the camera's offset.
    pub over: Vec<Op>,
}

/// Which part of [`Layers`] a placement draws ([`Layers::placed`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Static,
    /// The own words of mover `i` (an index of [`Layers::movers`]).
    Mover(usize),
    /// The words that cover mover `i`.
    Over(usize),
}

impl Layers {
    /// The parts in their draw order for `view`, each with its offset from rest in pixels: the
    /// static words at `-camera`; the movers by their in-between y then x (their place in the
    /// newest scene slid by their offset), at `-camera` plus their offset (a mover `view` does
    /// not slide, its slide ended, at rest); then the words that cover each, in the same order, at
    /// `-camera`.
    pub fn placed(&self, view: &View) -> Vec<(Part, (i32, i32))> {
        let cam = (-view.camera.0, -view.camera.1);
        let offset = |slot: u16| {
            view.movers
                .iter()
                .find(|m| m.slot == slot)
                .map_or((0, 0), |m| m.offset)
        };
        // Per mover: where it is drawn (y, x), its index, its offset from rest.
        let mut order: Vec<_> = (self.movers.iter().enumerate())
            .map(|(i, m)| {
                let (dx, dy) = offset(m.slot);
                let at = (m.home.1 + cam.1 + dy, m.home.0 + cam.0 + dx);
                (at, i, (cam.0 + dx, cam.1 + dy))
            })
            .collect();
        order.sort_by_key(|&(at, i, _)| (at, i));
        let mut out = vec![(Part::Static, cam)];
        out.extend(order.iter().map(|&(_, i, d)| (Part::Mover(i), d)));
        out.extend(order.iter().map(|&(_, i, _)| (Part::Over(i), cam)));
        out
    }
}

/// Spec 20 Design 5: one layer of a slide as the engine rasterized it, for the HD compositor: a
/// `width` × `height` pixel rect whose top left is at frame pixel (`x`, `y`) at rest, its colour
/// indices and sources, and which pixels its draws wrote (the others are clear).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerPixels {
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub src: Vec<crate::Src>,
    pub drawn: Vec<bool>,
}

impl LayerPixels {
    /// A clear layer of `width` × `height` pixels from frame pixel (`x`, `y`).
    pub fn clear(x: i32, y: i32, width: usize, height: usize) -> Self {
        let n = width * height;
        Self {
            x,
            y,
            width,
            height,
            pixels: vec![0; n],
            src: vec![crate::Src::CLASSIC; n],
            drawn: vec![false; n],
        }
    }

    /// The index of frame pixel (`x`, `y`) in the layer at rest, if it holds it.
    pub fn index(&self, (x, y): (i32, i32)) -> Option<usize> {
        let (c, r) = (x - self.x, y - self.y);
        ((0..self.width as i32).contains(&c) && (0..self.height as i32).contains(&r))
            .then(|| r as usize * self.width + c as usize)
    }

    /// The 16 × 16 blocks (from the top left) where this layer differs from `old` with `old`'s
    /// pixels moved by `shift` (its pixel p shows at p + `shift`; a block whose pixels it does not
    /// cover differs), merged along each row of blocks, then a run under one of the same columns
    /// into it, into rects (x, y, width, height) in pixels from the top left, by their top row;
    /// `None` when the two lie at other places or have other sizes.
    pub fn changed_since(&self, old: &LayerPixels, shift: (i32, i32)) -> Option<Vec<[usize; 4]>> {
        if (self.x, self.y, self.width, self.height) != (old.x, old.y, old.width, old.height) {
            return None;
        }
        let b = CELL as usize;
        let (cols, rows) = (self.width.div_ceil(b), self.height.div_ceil(b));
        // Row by row of the block: the old row's pixels at the place they show, compared as runs.
        let same = |bx: usize, by: usize| {
            let (x0, x1) = (bx * b, ((bx + 1) * b).min(self.width));
            let ox = x0 as i32 - shift.0;
            if ox < 0 || ox as usize + (x1 - x0) > old.width {
                return false;
            }
            (by * b..((by + 1) * b).min(self.height)).all(|y| {
                let oy = y as i32 - shift.1;
                if !(0..old.height as i32).contains(&oy) {
                    return false;
                }
                let (i, o) = (y * self.width + x0, oy as usize * old.width + ox as usize);
                let n = x1 - x0;
                let (drawn, was) = (&self.drawn[i..i + n], &old.drawn[o..o + n]);
                if drawn != was {
                    return false;
                }
                // A row clear in both (most of the cursor and border layer): nothing to compare.
                if !drawn.contains(&true) {
                    return true;
                }
                #[cfg(test)]
                ROWS_COMPARED.with(|n| n.set(n.get() + 1));
                let (pixels, src) = (&self.pixels[i..i + n], &self.src[i..i + n]);
                let (old_pixels, old_src) = (&old.pixels[o..o + n], &old.src[o..o + n]);
                // Equal as stored, or equal where drawn (a clear pixel's colour is no one's).
                pixels == old_pixels && src == old_src
                    || (drawn.iter().zip(pixels.iter().zip(src)))
                        .zip(old_pixels.iter().zip(old_src))
                        .all(|((&d, (p, s)), (q, t))| !d || p == q && s == t)
            })
        };
        let mut rects = Vec::new();
        for by in 0..rows {
            let mut run: Option<usize> = None;
            for bx in 0..=cols {
                let dirty = bx < cols && !same(bx, by);
                match (dirty, run) {
                    (true, None) => run = Some(bx),
                    (false, Some(from)) => {
                        let (x, y) = (from * b, by * b);
                        let w = (bx * b).min(self.width) - x;
                        let h = ((by + 1) * b).min(self.height) - y;
                        // Under a rect of the row above with the same columns: one rect.
                        match rects
                            .iter_mut()
                            .rev()
                            .find(|r: &&mut [usize; 4]| r[0] == x && r[2] == w && r[1] + r[3] == y)
                        {
                            Some(r) => r[3] += h,
                            None => rects.push([x, y, w, h]),
                        }
                        run = None;
                    }
                    _ => {}
                }
            }
        }
        Some(rects)
    }

    /// The pixels of rect (x, y, width, height) from the top left, as a layer of their own at their
    /// frame pixels.
    pub fn rect(&self, [x, y, w, h]: [usize; 4]) -> LayerPixels {
        let mut out = LayerPixels::clear(self.x + x as i32, self.y + y as i32, w, h);
        for r in 0..h {
            let from = (y + r) * self.width + x;
            let to = r * w;
            out.pixels[to..to + w].copy_from_slice(&self.pixels[from..from + w]);
            out.src[to..to + w].copy_from_slice(&self.src[from..from + w]);
            out.drawn[to..to + w].copy_from_slice(&self.drawn[from..from + w]);
        }
        out
    }

    /// The bounding rect (x, y, width, height) of 16-pixel draws at `ops`' places, if any.
    pub fn bounds(ops: &[Op]) -> Option<(i32, i32, usize, usize)> {
        let x0 = ops.iter().map(|o| o.x).min()?;
        let y0 = ops.iter().map(|o| o.y).min()?;
        let x1 = ops.iter().map(|o| o.x + CELL).max()?;
        let y1 = ops.iter().map(|o| o.y + CELL).max()?;
        Some((x0, y0, (x1 - x0) as usize, (y1 - y0) as usize))
    }
}

#[cfg(test)]
thread_local! {
    /// The block rows whose pixels [`LayerPixels::changed_since`] compared (the tests read it).
    static ROWS_COMPARED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;
