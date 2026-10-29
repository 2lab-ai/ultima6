//! Port-only (spec 21): the travel to a pressed map cell ("여기로 이동").
//!
//! The search is an A* over the player's own step rule ([`super::party::step_test`] fed
//! [`super::walk::can_move_to`] for the leader) and its cost (0x11ef84's of the cell left with the
//! leader on it, [`super::party::DIAGONAL_COST`] more on a diagonal), inside the map window. The
//! walk is one step per command prompt, entered as that step's direction key ([`Travel::next`],
//! which [`crate::Engine::tick`] asks before it queues a tick's input), so every rule, turn and
//! picture of a key's step applies. The travel reads the state only, draws no RNG and lives in the
//! engine, not in the savegame.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::engine::{InputEvent, Key, command_prompt};
use crate::geom::{VIEW_H, VIEW_W, WINDOW};
use crate::main_loop::Program;
use crate::map::flags::CELL_VISIBLE;
use crate::state::State;

use super::party::{self, StepTest};
use super::walk::{self, DX8, DY8};
use super::{leaves, seams};

/// Why a travel does not start ([`reachable`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The leader stands on the cell.
    Here,
    /// A vessel carries the party: its moves follow the wind and the current, not the key.
    Aboard,
    /// Combat mode.
    Combat,
    /// A hostile is in view ([`foe_in_view`]).
    Foe,
    /// The cell lies outside the map window the search covers.
    Far,
    /// No walk inside the window reaches the cell.
    Unreachable,
    /// No leader stands on the map.
    NoLeader,
    /// The command loop's program is not running (the opening, the main menu).
    NoGame,
}

impl Reason {
    /// Every reason, for the page's words.
    pub const ALL: [Reason; 8] = [
        Reason::Here,
        Reason::Aboard,
        Reason::Combat,
        Reason::Foe,
        Reason::Far,
        Reason::Unreachable,
        Reason::NoLeader,
        Reason::NoGame,
    ];

    /// A stable ASCII code for the page's words.
    pub fn code(self) -> &'static str {
        match self {
            Reason::Here => "here",
            Reason::Aboard => "aboard",
            Reason::Combat => "combat",
            Reason::Foe => "foe",
            Reason::Far => "far",
            Reason::Unreachable => "unreachable",
            Reason::NoLeader => "no-leader",
            Reason::NoGame => "no-game",
        }
    }
}

/// Why a travel ended (spec 21 Stops).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The leader stands on the target at a prompt.
    Arrived,
    /// The next step is refused, or the last went elsewhere, and the one new plan found no way.
    Blocked,
    /// Combat mode at a prompt.
    Combat,
    /// A hostile in view at a prompt ([`foe_in_view`]).
    Foe,
    /// A key, a repeat, text or a pointer button pressed.
    Input,
    /// A key wait other than the command prompt (a more prompt, a question, a conversation).
    Prompt,
    /// A following member stands more than [`LEASH`] cells from the leader.
    Party,
    /// The leader is not where a step could have put it (a level change, a teleport, another
    /// leader), or a Restore ran (a load: [`crate::saveload::SaveLoad::restores`] moved).
    Displaced,
    /// No leader, or the game left the command loop's program.
    Gone,
}

impl End {
    /// Every end, for the page's words.
    pub const ALL: [End; 9] = [
        End::Arrived,
        End::Blocked,
        End::Combat,
        End::Foe,
        End::Input,
        End::Prompt,
        End::Party,
        End::Displaced,
        End::Gone,
    ];

    /// A stable ASCII code for the page's words.
    pub fn code(self) -> &'static str {
        match self {
            End::Arrived => "arrived",
            End::Blocked => "blocked",
            End::Combat => "combat",
            End::Foe => "foe",
            End::Input => "input",
            End::Prompt => "prompt",
            End::Party => "party",
            End::Displaced => "displaced",
            End::Gone => "gone",
        }
    }
}

/// How far (Chebyshev, in cells) a following member may stand from the leader before the walk
/// stops ([`End::Party`]): how far 0x11f0a0 lets a party member end from the leader in combat
/// mode ([`super::walk::step`]), the original's one leash.
pub const LEASH: u16 = 8;

