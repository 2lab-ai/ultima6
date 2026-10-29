//! Port-only (spec 20): smooth motion between cells, no original address.
//!
//! The engine shows a turn's moves in one picture of the map window. A platform with the switch on
//! is handed, with each such picture, its map scene ([`Scene`]: the view's cells as the window
//! render drew them, per cell the base word and the node words with their object slots), and a
//! [`Presenter`] slides from the previous scene to it over its track's duration on the presentation clock
//! (engine time plus the pacer's carried fraction): the camera when the leader moved one cell, and
//! each object drawn in both scenes one cell apart. [`Presenter::plan`] gives what to draw at a
//! time as tile words at frame pixels; it holds no art, so the platform paints the plan with the
//! engine's blitter. The engine's state, RNG and timing never see any of it: a scene is a copy,
//! and with the switch off the frames are the engine's. Taps use [`SLIDE_MS`] (scaled by O3);
//! party tracks use each mover's measured interval while a manual direction or automatic route
//! continues. Route corners are retained as bounded waypoints; replacing the route resets cadence.
//!
//! - **Camera:** slides when the leader (the same slot, on the same level) moved by one cell
//!   (Chebyshev 1); any other change snaps every slide: a teleport, a ladder or a level change, a
//!   switch of leader, a scene that cannot slide ([`Scene::snap`]: a dissolve).
//! - **Movers:** a slot drawn in both scenes, one cell apart, of the same type slides; anything else
//!   appears, disappears or changes in place.
//! - **Order:** the static words (every node but the movers'), then the movers by their in-between
//!   y then x, then the occluders over them: in a cell the mover is drawn in, the words after it;
//!   in every other cell its draws cover from the slide's start to its end (its old cell, a
//!   diagonal's side cells, the cells the actual restart sweeps) the words from the
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
//!   one-cell move restarts from where the object is drawn, including accumulated visual lag.
//! - **Previews** ([`Presenter::preview`], spec 20 Design 4): a scene the engine built on a copy
//!   of its state right after the leader's step, before its own picture of the turn. The party's
//!   slide starts there, and the preview is held, drawn at rest, until the engine publishes a
//!   picture or its command prompt is back ([`Presenter::prompt`]), whichever comes first: a
//!   scene of the same places lets the party stand and slides the objects the turn moved since.
//!   No timer ends it: the hold keeps nothing past the bounded scenes kept ([`MAX_HISTORY`]).

pub mod attacks;
pub mod footsteps;

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// The slide's length on the presentation clock, D: 18 PAL frames of engine time (spec 20 Design
/// 1). Under the 427 ms between two world-turn pictures outside a fight, never under the 18 frames
/// after a fight's move the start fight showed, 55–58 % of a quiet castle step (31–33 frames).
pub const SLIDE_MS: f64 = 360.0;
/// Pixels per map cell.
pub const CELL: i32 = 16;
/// The fewest display frames a slide is shown in; at a speed that would show it in fewer it snaps.
pub const MIN_FRAMES: f64 = 2.0;
/// Typical scene count for a one-cell slide. Only sources needed by the actual sweep are kept.
pub const HISTORY: usize = 3;
/// Resource ceilings, independent of distance: a pathological burst may reset presentation
/// rather than retain unbounded snapshots or allocate a huge static raster.
pub const MAX_HISTORY: usize = 16;
/// Pending corners per automatic-route track, excluding its final target. Fixed storage;
/// overflow uses the same explicit, counted resource reset as the scene/raster ceilings.
pub const MAX_WAYPOINTS: usize = 16;
pub const MAX_STATIC_PIXELS: usize = 1024 * 1024;
/// Per-view area headroom: four cells round each edge, counted as area rather than a
/// displacement threshold. The absolute pixel ceiling still handles malformed scenes.
pub const SWEEP_MARGIN_CELLS: usize = 4;

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

/// The previous actual one-cell move of one mover, not the previous publication.
#[derive(Clone, Copy, Debug)]
struct Stride {
    at: f64,
    /// The raw interval ending here, never a previously buffered maximum.
    interval: f64,
}

impl Stride {
    fn next(previous: Option<Self>, time: f64) -> Self {
        Self {
            at: time,
            interval: previous
                .filter(|p| time.is_finite() && p.at.is_finite() && time > p.at)
                .map_or(0.0, |p| time - p.at),
        }
    }
}

