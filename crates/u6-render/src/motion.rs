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
//!   in the cells it overlaps besides (its old cell, a diagonal's side cells) the top tiles, the
//!   multi-tile parts and the light overlays ([`Rank`]). At the end of a slide the plan draws the
//!   engine's picture exactly: a static pass and the occluders drawn again over the movers give
//!   each pixel the colour of the cell's draws in list order (opaque, transparent, OR and outline
//!   pixels are all the same drawn twice).
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
/// (Chebyshev, in pixels) from its new cell: 1.5 cells.
const SNAP_PX: f64 = 1.5 * CELL as f64;
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

    /// What to draw for `view`, in order (the module docs): the cells of the newest scene and a
    /// cell round it (each from the newest scene that holds it; one held only by an older scene
    /// without the objects the newest one draws), the movers, the occluders.
    pub fn plan(&self, view: &View) -> Vec<Op> {
        let Some(newest) = self.history.first() else {
            return Vec::new();
        };
        let n = (**newest).as_ref();
        let moving =
            |slot: Option<u16>| slot.is_some_and(|s| view.movers.iter().any(|m| m.slot == s));
        let shown_here = |k: usize, slot: Option<u16>| {
            moving(slot) || (k > 0 && slot.is_some_and(|s| n.find(s).is_some()))
        };
        let screen = |at: (i32, i32)| {
            let (x, y) = n.pixel(at);
            (x - view.camera.0, y - view.camera.1)
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
        let mut ops = Vec::new();
        let (xs, ys) = (
            n.origin.0 - 1..=n.origin.0 + n.cols as i32,
            n.origin.1 - 1..=n.origin.1 + n.rows as i32,
        );
        for y in ys {
            for x in xs.clone() {
                let Some((k, i)) = source((x, y)) else {
                    continue;
                };
                let at = screen((x, y));
                ops.push(Op {
                    draw: Draw::Base { scene: k, cell: i },
                    x: at.0,
                    y: at.1,
                });
                for node in scene(k).cells[i].nodes.iter() {
                    if !shown_here(k, node.slot) {
                        ops.push(word(node.word, at));
                    }
                }
            }
        }
        let mut order: Vec<((i32, i32), &Mover)> = (view.movers.iter())
            .filter_map(|m| {
                let (x, y) = screen(n.find(m.slot)?.cell);
                Some(((y + m.offset.1, x + m.offset.0), m))
            })
            .collect();
        order.sort_by_key(|&(at, _)| at);
        let mut over = BTreeSet::new();
        for (_, m) in order {
            // An object's nodes lie in its own cell and the cells west and north of it (its other
            // parts), or east (a doorway's side tile).
            let Some(home) = n.find(m.slot).map(|p| p.cell) else {
                continue;
            };
            for (cx, cy) in
                (home.1 - 1..=home.1).flat_map(|y| (home.0 - 1..=home.0 + 1).map(move |x| (x, y)))
            {
                let Some(i) = n.index((cx, cy)) else {
                    continue;
                };
                let mine = n.cells[i].nodes.iter().filter(|nd| nd.slot == Some(m.slot));
                let mut any = false;
                for node in mine {
                    let (x, y) = screen((cx, cy));
                    ops.push(word(node.word, (x + m.offset.0, y + m.offset.1)));
                    any = true;
                }
                if any {
                    let (sx, sy) = (m.offset.0.signum(), m.offset.1.signum());
                    for (dx, dy) in [(0, 0), (sx, 0), (0, sy), (sx, sy)] {
                        over.insert((cy + dy, cx + dx));
                    }
                }
            }
        }
        for (y, x) in over {
            let Some((k, i)) = source((x, y)) else {
                continue;
            };
            let at = screen((x, y));
            let nodes = &scene(k).cells[i].nodes;
            let first = nodes.iter().position(|nd| moving(nd.slot));
            for (j, node) in nodes.iter().enumerate() {
                let after = first.map_or(node.rank != Rank::Own, |f| j > f);
                if after && !shown_here(k, node.slot) {
                    ops.push(word(node.word, at));
                }
            }
        }
        ops
    }
}

#[cfg(test)]
mod tests;