/// The points of a step and its count weigh as one number: points × this + steps, so fewer
/// steps break a tie of points (more than any path's steps: at most `WINDOW²`).
const STEP_WEIGHT: u64 = 1 << 16;

/// The direction key of 8-way direction `d` (0 = north, clockwise; [`DX8`], [`DY8`]).
pub fn key_of(d: u8) -> Key {
    [
        Key::Up,
        Key::UpRight,
        Key::Right,
        Key::DownRight,
        Key::Down,
        Key::DownLeft,
        Key::Left,
        Key::UpLeft,
    ][usize::from(d & 7)]
}

/// The map window the search covers: [`WINDOW`] cells a side from the origin of the window cache
/// (A4-0x2ef8 / -0x2ef6), coordinates under the level's mask (A4-0x796c).
#[derive(Clone, Copy, Debug)]
struct Window {
    x0: u16,
    y0: u16,
    mask: u16,
}

impl Window {
    fn of(s: &State) -> Window {
        let (x0, y0, mask) = seams::window_origin(s);
        Window { x0, y0, mask }
    }

    /// The window cell of map (x, y), if it lies inside.
    fn cell(&self, (x, y): (u16, u16)) -> Option<usize> {
        let col = usize::from(x.wrapping_sub(self.x0) & self.mask);
        let row = usize::from(y.wrapping_sub(self.y0) & self.mask);
        (col < WINDOW && row < WINDOW).then_some(row * WINDOW + col)
    }

    /// The map cell of window cell `i`.
    fn at(&self, i: usize) -> (u16, u16) {
        let (col, row) = ((i % WINDOW) as u16, (i / WINDOW) as u16);
        (
            self.x0.wrapping_add(col) & self.mask,
            self.y0.wrapping_add(row) & self.mask,
        )
    }

    fn norm(&self, (x, y): (u16, u16)) -> (u16, u16) {
        (x & self.mask, y & self.mask)
    }
}

/// The answers of the step rule for one pass of the search: each window cell's
/// [`walk::can_move_to`] for the leader on the search's copy of the state, with the pass's
/// damaging-tile switch (A4-0x6a8a) and without it, and each cell's cost, asked once.
struct Grid {
    w: Window,
    leader: u16,
    z: u16,
    /// The pass's switch: 1 refuses harm on the cells a step enters.
    avoid: u16,
    /// The leader's own cell, and what its tile adds to 0x11ef84's cost of any cell it leaves.
    home: Option<usize>,
    own: i16,
    strict: Vec<Option<bool>>,
    plain: Vec<Option<bool>>,
    cost: Vec<Option<i16>>,
}

impl Grid {
    fn new(c: &State, w: Window, leader: u16, avoid: u16) -> Grid {
        let p = c.objects.location[usize::from(leader)];
        Grid {
            w,
            leader,
            z: walk::party_z(c),
            avoid,
            home: w.cell((p.x, p.y)),
            own: walk::object_cost(c, c.objects.type_frame[usize::from(leader)]),
            strict: vec![None; WINDOW * WINDOW],
            plain: vec![None; WINDOW * WINDOW],
            cost: vec![None; WINDOW * WINDOW],
        }
    }

    /// May the leader enter map (x, y): 0x11b1c0 on the copy, with the pass's switch when
    /// `strict` (the cell a step enters), else without it (the side cells a diagonal looks at,
    /// as a key's step tests them: 0x11d4ee calls 0x11b1c0 with the switch clear); a cell outside
    /// the window is not searched.
    fn enter(&mut self, c: &mut State, x: u16, y: u16, strict: bool) -> bool {
        let Some(i) = self.w.cell((x, y)) else {
            return false;
        };
        let (avoid, answers) = if strict && self.avoid != 0 {
            (self.avoid, &mut self.strict)
        } else {
            (0, &mut self.plain)
        };
        let leader = self.leader;
        *answers[i].get_or_insert_with(|| {
            c.actors.work.avoid_damage = avoid;
            let open = walk::can_move_to(c, leader, x, y);
            c.actors.work.avoid_damage = 0;
            open
        })
    }

    /// The points a step leaving window cell `i` pays on foot before 0x11c650's factors:
    /// 0x11ef84's cost of the cell with the leader standing on it ([`walk::tile_cost`], plus the
    /// leader's own tile where it does not stand now).
    fn cost(&mut self, c: &State, i: usize) -> i16 {
        let (x, y) = self.w.at(i);
        let own = if Some(i) == self.home { 0 } else { self.own };
        *self.cost[i].get_or_insert_with(|| walk::tile_cost(c, x, y, self.z) + own)
    }
}