fn leg_duration(previous: Option<Stride>, time: f64, base: f64) -> f64 {
    let interval = Stride::next(previous, time).interval;
    if interval == 0.0 {
        base
    } else {
        interval.max(previous.map_or(0.0, |p| p.interval))
    }
}

/// Authoritative movement observations precede possibly delayed scene publications.
#[derive(Clone, Debug)]
struct Observation {
    cell: (i32, i32),
    stride: Option<Stride>,
    periods: Vec<((i32, i32), f64, f64)>,
}

/// One slide in world pixels, over its duration from `start`: straight for manual motion,
/// or through the remaining corners of an automatic route at constant path speed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    from: (f64, f64),
    to: (f64, f64),
    start: f64,
    duration: f64,
    via: [(f64, f64); MAX_WAYPOINTS],
    via_len: usize,
}

impl Track {
    fn straight(from: (f64, f64), to: (f64, f64), start: f64, duration: f64) -> Self {
        Self {
            from,
            to,
            start,
            duration,
            via: [(0.0, 0.0); MAX_WAYPOINTS],
            via_len: 0,
        }
    }

    fn points(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.via[..self.via_len].iter().copied().chain([self.to])
    }

    fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
        (a.0 - b.0).abs().max((a.1 - b.1).abs())
    }

    fn length(&self) -> f64 {
        let mut from = self.from;
        self.points()
            .map(|to| {
                let d = Self::distance(from, to);
                from = to;
                d
            })
            .sum()
    }

    fn travelled(&self, t: f64) -> f64 {
        if self.duration == 0.0 {
            self.length()
        } else {
            self.length() * ((t - self.start) / self.duration).clamp(0.0, 1.0)
        }
    }

    /// Remaining corners and the previous target, in order; no allocation or unbounded queue.
    fn retarget(&self, to: (f64, f64), time: f64, duration: f64, preserve: bool) -> Option<Self> {
        let mut next = Self::straight(self.at(time), to, time, duration);
        if preserve {
            let mut from = self.from;
            let mut left = self.travelled(time);
            let mut previous = next.from;
            for (index, point) in self.points().enumerate() {
                let distance = Self::distance(from, point);
                from = point;
                if left >= distance {
                    left -= distance;
                    continue;
                }
                left = 0.0;
                if point == previous || (point == to && index == self.via_len) {
                    continue;
                }
                if next.via_len == MAX_WAYPOINTS {
                    return None;
                }
                next.via[next.via_len] = point;
                next.via_len += 1;
                previous = point;
            }
        }
        Some(next)
    }

    /// Bounds of every point relative to the final target, for ground and occlusion coverage.
    fn bounds(&self) -> ((f64, f64), (f64, f64)) {
        let (mut low, mut high) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
        for p in std::iter::once(self.from).chain(self.points()) {
            let d = (p.0 - self.to.0, p.1 - self.to.1);
            low.0 = low.0.min(d.0);
            low.1 = low.1.min(d.1);
            high.0 = high.0.max(d.0);
            high.1 = high.1.max(d.1);
        }
        (low, high)
    }

    fn at(&self, t: f64) -> (f64, f64) {
        if self.duration == 0.0 {
            return self.to;
        }
        if self.via_len == 0 {
            let p = ((t - self.start) / self.duration).clamp(0.0, 1.0);
            return (
                self.from.0 + (self.to.0 - self.from.0) * p,
                self.from.1 + (self.to.1 - self.from.1) * p,
            );
        }
        let mut left = self.travelled(t);
        let mut from = self.from;
        for to in self.points() {
            let distance = Self::distance(from, to);
            if distance > 0.0 && left < distance {
                let p = left / distance;
                return (from.0 + (to.0 - from.0) * p, from.1 + (to.1 - from.1) * p);
            }
            left -= distance;
            from = to;
        }
        self.to
    }

    fn done(&self, t: f64) -> bool {
        t >= self.start + self.duration
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

/// Manual direction changes restart cadence; turns within one automatic route do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalkSession {
    Direction(i32, i32),
    Route(u64),
}