/// What one search did: the steps from the start to the goal (directions, the first first), and
/// the cells it expanded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Search {
    pub path: Option<Vec<u8>>,
    pub expanded: usize,
}

/// The A*: from `from` to `to` inside the window on `c` (the search's copy), the leader's step
/// rule deciding each step ([`party::step_test`]) and its cost the points the step pays on foot.
/// The estimate, 5 points a cell of Manhattan distance and one step a cell of Chebyshev distance,
/// never overestimates (a straight step pays at least 5, a diagonal at least 10 and covers one
/// cell of each axis), so the path is a cheapest one; ties of points go to fewer steps, then to the
/// nearer cell, then to the lower window cell. Each cell is expanded at most once.
fn search(c: &mut State, grid: &mut Grid, from: (u16, u16), to: (u16, u16)) -> Search {
    let w = grid.w;
    let none = Search {
        path: None,
        expanded: 0,
    };
    let (Some(start), Some(goal)) = (w.cell(from), w.cell(to)) else {
        return none;
    };
    let (gx, gy) = ((goal % WINDOW) as i32, (goal / WINDOW) as i32);
    let estimate = |i: usize| {
        let dx = ((i % WINDOW) as i32 - gx).unsigned_abs() as u64;
        let dy = ((i / WINDOW) as i32 - gy).unsigned_abs() as u64;
        5 * (dx + dy) * STEP_WEIGHT + dx.max(dy)
    };
    let n = WINDOW * WINDOW;
    let mut g = vec![u64::MAX; n];
    let mut came = vec![0u8; n];
    let mut closed = vec![false; n];
    let mut heap = BinaryHeap::new();
    g[start] = 0;
    heap.push(Reverse((estimate(start), estimate(start), start)));
    let mut expanded = 0;
    while let Some(Reverse((_, _, i))) = heap.pop() {
        if closed[i] {
            continue;
        }
        closed[i] = true;
        expanded += 1;
        if i == goal {
            let mut path = Vec::new();
            let mut k = goal;
            while k != start {
                let d = came[k];
                path.push(d);
                let (x, y) = w.at(k);
                let back = (
                    x.wrapping_add_signed(-DX8[usize::from(d)]),
                    y.wrapping_add_signed(-DY8[usize::from(d)]),
                );
                k = w.cell(back).expect("a step's source lies in the window");
            }
            path.reverse();
            return Search {
                path: Some(path),
                expanded,
            };
        }
        let here = w.at(i);
        for d in 0..8u8 {
            let Some(j) = w.cell(party::step_cell(here, usize::from(d))) else {
                continue;
            };
            if closed[j] {
                continue;
            }
            let (nx, ny) = party::step_cell(here, usize::from(d));
            if !grid.enter(c, nx, ny, true) {
                continue;
            }
            let test = party::step_test(|x, y| grid.enter(c, x, y, false), here, usize::from(d));
            if test != StepTest::Open {
                continue;
            }
            let diagonal = DX8[usize::from(d)] != 0 && DY8[usize::from(d)] != 0;
            let points = grid.cost(c, i) + if diagonal { party::DIAGONAL_COST } else { 0 };
            let step = points.max(0) as u64 * STEP_WEIGHT + 1;
            let ng = g[i] + step;
            if ng < g[j] {
                g[j] = ng;
                came[j] = d;
                heap.push(Reverse((ng + estimate(j), estimate(j), j)));
            }
        }
    }
    Search {
        path: None,
        expanded,
    }
}

/// The plan of a walk from `from` to `to` on `s`: [`plan_on`] a copy of it. The real state is not
/// touched.
pub(crate) fn plan(s: &State, leader: u16, from: (u16, u16), to: (u16, u16)) -> Search {
    plan_on(&mut s.clone(), leader, from, to)
}

/// The search on `c`, the plan's copy of the state, with the hiccup flag (A4-0x6a9c) clear, so
/// the step rule queues no line: first with its damaging-tile switch set (A4-0x6a8a, the one NPC
/// steps use) on the cells a step enters, refusing harm, then, if that finds no way, without it,
/// as a key walks. Draws no RNG from `c` (tested).
pub(crate) fn plan_on(c: &mut State, leader: u16, from: (u16, u16), to: (u16, u16)) -> Search {
    c.actors.work.hic = 0;
    let w = Window::of(c);
    let mut expanded = 0;
    for avoid in [1, 0] {
        let mut grid = Grid::new(c, w, leader, avoid);
        let found = search(c, &mut grid, from, to);
        expanded += found.expanded;
        if found.path.is_some() {
            return Search {
                path: found.path,
                expanded,
            };
        }
    }
    Search {
        path: None,
        expanded,
    }
}

/// Is the step from `here` in direction `d` open now: the step rule on a copy of `s`, as
/// [`plan`] asks it.
fn step_open(s: &State, leader: u16, here: (u16, u16), d: u8) -> bool {
    let mut c = s.clone();
    c.actors.work.hic = 0;
    party::step_test(
        |x, y| walk::can_move_to(&mut c, leader, x, y),
        here,
        usize::from(d),
    ) == StepTest::Open
}

/// A hostile in view of `leader`: an actor slot not in the party (status 0x80 clear), not dead
/// (status 0x10 clear), aligned evil (status & 0x60 = 0x20) or chaotic (0x60), the alignments
/// 0x12500a lets take a good (0x40) actor as a foe ([`crate::combat::npc::find_target`]); on a view
/// cell the leader sees (view flags bit 0x80) and seen by 0x1224ea ([`leaves::can_see`]).
pub fn foe_in_view(s: &State, leader: u16) -> bool {
    let w = Window::of(s);
    let [vx, vy] = s.map.view_offset;
    let z = s.objects.location[usize::from(leader)].z;
    (1..0x100u16).any(|a| {
        let i = usize::from(a);
        let status = s.actors.attrs.status[i];
        let p = s.objects.location[i];
        if s.objects.type_frame[i] == 0
            || a == leader
            || status & 0x90 != 0
            || !matches!(status & 0x60, 0x20 | 0x60)
            || p.z != z
        {
            return false;
        }
        let col = i32::from(p.x.wrapping_sub(w.x0) & w.mask) - i32::from(vx);
        let row = i32::from(p.y.wrapping_sub(w.y0) & w.mask) - i32::from(vy);
        if !(0..VIEW_W as i32).contains(&col) || !(0..VIEW_H as i32).contains(&row) {
            return false;
        }
        let cell = s.map.view_cell(col as usize, row as usize);
        let seen = s
            .map
            .view_flags
            .get(cell)
            .is_some_and(|f| f & CELL_VISIBLE != 0);
        seen && leaves::can_see(s, leader, a)
    })
}

/// A member the followers' pass moves ([`party::following`]) stands more than [`LEASH`] cells
/// from the leader (the distance under the level's mask: a member across the map's seam stands
/// next to the leader).
fn left_behind(s: &State, leader: u16) -> bool {
    let l = s.objects.location[usize::from(leader)];
    let mask = s.map.world_mask;
    party::following(s).into_iter().any(|m| {
        let p = s.objects.location[usize::from(m)];
        chebyshev((p.x, p.y), (l.x, l.y), mask) > LEASH
    })
}

/// The Chebyshev distance of `a` and `b` on a level whose coordinates wrap under `mask`
/// (A4-0x796c: 0x3ff on the surface, 0xff below): on each axis the shorter way round.
pub(crate) fn chebyshev(a: (u16, u16), b: (u16, u16), mask: u16) -> u16 {
    let d = |p: u16, q: u16| {
        let ahead = p.wrapping_sub(q) & mask;
        ahead.min(mask - ahead + 1)
    };
    d(a.0, b.0).max(d(a.1, b.1))
}

/// A key wait other than the command prompt ([`command_prompt`], spec 19's one definition: the
/// command loop reads a command key, A4-0x7a2c set, with keys going through command processing,
/// A4-0x7a42 set, the more prompt clears it, and no cursor up, A4-0x7a60 = 0: with Tab's
/// cross-hair, 1, or the panel cursor, 2, a direction key moves the cursor, `ui/target.rs`
/// `tab_key`, `targeting_key` and `step`): the line editor (A4-0x7a57), whatever else is set, or
/// the event wait 0x106bbc while the command prompt is not up. The line editor's flag is read
/// here, where it decides the result; the command prompt's definition needs none (the line
/// editor never runs while A4-0x7a2c is set: it is entered only from a command's handler, after
/// the command read cleared the flag, `main_loop/commands.rs` `command_loop`).
fn other_prompt(s: &State) -> bool {
    s.ui.input.line_input || (!command_prompt(s) && s.ui.input.in_wait)
}