/// The scenes published and the slides between them (the module docs). `T` is what the platform
/// keeps with a scene (the engine's: what it draws in place of a base).
pub struct Presenter<T> {
    /// Newest first; only scenes supplying cells in the current sweep.
    history: Vec<Rc<T>>,
    camera: Option<Track>,
    duration: f64,
    minimum_duration: f64,
    walking: Vec<u16>,
    walk_session: Option<WalkSession>,
    camera_stride: Option<Stride>,
    strides: BTreeMap<u16, Stride>,
    observations: BTreeMap<i32, Observation>,
    movers: Vec<(u16, Track)>,
    /// The newest scene is a preview the engine has not shown yet, held until it publishes a
    /// picture or its prompt is back ([`Presenter::preview`]).
    held: bool,
    /// [`Presenter::generation`].
    generation: u64,
    capacity_resets: u64,
    snaps: u64,
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
            duration: SLIDE_MS,
            minimum_duration: 0.0,
            walking: Vec::new(),
            walk_session: None,
            camera_stride: None,
            strides: BTreeMap::new(),
            observations: BTreeMap::new(),
            movers: Vec::new(),
            held: false,
            generation: 0,
            capacity_resets: 0,
            snaps: 0,
        }
    }

    /// Duration of newly published moves, in engine ms (spec 19 O3).
    /// A running track keeps its own duration; instant ends every track at once.
    pub fn set_duration(&mut self, ms: f64) {
        let ms = if ms.is_finite() && ms >= 0.0 {
            ms
        } else {
            SLIDE_MS
        };
        if self.duration != ms {
            self.duration = ms;
            if ms == 0.0 {
                self.snap();
            }
        }
    }

    /// Party slots eligible for cadence matching while a physical direction is held.
    /// Empty on release/modifiers. A release settles the remaining distance at base speed;
    /// it changes no target or game input. A follower's changing formation direction does not
    /// reset cadence; the physical held direction does. Independent NPC tracks are untouched.
    pub fn set_walking(&mut self, party: &[u16], direction: (i32, i32), time: f64) {
        self.set_walk_session(
            party,
            WalkSession::Direction(direction.0, direction.1),
            time,
        );
    }

    /// Cadence of one accepted automatic route. Its identity survives corners and replanning;
    /// actual cells still determine each track's direction. Replacing the route or switching
    /// to manual input settles and resets its cadence just as releasing the walking party does.
    pub fn set_route_walking(&mut self, party: &[u16], session: u64, time: f64) {
        self.set_walk_session(party, WalkSession::Route(session), time);
    }

    fn set_walk_session(&mut self, party: &[u16], session: WalkSession, time: f64) {
        let session = (!party.is_empty()).then_some(session);
        if self.walking == party && self.walk_session == session {
            return;
        }
        let previous_session = std::mem::replace(&mut self.walk_session, session);
        let previous_party = std::mem::replace(&mut self.walking, party.to_vec());
        self.camera_stride = None;
        self.strides.clear();
        self.observations.clear();
        if !previous_party.is_empty() {
            let base = self.duration;
            let settle = |m: &mut Track| {
                let preserve =
                    matches!(previous_session, Some(WalkSession::Route(_))) && session.is_none();
                let remaining = (m.start + m.duration - time).max(0.0);
                let mut next = m
                    .retarget(m.to, time, remaining, preserve)
                    .expect("settling cannot add a waypoint");
                next.duration = remaining.min(base * next.length() / f64::from(CELL));
                *m = next;
            };
            if let Some(c) = &mut self.camera {
                settle(c);
            }
            for (slot, m) in &mut self.movers {
                if previous_party.contains(slot) {
                    settle(m);
                }
            }
            self.generation += 1;
        }
    }

    /// Observe an actual game cell after a polling step, before taking its pictures. Camera
    /// uses slot -1. Publication time is not a cadence sample: a withheld preview may be
    /// followed only 40ms later by the next step's allowed preview.
    pub fn observe(&mut self, slot: i32, cell: (i32, i32), time: f64) {
        if !time.is_finite()
            || !self.walking()
            || (slot >= 0 && !self.walking.contains(&(slot as u16)))
        {
            return;
        }
        let o = self
            .observations
            .entry(slot)
            .or_insert_with(|| Observation {
                cell,
                stride: None,
                periods: Vec::new(),
            });
        if cell == o.cell {
            return;
        }
        let period = if chebyshev(cell, o.cell) == 1 {
            leg_duration(o.stride, time, 0.0)
        } else {
            0.0
        };
        o.cell = cell;
        o.stride = Some(if period == 0.0 {
            Stride::next(None, time)
        } else {
            Stride::next(o.stride, time)
        });
        o.periods.push((cell, time, period));
        if o.periods.len() > 4 {
            o.periods.remove(0);
        }
    }

    fn observed_period(&self, slot: i32, cell: (i32, i32), time: f64) -> Option<f64> {
        let observation = self.observations.get(&slot)?;
        // A target evicted from the four-observation ring is unknown, not a publication-time
        // sample. Use the tap duration rather than pair it with another logical step.
        Some(
            observation
                .periods
                .iter()
                .rev()
                .find(|p| p.0 == cell && p.1 <= time)
                .map_or(
                    self.duration,
                    |p| if p.2 == 0.0 { self.duration } else { p.2 },
                ),
        )
    }

    /// Whether a held walk can use measured cadence. The platform uses this to retain
    /// observations even where the first tap is too short for two display frames.
    pub fn walking(&self) -> bool {
        !self.walking.is_empty()
    }

    /// Camera sweep's cell rectangle, including one cell around the view. Lag is a visual
    /// offset, not a teleport: size follows the actual track rather than a 24px clamp.
    fn coverage(&self) -> Option<(i32, i32, i32, i32)> {
        let n = self.history.first()?.as_ref().as_ref();
        let (lo, hi) = self.camera.map_or(((0.0, 0.0), (0.0, 0.0)), |c| c.bounds());
        let low = |d: f64| ((d.min(0.0) / f64::from(CELL)).floor() as i32).min(-1);
        let high = |d: f64| ((d.max(0.0) / f64::from(CELL)).ceil() as i32).max(1);
        let x0 = n.origin.0.checked_add(low(lo.0))?;
        let y0 = n.origin.1.checked_add(low(lo.1))?;
        let x1 = n
            .origin
            .0
            .checked_add(i32::try_from(n.cols).ok()?)?
            .checked_add(high(hi.0))?;
        let y1 = n
            .origin
            .1
            .checked_add(i32::try_from(n.rows).ok()?)?
            .checked_add(high(hi.1))?;
        let w = usize::try_from(x1.checked_sub(x0)?).ok()?;
        let h = usize::try_from(y1.checked_sub(y0)?).ok()?;
        let budget = n
            .cols
            .checked_add(2 * SWEEP_MARGIN_CELLS)?
            .checked_mul(n.rows.checked_add(2 * SWEEP_MARGIN_CELLS)?)?
            .checked_mul((CELL * CELL) as usize)?
            .min(MAX_STATIC_PIXELS);
        (w.checked_mul(h)?.checked_mul((CELL * CELL) as usize)? <= budget)
            .then_some((x0, y0, x1, y1))
    }

    /// Pixel rectangle of the static layer at rest, large enough for the camera's sweep.
    pub fn static_bounds(&self) -> Option<(i32, i32, usize, usize)> {
        let n = self.history.first()?.as_ref().as_ref();
        let (x0, y0, x1, y1) = self.coverage()?;
        let (x, y) = n.pixel((x0, y0));
        Some((
            x,
            y,
            ((x1 - x0) * CELL) as usize,
            ((y1 - y0) * CELL) as usize,
        ))
    }

    /// Retain only scenes supplying cells in the current sweep that a newer scene lacks.
    /// Repeated idle pictures cannot evict the departing strip. Normally <= HISTORY;
    /// an unusually long lag retains the extra needed strip until the next publication.
    fn keep_coverage(&mut self) -> bool {
        let Some((x0, y0, x1, y1)) = self.coverage() else {
            return false;
        };
        let mut missing: BTreeSet<_> = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .collect();
        self.history.retain(|s| {
            let n = s.as_ref().as_ref();
            let before = missing.len();
            missing.retain(|&at| n.index(at).is_none());
            missing.len() != before
        });
        self.history.len() <= MAX_HISTORY
    }

    /// Resource-limit resets since clear. Ordinary cadence fixtures must keep this at zero.
    pub fn capacity_resets(&self) -> u64 {
        self.capacity_resets
    }

    /// Fewest engine ms a track must last to be shown (two display frames). Observations
    /// still advance when a short first tap snaps, so a later held interval can animate.
    pub fn set_minimum_duration(&mut self, ms: f64) {
        self.minimum_duration = ms.max(0.0);
    }

    /// Timing receipt, camera first (slot -1), then each mover: start, duration and the
    /// displacement from target at onset. Contains no game bytes or absolute world places.
    pub fn timings(&self) -> Vec<(i32, f64, f64, f64, f64)> {
        self.camera
            .iter()
            .map(|c| (-1, c))
            .chain(self.movers.iter().map(|(slot, c)| (i32::from(*slot), c)))
            .map(|(slot, c)| {
                (
                    slot,
                    c.start,
                    c.duration,
                    c.from.0 - c.to.0,
                    c.from.1 - c.to.1,
                )
            })
            .collect()
    }

    /// Actual distance travelled by each track, camera first. A route sums the Chebyshev
    /// length of all its segments; dividing by `timings`' duration gives its speed bound.
    pub fn path_distances(&self) -> Vec<(i32, f64)> {
        self.camera
            .iter()
            .map(|c| (-1, c))
            .chain(self.movers.iter().map(|(slot, c)| (i32::from(*slot), c)))
            .map(|(slot, c)| (slot, c.length()))
            .collect()
    }

    /// Forgets every scene and slide (a new game).
    pub fn clear(&mut self) {
        self.history.clear();
        self.capacity_resets = 0;
        self.walking.clear();
        self.walk_session = None;
        self.snap();
    }

    /// The scenes kept, newest first ([`Draw::Base`] names them by index).
    pub fn history(&self) -> &[Rc<T>] {
        &self.history
    }

    /// Every slide ends at once, a held preview with them, and the scenes are kept: the platform
    /// shows the engine's frame, and the next publication that moves something slides from the
    /// newest scene. A new generation ([`Presenter::generation`]). Not a publication: a foreign
    /// picture ([`Presenter::foreign`]), an open context menu (spec 17, u6-web `Game::slide_view`).
    pub fn snap(&mut self) {
        self.snaps += 1;
        self.camera = None;
        self.movers.clear();
        self.history.truncate(1);
        self.camera_stride = None;
        self.strides.clear();
        self.observations.clear();
        self.held = false;
        self.generation += 1;
    }

    /// Discontinuities, including implicit restarts for teleports and resource limits.
    pub fn snaps(&self) -> u64 {
        self.snaps
    }

    /// Drops the slides ended at `t` (each then stands where it slid to).
    pub fn end(&mut self, t: f64) {
        if self.camera.is_some_and(|c| c.done(t)) {
            self.camera = None;
        }
        self.movers.retain(|(_, m)| !m.done(t));
        if self.camera.is_none() && self.movers.is_empty() {
            self.history.truncate(1);
        }
    }

    /// The engine's command prompt is back (spec 20 Design 3): the turn a held preview was made
    /// for is over, and it ends; the slides run on. A turn whose picture of the map was the
    /// preview's own (nothing in the shown map changed: no publication) ends its hold here.
    pub fn prompt(&mut self) {
        self.held = false;
    }

    /// A scene the engine built ahead of its picture (the early picture of the leader's step):
    /// slid to as [`Presenter::publish`] does, then held at rest until the engine publishes a
    /// picture (a scene or a foreign one) or its command prompt is back ([`Presenter::prompt`]),
    /// whichever comes first; no timer. Where slides are not shown it is left out, unless it snaps
    /// ([`Scene::snap`]: the first scene after a load): then the presenter starts again from it,
    /// as from a publication that snaps. The engine counts a scene that snaps handed over once it
    /// queued it (u6-core `Io::break_motion` ends there), so no scene before it may stay here.
    pub fn preview(&mut self, shot: Rc<T>, time: f64, slides: bool) {
        self.accept_preview(shot, time, slides);
    }

    /// Whether the early scene was actually accepted; an omitted preview cannot restore
    /// a platform's world overlay after a foreign picture.
    pub fn accept_preview(&mut self, shot: Rc<T>, time: f64, slides: bool) -> bool {
        if slides && self.duration > 0.0 {
            self.publish(shot, time, true);
            self.held = true;
            true
        } else if (*shot).as_ref().snap {
            self.restart(shot);
            true
        } else {
            false
        }
    }

    /// A publication with no scene of its own (a missile, a flash, a quake): every slide ends, so
    /// the platform shows the engine's frame ([`Presenter::snap`]).
    pub fn foreign(&mut self) {
        self.snap();
    }

    /// The engine published `shot`'s scene at engine time `time`; `slides`: whether slides are
    /// shown now ([`slides_shown`]).
    pub fn publish(&mut self, shot: Rc<T>, time: f64, slides: bool) {
        if !time.is_finite() {
            return self.restart(shot);
        }
        self.end(time);
        self.held = false;
        let before = (self.camera, self.movers.clone());
        let new = (*shot).as_ref();
        let follows = |prev: &Scene| {
            prev.leader == new.leader
                && prev.centre.2 == new.centre.2
                && (prev.cols, prev.rows, prev.corner) == (new.cols, new.rows, new.corner)
                && chebyshev((prev.centre.0, prev.centre.1), (new.centre.0, new.centre.1)) <= 1
        };
        let prev = match self.history.first() {
            Some(p) if slides && self.duration > 0.0 && !new.snap && follows((**p).as_ref()) => {
                Rc::clone(p)
            }
            _ => return self.restart(shot),
        };
        let prev = (*prev).as_ref();
        let centre = |s: &Scene| (s.centre.0, s.centre.1);
        if centre(prev) != centre(new) {
            let from = self.camera.map_or(px(centre(prev)), |c| c.at(time));
            let to = px(centre(new));
            let duration = if self.walking() {
                self.observed_period(-1, centre(new), time)
                    .unwrap_or_else(|| leg_duration(self.camera_stride, time, self.duration))
            } else {
                self.duration
            };
            self.camera_stride = self
                .walking()
                .then_some(Stride::next(self.camera_stride, time));
            self.camera = if duration >= self.minimum_duration {
                let next =
                    self.camera
                        .map_or(Some(Track::straight(from, to, time, duration)), |old| {
                            old.retarget(
                                to,
                                time,
                                duration,
                                matches!(self.walk_session, Some(WalkSession::Route(_)))
                                    || old.via_len > 0,
                            )
                        });
                let Some(next) = next else {
                    self.restart(shot);
                    self.capacity_resets += 1;
                    return;
                };
                Some(next)
            } else {
                None
            };
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
                self.strides.remove(&p.slot);
                continue;
            }
            let from = running.map_or(px(q.cell), |m| m.at(time));
            let duration = if self.walking.contains(&p.slot) {
                let previous = self.strides.get(&p.slot).copied();
                self.strides.insert(p.slot, Stride::next(previous, time));
                self.observed_period(i32::from(p.slot), p.cell, time)
                    .unwrap_or_else(|| leg_duration(previous, time, self.duration))
            } else {
                self.duration
            };
            if duration >= self.minimum_duration {
                let next = running.map_or(Some(Track::straight(from, to, time, duration)), |old| {
                    old.retarget(
                        to,
                        time,
                        duration,
                        (matches!(self.walk_session, Some(WalkSession::Route(_)))
                            && self.walking.contains(&p.slot))
                            || old.via_len > 0,
                    )
                });
                let Some(next) = next else {
                    self.restart(shot);
                    self.capacity_resets += 1;
                    return;
                };
                movers.push((p.slot, next));
            }
        }
        self.movers = movers;
        self.strides.retain(|slot, _| new.find(*slot).is_some());
        self.history.insert(0, shot);
        if !self.keep_coverage() {
            let latest = Rc::clone(&self.history[0]);
            self.restart(latest);
            self.capacity_resets += 1;
        }
        if (self.camera, &self.movers) != (before.0, &before.1) {
            self.generation += 1;
        }
    }

    /// `shot` alone, nothing sliding: what the engine shows.
    fn restart(&mut self, shot: Rc<T>) {
        // The first picture can arrive after the first observed step. It establishes the
        // scene baseline without erasing that authoritative timestamp.
        let observations = if self.history.is_empty() && !shot.as_ref().as_ref().snap {
            std::mem::take(&mut self.observations)
        } else {
            BTreeMap::new()
        };
        self.history.clear();
        self.history.push(shot);
        self.snap();
        self.observations = observations;
    }

    /// Whether a slide runs at `t`, or a preview is held (the platform draws the plan then, else
    /// the engine's frame).
    pub fn active(&self, t: f64) -> bool {
        self.camera.is_some_and(|c| !c.done(t))
            || self.movers.iter().any(|(_, m)| !m.done(t))
            || self.held
    }

    /// The slides as drawn at `t` (at their end once `t` passed it, until [`Presenter::end`]),
    /// and a preview held at rest; `None` when neither is.
    pub fn view(&self, t: f64) -> Option<View> {
        let idle = self.camera.is_none() && self.movers.is_empty();
        if idle && !self.held || self.history.is_empty() {
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
        self.layers().plan(view)
    }

    /// Bumped whenever what slides changes: a camera slide starts or restarts, a mover joins,
    /// restarts or leaves the slides, the slides end at once ([`Presenter::snap`]: a foreign
    /// picture, an open context menu; [`Presenter::clear`]; a scene that snaps). A publication that only lets the slides run on (the same
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
        self.layers_with_actors(&[], 0)
    }

    /// Give display-only actor poses their own layers, including while stationary. Extra
    /// coverage changes occlusion only, never ground position or draw order. Up to 3 pixels
    /// pads above a walking foot; larger margins cover a strike in every direction.
    pub fn layers_with_actors(&self, actors: &[u16], lift: i32) -> Layers {
        let mut tracks = self.movers.clone();
        for &slot in actors.iter().filter(|&&s| s < 256) {
            if !tracks.iter().any(|(s, _)| *s == slot) {
                tracks.push((slot, Track::straight((0.0, 0.0), (0.0, 0.0), 0.0, 0.0)));
            }
        }
        let Some(newest) = self.history.first() else {
            return Layers::default();
        };
        let n = (**newest).as_ref();
        if !actors.is_empty() {
            // Ground-position ties must retain the scene's own node order, not actor-slot
            // order: two standing people may occupy one cell.
            tracks.sort_by_cached_key(|(slot, _)| {
                n.find(*slot).and_then(|p| n.index(p.cell)).map_or(
                    (usize::MAX, usize::MAX),
                    |cell| {
                        (
                            cell,
                            n.cells[cell]
                                .nodes
                                .iter()
                                .position(|node| node.slot == Some(*slot))
                                .unwrap_or(usize::MAX),
                        )
                    },
                )
            });
        }
        let moving = |slot: Option<u16>| slot.is_some_and(|s| tracks.iter().any(|m| m.0 == s));
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
        let Some((x0, y0, x1, y1)) = self.coverage() else {
            return Layers::default();
        };
        let (xs, ys) = (x0..x1, y0..y1);
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
        for &(slot, track) in &tracks {
            // An object's nodes lie in its own cell and the cells west and north of it (its other
            // parts), or east (a doorway's side tile).
            let Some(home) = n.find(slot).map(|p| p.cell) else {
                continue;
            };
            // The pixels the mover is drawn from its place, start to end: each cell it covers on
            // the way, the actual restart's start and a diagonal's side cells included;
            // drawn at whole pixels (`Track::offset` rounds), so from the way's floor to its ceil.
            let (mut low, mut high) = track.bounds();
            if actors.contains(&slot) {
                let margin = f64::from(lift.clamp(0, 9));
                low.1 -= margin;
                if lift > 3 {
                    low.0 -= margin;
                    high.0 += margin;
                    high.1 += margin;
                }
            }
            let swept = |c: i32, low: f64, high: f64| {
                let (lo, hi) = (
                    (f64::from(c * CELL) + low).floor(),
                    (f64::from(c * CELL) + high).ceil(),
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
                    // Only cells that can reach the shown camera sweep can occlude a mover.
                    // Intersect before iterating: a pathological offscreen mover's long lag
                    // must not allocate/scan an unbounded world rectangle.
                    let ys = swept(cy, low.1, high.1);
                    let xs = swept(cx, low.0, high.0);
                    for y in (*ys.start()).max(y0)..=(*ys.end()).min(y1 - 1) {
                        for x in (*xs.start()).max(x0)..=(*xs.end()).min(x1 - 1) {
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
    /// Place an already prepared set of layers, preserving their ground-based ordering.
    pub fn plan(&self, view: &View) -> Vec<Op> {
        let mut ops: Vec<Op> = Vec::new();
        for (layer, (dx, dy)) in self.placed(view) {
            let from = match layer {
                Part::Static => &self.statics,
                Part::Mover(i) => &self.movers[i].ops,
                Part::Over(i) => &self.movers[i].over,
            };
            ops.extend(from.iter().map(|op| Op {
                x: op.x + dx,
                y: op.y + dy,
                ..*op
            }));
        }
        ops
    }

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