/// Why `s` refuses a travel to map cell `target` on the party's level, `None` when it may start:
/// the menu's enabled state. Reads `s` only (the search runs on a copy).
pub fn reachable(s: &State, target: (u16, u16)) -> Option<Reason> {
    Travel::start(s, target).err()
}

/// What the platform's input of one tick holds, for [`Travel::next`].
#[derive(Clone, Copy, Debug)]
pub struct Polled<'a> {
    /// The tick's input, before it is queued.
    pub input: &'a [InputEvent],
    /// The handler's pointer buttons before it.
    pub buttons: u8,
    /// A key or text waits in the engine's queue.
    pub queued: bool,
    /// One of them is not the travel's own last press: a key queued before the travel started
    /// or past its input.
    pub foreign: bool,
    /// Spec 21's merge contract with the context menu (spec 17): the menu took new input in this
    /// tick (its [`crate::ui::popup::Popup::took_new_input`]; `input` is then what the menu passed
    /// on, without what it took), or after the choice of "Go here" that started the travel in it
    /// ([`crate::ui::popup::GoHere::later`]; `input` then what it passed on after the choice): a
    /// stop, as the player's input is.
    pub menu: bool,
    /// Spec 21 S3: a step is due on the platform's clock
    /// ([`crate::engine::Engine::set_travel_due`]: [`crate::engine::MIN_STEP_MS`] since the prompt
    /// last took a press). Read only where the travel would press, after the stops; true at every
    /// prompt by default.
    pub due: bool,
}

impl Polled<'_> {
    /// A key, a repeat or text, a pointer button that was up before going down, a key in the
    /// queue the travel did not press, or new input the context menu took. No wildcard arm: an
    /// event kind added later (K2's `Paced`) is decided here (spec 21 Stops 5). Read at a tick's
    /// press of the travel and, in the tick that chose "Go here", over the events after the choice
    /// (`engine.rs` `start_go_here`).
    pub(crate) fn stops(&self) -> bool {
        if self.foreign || self.menu {
            return true;
        }
        let mut buttons = self.buttons;
        self.input.iter().any(|e| match *e {
            InputEvent::Key(_)
            | InputEvent::Repeat(_)
            | InputEvent::Paced(_)
            | InputEvent::Text(_) => true,
            InputEvent::Release(_) => false,
            InputEvent::Pointer { buttons: b, .. } => {
                let pressed = b & !buttons != 0;
                buttons = b;
                pressed
            }
        })
    }
}

/// What a travel does at a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing now (the turn is still running, or a key waits).
    Hold,
    /// Press this key now: the next step.
    Press(Key),
    /// The walk is over.
    Stop(End),
}

/// A travel under way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Travel {
    target: (u16, u16),
    z: u8,
    leader: u16,
    /// The steps still to take, the next last.
    path: Vec<u8>,
    /// The cell the plan starts from (the leader's at the start).
    start: (u16, u16),
    /// The Restores run when the travel started ([`crate::saveload::SaveLoad::restores`]).
    restores: u32,
    /// The leader's cell when the last step was pressed, and the cell that step aims at.
    pressed: Option<((u16, u16), (u16, u16))>,
    /// The one new plan of the current blockage was made.
    replanned: bool,
    /// The steps that landed where they aimed (receipts).
    pub steps: u32,
    /// The keys pressed (receipts).
    pub presses: u32,
    /// The plans made, the first included (receipts).
    pub plans: u32,
}

impl Travel {
    /// A travel of the leader to map cell `target` (masked to the level's coordinates) on `s`, or
    /// why there is none.
    pub fn start(s: &State, target: (u16, u16)) -> Result<Travel, Reason> {
        if s.main.program != Program::Game {
            return Err(Reason::NoGame);
        }
        let leader = s.actors.party.leader().ok_or(Reason::NoLeader)?.0;
        let p = s.objects.location[usize::from(leader)];
        if s.objects.type_frame[usize::from(leader)] == 0 || u16::from(p.z) != walk::party_z(s) {
            return Err(Reason::NoLeader);
        }
        if s.actors.party.leader_index == s.actors.party.size {
            return Err(Reason::Aboard);
        }
        let w = Window::of(s);
        let target = w.norm(target);
        let here = w.norm((p.x, p.y));
        if target == here {
            return Err(Reason::Here);
        }
        if s.actors.combat_mode() {
            return Err(Reason::Combat);
        }
        if foe_in_view(s, leader) {
            return Err(Reason::Foe);
        }
        if w.cell(target).is_none() {
            return Err(Reason::Far);
        }
        let mut path = plan(s, leader, here, target)
            .path
            .ok_or(Reason::Unreachable)?;
        path.reverse();
        Ok(Travel {
            target,
            z: p.z,
            leader,
            path,
            start: here,
            restores: s.saveload.restores,
            pressed: None,
            replanned: false,
            steps: 0,
            presses: 0,
            plans: 1,
        })
    }

    /// The target cell.
    pub fn target(&self) -> (u16, u16) {
        self.target
    }

    /// The one new plan of a blockage, from `here`; false when it was made already or finds no
    /// way.
    fn replan(&mut self, s: &State, here: (u16, u16)) -> bool {
        if self.replanned {
            return false;
        }
        self.replanned = true;
        self.plans += 1;
        match plan(s, self.leader, here, self.target).path {
            Some(mut path) => {
                path.reverse();
                self.path = path;
                true
            }
            None => false,
        }
    }

    /// The travel at a tick, `s` as the last tick left it and `polled` this tick's input: the
    /// stops (spec 21), else at the command prompt with no key waiting the next step's key, else
    /// nothing.
    pub fn next(&mut self, s: &State, polled: &Polled<'_>) -> Next {
        if polled.stops() {
            return Next::Stop(End::Input);
        }
        if s.main.program != Program::Game {
            return Next::Stop(End::Gone);
        }
        if s.saveload.restores != self.restores {
            return Next::Stop(End::Displaced);
        }
        if other_prompt(s) {
            return Next::Stop(End::Prompt);
        }
        // Spec 21 S3: at most one step a MIN_STEP_MS of the platform's clock, tested here, after
        // the stops and before the last press is read back (which would pop the path).
        if !command_prompt(s) || polled.queued || !polled.due {
            return Next::Hold;
        }
        let Some(leader) = s.actors.party.leader().map(|l| l.0) else {
            return Next::Stop(End::Gone);
        };
        let p = s.objects.location[usize::from(leader)];
        if leader != self.leader || p.z != self.z {
            return Next::Stop(End::Displaced);
        }
        let w = Window::of(s);
        let here = w.norm((p.x, p.y));
        let mut blocked = false;
        // Where the leader stood when the last step was pressed; before the first press, the
        // plan's start (the leader may have moved between the start and the first prompt).
        let last = self.pressed.take();
        let from = last.map_or(self.start, |(from, _)| from);
        match last {
            Some((_, aimed)) if here == aimed => {
                self.path.pop();
                self.steps += 1;
                self.replanned = false;
            }
            None if here == self.start => {}
            _ if chebyshev(here, from, w.mask) > 1 => return Next::Stop(End::Displaced),
            _ => blocked = true,
        }
        if here == self.target {
            return Next::Stop(End::Arrived);
        }
        if s.actors.combat_mode() {
            return Next::Stop(End::Combat);
        }
        if foe_in_view(s, leader) {
            return Next::Stop(End::Foe);
        }
        if left_behind(s, leader) {
            return Next::Stop(End::Party);
        }
        let open = |d: Option<&u8>| d.is_some_and(|&d| step_open(s, leader, here, d));
        if (blocked || !open(self.path.last())) && !self.replan(s, here) {
            return Next::Stop(End::Blocked);
        }
        let Some(&d) = self.path.last() else {
            return Next::Stop(End::Blocked);
        };
        self.pressed = Some((here, w.norm(party::step_cell(here, usize::from(d)))));
        self.presses += 1;
        Next::Press(key_of(d))
    }
}

#[cfg(test)]
#[path = "travel_tests.rs"]
mod tests;
