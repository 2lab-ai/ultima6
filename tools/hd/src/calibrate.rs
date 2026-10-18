//! `calibrate`: sheets of the neighbour gate's pairs (spec 07 HD2 "Neighbour seams"), for a read
//! by eye before its limit, its measure or the edges are changed. It reads `gates.jsonl` of the
//! work dir through a snapshot (copied to the out dir first and read there) and the ×8 masters
//! of `--slices`, each file read once, into memory, at the start (its digest kept); nothing of
//! either is read again while the sheets are drawn. No lock is taken: a re-ingest while they are
//! read can mix two states, which this mode does not check for (`--final` does). It writes PNG
//! sheets and `samples.tsv` to the out dir, as `--final` does ([`out_dir`]: a fresh directory
//! beside it, swapped in whole at the end, an old one replaced only when this mode wrote it:
//! it holds [`out_dir::FIRST`]'s marker; on a failure the snapshot and `refused.txt`, no
//! sheet). It calls nothing: no `codex`, no upload. It samples
//! the first pass (below), or, with `--final`, draws a window of every item of the final pack
//! (the last section).
//!
//! - **The rows** ([`rows`]): the pairs checked in the first pass (`"pass":1`: the images ingest
//!   judged before any neighbour fallback), each in a bucket by its result ([`Bucket`]): `pass`
//!   (not failing: the negative controls), then the failing ones by ratio / limit,
//!   `fail-le1.1`, `fail-1.1-1.5` and `fail-gt1.5`; crossed with its sides (gen-gen, gen-mmpx).
//!   A row with nothing measured (a null ratio) is skipped.
//! - **The samples** ([`sample`]): `per` rows of each bucket × sides, drawn by the seed alone
//!   (the rows in pair order, then shuffled): the rows of the map on the surface (source `map`
//!   or `map+objects`, level 0) first, the other rows only to fill a short cell; in a sheet, by
//!   ratio / limit. `--source LABEL` ([`Source`]) samples only the rows with that label, the
//!   surface first after one row of each other level the cell has; `all`, the default, every
//!   row as above. A row's labels ([`labels`]) are its `source` split at `+` (`map+objects` has
//!   both) and its `label` or `labels` when it has them: `map`, `objects`, `replace`,
//!   `object-terrain` ([`LABELS`]), or any other by its name (a label no row has takes none).
//! - **A window** ([`frame`]): [`COLS`] × [`ROWS`] map cells, the pair's cell A at [`A_CELL`] and
//!   B east or south of it, drawn as the page's map window draws its cells into the frame: the
//!   map tile of each cell as the world places it (its chunks, the level's wrap), then the
//!   starting objects as the object draw stacks them ([`stacks`]), every pixel naming its record
//!   in the source plane. An animated tile is drawn at frame 0 (counter 0 of its first slot);
//!   in the pair's two cells a tile whose frames include the pair's record is drawn as that
//!   record, so the join shown is the one measured. `samples.tsv` says when a frame was chosen
//!   (`frames`: `frame0`, `pair`), and `missing` where the window does not show the pair
//!   measured: a join cell draws no tile with the pair's record, or tiles drawn after them
//!   cover every row of the join the gate measures (a row where both records are opaque on
//!   their lines: [`Framed::joins`]; a record shows where some pixel of it is left in the
//!   source plane, which the panels draw from).
//! - **The panels** ([`panels`]): the frame composed by the page's compositor
//!   ([`Compositor::compose`]), at S 4 and at S 2: classic (the frame's colours, nearest), no pack
//!   (MMPX of every record) and the pack of `--slices` (each master box-averaged ×8 to ×4 as
//!   `pack` writes it, in front of MMPX for every other record).
//! - **The sheets** ([`sheet`]): one per bucket × sides (`<bucket>-<sides>.png`, and `-s2.png` at
//!   S 2), a sample per row, each panel captioned with the sample's index, bucket, sides and
//!   ratio / limit; the join marked in the margins outside the panels, never on the art: a tick
//!   across the join line and a bar along the edge's length. A sheet taller than [`MAX_HEIGHT`] is
//!   split into parts (`-p1`, `-p2`, … at both scales). The `--at` windows go to `control.png`.
//!   With one source every name starts with it (`objects-fail-gt1.5-gen-mmpx.png`,
//!   `objects-control.png`).
//! - **`samples.tsv`**: a line per sheet row; `ratio_slices` is the ratio measured again from the
//!   images of `--slices` (equal to `ratio` when they are the images the first pass judged).
//!
//! `--at z,x,y[,edge]` adds the window of cell (x, y) of level z, its pair the one of the rows
//! drawn across its edge (east when not given), with its ratio and limit when it was measured.
//! `--at leader[,edge]` adds the window of the party leader's start cell, then the failing
//! gen-mmpx pairs drawn within [`NEAR`] cells of it on its level, nearest first, at most `per`.
//!
//! **`--final`** ([`final_read`]): the read of the final pack, every item of it, not a sample
//! (`--slices`, `--at`, `--per` and `--seed` are not taken).
//! - **The state** ([`final_read::fixed`]): snapshots of `gates.jsonl`, `ingest.json` and
//!   `jobs.jsonl` (copied to the out dir and read there), the masters of the work dir's
//!   `slices/` (tiles, portraits and bitmaps, listed once, each file read once, into memory, its
//!   digest kept; the tiles' decoded) and the plan of
//!   items, which must be the one every job was laid out from (its fingerprint: the log's item
//!   numbers name its items), every item the log names (listed, or sent back) laid out in some
//!   job; all read at the start. Before the run reports success it reads
//!   the three files and every master of `slices/` again and fails if any is not what it read
//!   (a re-ingest during the run: the sheets written then read no one state). `gates.jsonl` logs pass 1 whole
//!   and a later pass's failing pairs only, so the last pass of the fixed point (the one that
//!   fails nothing) is rebuilt: pass 1's pairs with a side still generated (a record with a
//!   master), each measured from the slices by the gate itself ([`measure`]), its sides the
//!   slices'. Where neither side changed since pass 1 the measure must be the one logged, every
//!   pair must pass, and a log that has the last pass's lines must have exactly the rebuilt pass.
//!   Pass 1's lines must be the world's pairs with a side generated then, by the gate's own
//!   [`neighbour::pairs`], each once, from the source and at the cell the world has it; each
//!   master must be the file ingest wrote (its digest in `ingest.json`; an older `ingest.json`
//!   with none is refused; the digest, [`work::digest`], catches a race or an accident, not a
//!   file swapped on purpose). The list, the masters, the plan's items, the fallbacks and pass 1's
//!   sides must agree too
//!   (every rule is listed at [`final_read::fixed`]); every pair of the last pass must have a
//!   cell. On a disagreement nothing is drawn: the error gives its kinds and counts,
//!   `disagreements.txt` in the out dir the details.
//! - **The windows**: one per item of tile records the fixed point left generated (listed whole
//!   in `ingest.json`; a multi-record object is one item; portraits and bitmaps have no window);
//!   then one per id of `--include FILE` (a record id a line, `t:N` or `N`, `#` to the line's end
//!   a comment, each id once; the owner's file, refused inside a git work tree), and for an id
//!   not generated in the final pack a row saying so and no window. A window is placed at the
//!   pair of the last pass that one of its records meets with the largest ratio /
//!   limit, the first in pair order on a tie (with `--source LABEL` one of that label: a survivor
//!   that meets none has no coverage window and is counted, an included id takes any pair);
//!   else, in no pair, where the world first draws one of its records (level, row, column), said
//!   in the row; else, drawn nowhere in the starting world, the item alone in empty windows
//!   (each record as itself: one the animation always replaces, which the world never draws
//!   as itself, lands here), its pieces in rows from the second column and row with a cell
//!   between them, in as many
//!   windows as they need (the row says which of how many). Then every pair of the last pass
//!   that none of those windows draws (with one label, a pair of it) gets a window placed at
//!   it, in pair order, each counting the pairs it draws. Then the records of an item with a
//!   window that no window shows (a window at a pair or where the world draws the item shows
//!   the records near A's cell: one frame of an actor) are drawn alone, the item's pieces that
//!   have them, in as many windows as they need: every record of every item with a window is
//!   drawn in one. Every window is placed, and the pairs it draws found, before
//!   any sheet or `samples.tsv` is written: a pair of the last pass that is still drawn in no
//!   window (its own window does not show it: a side covered at its cell, `unread`), or a window
//!   of an item or an included id that shows none of its records (covered where it is drawn),
//!   or a piece that fits no window (`hidden`), fails the run, nothing drawn, the details in
//!   `unshown.txt`; every other record of an item with a window is then drawn in one (an
//!   invariant the rest windows keep, asserted in debug builds, not a check). The frame as
//!   above, A's cell at [`A_CELL`]: at a pair, its records in its two cells; where it is drawn,
//!   its record in A's cell.
//! - **The marks**: every pair of the last pass drawn inside the window (a record of a cell
//!   beside one of the cell east or south of it, both left on a row of the join the gate
//!   measures, [`Framed::joins`]: a pair covered there by a tile drawn after it is not drawn) is
//!   marked
//!   in the panels' margins as above and listed under the panels: its cell (column, row of the
//!   window), edge, labels, sides and ratio / limit, `>` before the pair the window is placed at
//!   (a line too long for the sheet goes on, indented, in lines of its own: nothing is cut).
//!   A pair of the last pass whose two records are drawn at a join but covered there is judged
//!   hidden: marked in a colour of its own, listed with `hidden`, and not read.
//! - **The sheets** ([`final_read::final_sheet`]): `final-<class>.png` by the class ingest
//!   names, `final-<class>-alone.png` for the windows drawn nowhere, `final-pairs.png` for the
//!   pairs no other window draws and `final-include.png`, in
//!   parts under [`MAX_HEIGHT`], each with its `-s2.png`; with one `--source` every name starts
//!   with it. A window's caption: its index, class, placement and why it is drawn.
//! - **`samples.tsv`** ([`final_read::FINAL_TSV_HEADER`]): a line per window, then per included
//!   id not generated: the index, `coverage`, `pair` or `include`, the item, its records (a pair
//!   window: the pair's generated ones), its class, the placement, A's cell (z, x, y) and edge,
//!   the pairs marked, their labels × sides with counts, each pair (cell, edge, labels, sides,
//!   A, B, ratio, limit), each pair judged hidden (cell, edge, labels, sides, A, B), the sheet
//!   and row, `frames` (as above; `itself` for a record drawn alone as itself where its tile's
//!   frame 0 is another), and a note.
//! - **The report** ([`final_read::FinalSummary`]): survivors per class (items, images, windows by
//!   placement, the records drawn in a window of the survivors' records, the survivors with no
//!   map window: portraits and bitmaps, and with one label the survivors that meet no pair of
//!   it); then, for each label of [`LABELS`] and any other the last pass has, both sides,
//!   the pairs in the last pass, of them drawn in a window, and the windows drawing one (the
//!   pair windows too), 0 printed; the pair windows, and the pairs of the last pass drawn in
//!   no window (0: a run that leaves one fails).
//! - **The out dir** ([`out_dir`], [`final_read::MARKER`]): the run is written to a fresh
//!   directory beside `--out` and swapped in whole at the end, so after any run `--out` holds
//!   that run's output alone: on success its sheets and `samples.tsv`, drawn from one state; on
//!   a refusal, or any other failure (a sheet not written, the masters not read again), its
//!   `refused.txt` (the error), the details (`disagreements.txt` or `unshown.txt`) and the
//!   snapshots read, in a fresh dir, never a sheet the run drew; when the work dir changed
//!   during the run, `changed.txt` (what changed) too. An old `--out` is replaced only when it holds the marker file
//!   every run of this mode writes; anything else there (an empty dir too, or the first pass's)
//!   is left as it is and the run fails.
//!
//! The object draw (`u6_core::objects::draw`, 0x10e81e, 0x10e73a, 0x10e596) as far as the
//! starting lists tell it: the objects in list order, (y, x), and at one cell the later in the
//! starting lists first (0x10a7b2 links an object before those at its cell; the load's own
//! order is not modelled); each object's tile in its cell and its parts west and north of it; a
//! tile with flags3 0x20 replaces its cell's base tile; any other goes to the front of the
//! cell's list, but a top tile (flags2 0x10) behind the cell's other tiles and a top part last;
//! the cell drawn base first, then its list. Not modelled: what the view hides (hidden and dark
//! cells, light), actor effects (outlines, flashes), the two doorways' side tiles and the shore
//! merge (a shore record is drawn as loaded, as the gate measures it).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::Deserialize;
use u6_formats::gfx::{TILE_COUNT, TILE_W};
use u6_hdpack::{ImageId, SCALE};
use u6_render::hd::{Compositor, Image, Images, Pack, PackHd, Scale};
use u6_render::{Framebuffer, Kind, Rgb, Src, WIDTH};

use crate::atlas::{self, MASTER};
use crate::game::Game;
use crate::gate::Edge;
use crate::img::{self, Rgba};
use crate::neighbour::{self, At, Drawn, Pair, Side};
use crate::work::{self, Work};

pub mod final_read;
pub mod out_dir;
#[cfg(test)]
mod tests;

/// Map cells across a window.
pub const COLS: usize = 8;
/// Map cells down a window.
pub const ROWS: usize = 6;
/// The pair's cell A in its window: column, row.
pub const A_CELL: (usize, usize) = (3, 2);
/// The tallest sheet written, in pixels: a taller one is split into parts.
pub const MAX_HEIGHT: u32 = 8000;
/// How far from the leader's start cell `--at leader` looks for failing pairs, in cells.
pub const NEAR: usize = 8;
/// flags3: the tile replaces its cell's base tile (0x10e596).
const REPLACES_BASE: u8 = 0x20;
/// flags2: a top tile (0x10e596).
const TOP: u8 = 0x10;
/// The sheet's background, the captions and the join marks.
const BACK: [u8; 4] = [40, 40, 40, 255];
const INK: [u8; 4] = [235, 235, 235, 255];
const MARK: [u8; 4] = [255, 0, 255, 255];
/// The final read's mark of a pair whose sides are drawn at a join but covered there (judged
/// hidden, not read): drawn dashed, over the others, so neither hides the other.
const MARK_HIDDEN: [u8; 4] = [0, 200, 255, 255];

/// Which sides of a pair were generated in the first pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sides {
    GenGen,
    GenMmpx,
}

impl Sides {
    pub const ALL: [Sides; 2] = [Sides::GenGen, Sides::GenMmpx];

    pub fn name(self) -> &'static str {
        match self {
            Sides::GenGen => "gen-gen",
            Sides::GenMmpx => "gen-mmpx",
        }
    }
}

/// A pair checked in the first pass, as `gates.jsonl` has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub pair: Pair,
    pub fail: bool,
    pub ratio: f32,
    pub limit: f32,
    pub sides: Sides,
    pub source: String,
    pub at: At,
}

impl Row {
    /// The ratio over its limit.
    pub fn excess(&self) -> f32 {
        if self.limit > 0.0 {
            self.ratio / self.limit
        } else {
            f32::INFINITY
        }
    }

    /// Whether the pair has label `label` (`map`, `objects`, …): one of its labels, `+` between
    /// them in `source` ([`labels`]).
    pub fn from(&self, label: &str) -> bool {
        self.source.split('+').any(|s| s == label)
    }

    /// A pair of the map on the surface: what the samples take first.
    fn surface_map(&self) -> bool {
        self.at.z == 0 && self.from("map")
    }
}

/// The pair labels the neighbour gate writes (H5: `map`, `objects`; H7: `replace`, a 0x20 object
/// standing in for its cell's base, and `object-terrain`, an object meeting a neighbouring cell's
/// map record). The summary of `--final` prints each of them, 0 when no pair has it; a label
/// `gates.jsonl` has beyond these is taken by its name.
pub const LABELS: [&str; 4] = ["map", "objects", "replace", "object-terrain"];

/// The labels of a `gates.jsonl` pair line `v`, `+` between them, in the order first found:
/// its `source` split at `+`, then a `label` (a string) and `labels` (strings) when it has them.
pub fn labels(v: &serde_json::Value) -> String {
    let text = |k: &str| v.get(k).and_then(serde_json::Value::as_str);
    let list = v
        .get("labels")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str);
    let mut out: Vec<&str> = Vec::new();
    for s in text("source").into_iter().chain(text("label")).chain(list) {
        for l in s.split('+').filter(|l| !l.is_empty()) {
            if !out.contains(&l) {
                out.push(l);
            }
        }
    }
    out.join("+")
}

/// Whether the `gates.jsonl` line `v` is a pair line: its `neighbour` is an object. A line with
/// no `neighbour` is a line of another kind (`Ok(false)`); one whose `neighbour` is anything
/// else (null, a string, an array, …) is refused, not taken for another kind.
pub fn pair_line(v: &serde_json::Value) -> Result<bool, &'static str> {
    match v.get("neighbour") {
        None => Ok(false),
        Some(n) if n.is_object() => Ok(true),
        Some(_) => Err("a neighbour field that is not a pair (an object of a, b and edge)"),
    }
}

/// The rows a run takes (`--source`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// Every row; the map's on the surface first.
    #[default]
    All,
    /// The rows with one label ([`LABELS`], or any other `gates.jsonl` has, by its name).
    Label(String),
}

impl Source {
    /// `all`, or a label: letters, digits, `-` and `_` (it names the sheets). A label no row
    /// has is not refused: the run takes no row of it, and says so.
    pub fn parse(s: &str) -> Result<Source, String> {
        let name = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        match s {
            "all" => Ok(Source::All),
            _ if !s.is_empty() && s.chars().all(name) => Ok(Source::Label(s.to_string())),
            _ => Err(format!(
                "bad --source {s:?} (all, or a label of gates.jsonl: letters, digits, - and _)"
            )),
        }
    }

    /// `all` or the label.
    pub fn name(&self) -> &str {
        match self {
            Source::All => "all",
            Source::Label(l) => l,
        }
    }

    /// Whether the run takes row `r`.
    pub fn takes(&self, r: &Row) -> bool {
        match self {
            Source::All => true,
            Source::Label(l) => r.from(l),
        }
    }

    /// Whether `r` is of the rows a cell takes first: the map's on the surface for every row,
    /// the surface's for one label.
    fn first(&self, r: &Row) -> bool {
        match self {
            Source::All => r.surface_map(),
            Source::Label(_) => r.at.z == 0,
        }
    }

    /// What the sheets' names start with: nothing for every row, else the label and a dash
    /// (`objects-fail-gt1.5-gen-mmpx.png`).
    pub fn prefix(&self) -> String {
        match self {
            Source::All => String::new(),
            Source::Label(l) => format!("{l}-"),
        }
    }
}

/// The bucket of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bucket {
    /// Not failing.
    Pass,
    /// Failing, ratio / limit at most 1.1.
    Le11,
    /// Failing, ratio / limit above 1.1, at most 1.5.
    To15,
    /// Failing, ratio / limit above 1.5.
    Gt15,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [Bucket::Pass, Bucket::Le11, Bucket::To15, Bucket::Gt15];

    /// By the row's own verdict, then the failing ones by ratio / limit.
    pub fn of(r: &Row) -> Bucket {
        let q = r.excess();
        if !r.fail {
            Bucket::Pass
        } else if q <= 1.1 {
            Bucket::Le11
        } else if q <= 1.5 {
            Bucket::To15
        } else {
            Bucket::Gt15
        }
    }

    /// The sheet's name.
    pub fn name(self) -> &'static str {
        match self {
            Bucket::Pass => "pass",
            Bucket::Le11 => "fail-le1.1",
            Bucket::To15 => "fail-1.1-1.5",
            Bucket::Gt15 => "fail-gt1.5",
        }
    }

    /// The caption's.
    fn label(self) -> &'static str {
        match self {
            Bucket::Pass => "pass",
            Bucket::Le11 => "<=1.1",
            Bucket::To15 => "1.1-1.5",
            Bucket::Gt15 => ">1.5",
        }
    }
}

pub fn edge_name(e: Edge) -> &'static str {
    match e {
        Edge::East => "east",
        Edge::South => "south",
    }
}

fn parse_edge(s: &str) -> Option<Edge> {
    match s {
        "east" => Some(Edge::East),
        "south" => Some(Edge::South),
        _ => None,
    }
}

#[derive(Deserialize)]
struct RawPair {
    a: String,
    b: String,
    edge: String,
}

#[derive(Deserialize)]
struct RawAt {
    z: usize,
    x: usize,
    y: usize,
}

#[derive(Deserialize)]
struct RawRow {
    neighbour: RawPair,
    pass: usize,
    fail: bool,
    ratio: Option<f32>,
    limit: Option<f32>,
    sides: String,
    at: Option<RawAt>,
}

/// The first pass's pairs of `gates.jsonl` (`text`) that were measured and have a cell, and how
/// many of its pairs were skipped (nothing measured, or no cell).
pub fn rows(text: &str) -> Result<(Vec<Row>, usize), String> {
    let (mut out, mut skipped) = (Vec::new(), 0);
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bad = |e: &dyn std::fmt::Display| format!("gates.jsonl line {}: {e}", i + 1);
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| bad(&e))?;
        if !pair_line(&v).map_err(|e| bad(&e))? {
            continue;
        }
        let source = labels(&v);
        if source.is_empty() {
            return Err(bad(&"a pair with no label (source, label or labels)"));
        }
        let r: RawRow = serde_json::from_value(v).map_err(|e| bad(&e))?;
        if r.pass != 1 {
            continue;
        }
        let (Some(ratio), Some(limit), Some(at)) = (r.ratio, r.limit, r.at) else {
            skipped += 1;
            continue;
        };
        let record = |s: &str| match atlas::parse_image(s) {
            Some(ImageId::Tile(t)) => Ok(t),
            _ => Err(bad(&"not a pair of tile records")),
        };
        let sides = match r.sides.as_str() {
            "gen-gen" => Sides::GenGen,
            "gen-mmpx" => Sides::GenMmpx,
            _ => return Err(bad(&"sides is neither gen-gen nor gen-mmpx")),
        };
        out.push(Row {
            pair: Pair {
                a: record(&r.neighbour.a)?,
                b: record(&r.neighbour.b)?,
                edge: parse_edge(&r.neighbour.edge).ok_or_else(|| bad(&"no edge"))?,
            },
            fail: r.fail,
            ratio,
            limit,
            sides,
            source,
            at: At {
                z: at.z,
                x: at.x,
                y: at.y,
            },
        });
    }
    Ok((out, skipped))
}

/// SplitMix64: the samples' draw, from the seed alone.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Fisher-Yates.
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// What [`sample`] took from one bucket × sides.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    /// Rows the cell had, and how many of them it takes first (the map's on the surface; with
    /// one source, its rows on the surface).
    pub had: usize,
    pub surface: usize,
    /// The rows taken, in sheet order (by ratio / limit, then pair).
    pub rows: Vec<Row>,
}

/// `per` rows of every bucket × sides that has rows of `source` (module docs): each cell drawn
/// by its own generator from `seed` and the cell, so a cell's draw does not depend on another's
/// rows. `per` 0 takes nothing: no cell.
///
/// With every row ([`Source::All`]) a cell takes the map's rows on the surface first and the
/// others only to fill. With one source a cell takes, before its surface rows, one row of each
/// other level it has rows on (in level order, keeping a place for the surface when it has
/// rows there), so the sheets show every level the source reaches; then its surface rows, then
/// the rest.
pub fn sample(
    rows: &[Row],
    per: usize,
    seed: u64,
    source: Source,
) -> BTreeMap<(Bucket, Sides), Taken> {
    if per == 0 {
        return BTreeMap::new();
    }
    let mut by: BTreeMap<(Bucket, Sides), Vec<&Row>> = BTreeMap::new();
    for r in rows.iter().filter(|r| source.takes(r)) {
        by.entry((Bucket::of(r), r.sides)).or_default().push(r);
    }
    by.into_iter()
        .map(|(key, mut v)| {
            v.sort_by(|a, b| a.pair.cmp(&b.pair).then(a.at.cmp(&b.at)));
            let (mut first, mut rest): (Vec<&Row>, Vec<&Row>) =
                v.iter().copied().partition(|r| source.first(r));
            let cell = (key.0 as u64) << 8 | key.1 as u64;
            let mut rng = Rng(seed ^ cell.wrapping_mul(0xd6e8_feb8_6659_fd93));
            rng.shuffle(&mut first);
            rng.shuffle(&mut rest);
            let mut each: Vec<&Row> = Vec::new();
            if source != Source::All {
                // The first of each level in the drawn order, by level.
                let mut levels = BTreeSet::new();
                each = rest
                    .iter()
                    .copied()
                    .filter(|r| levels.insert(r.at.z))
                    .collect();
                each.sort_by_key(|r| r.at.z);
                each.truncate(if first.is_empty() { per } else { per - 1 });
                rest.retain(|r| !each.iter().any(|e| std::ptr::eq(*e, *r)));
            }
            let mut taken: Vec<Row> = each
                .iter()
                .chain(&first)
                .chain(&rest)
                .take(per)
                .map(|&r| r.clone())
                .collect();
            taken.sort_by(|a, b| a.excess().total_cmp(&b.excess()).then(a.pair.cmp(&b.pair)));
            let taken = Taken {
                had: v.len(),
                surface: first.len(),
                rows: taken,
            };
            (key, taken)
        })
        .collect()
}

/// What the object draw puts in one cell: its base tile (the map's; `None` off the map) and the
/// tiles drawn over it, in draw order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stack {
    pub base: Option<u16>,
    pub list: Vec<u16>,
}

impl Stack {
    /// The tiles in draw order.
    pub fn tiles(&self) -> impl Iterator<Item = u16> + '_ {
        self.base.into_iter().chain(self.list.iter().copied())
    }

    /// 0x10e596's rule (module docs); `part`: a part of an object added to its cell (mode 1).
    fn add(&mut self, game: &Game, t: u16, part: bool) {
        let f2 = |t: u16| game.flags2.get(usize::from(t)).copied().unwrap_or(0);
        let f3 = game.flags3.get(usize::from(t)).copied().unwrap_or(0);
        if f3 & REPLACES_BASE != 0 {
            self.base = Some(t);
        } else if f2(t) & TOP == 0 {
            self.list.insert(0, t);
        } else if part {
            self.list.push(t);
        } else {
            let at = self
                .list
                .iter()
                .position(|&n| f2(n) & TOP != 0)
                .unwrap_or(self.list.len());
            self.list.insert(at, t);
        }
    }
}

/// The `cols × rows` cells from (x0, y0) of level `z`, wrapping at the level's side, as the object
/// draw stacks them (module docs), row-major.
pub fn stacks(
    game: &Game,
    z: usize,
    (x0, y0): (usize, usize),
    (cols, rows): (usize, usize),
) -> Vec<Stack> {
    let world = &game.world;
    let mut out = vec![Stack::default(); cols * rows];
    let s = world.levels.get(z).map_or(0, |l| l.tiles());
    if s == 0 {
        return out;
    }
    for r in 0..rows {
        for c in 0..cols {
            out[r * cols + c].base = world.tile(z, (x0 + c) % s, (y0 + r) % s);
        }
    }
    // A world coordinate's place from the window's first column or row.
    let rel = |v: i64, from: usize| (v - from as i64).rem_euclid(s as i64) as usize;
    let mut visits = Vec::new();
    for (i, o) in world.objects.iter().enumerate() {
        if usize::from(o.z) != z {
            continue;
        }
        let mut here = Vec::new();
        for (k, (dx, dy, t)) in neighbour::parts(game, o.tile).into_iter().enumerate() {
            let c = rel(i64::from(o.x) + i64::from(dx), x0);
            let r = rel(i64::from(o.y) + i64::from(dy), y0);
            if c < cols && r < rows {
                here.push((r * cols + c, t, k > 0));
            }
        }
        if !here.is_empty() {
            visits.push(((o.y, o.x, Reverse(i)), here));
        }
    }
    visits.sort_by_key(|v| v.0);
    for (_, here) in visits {
        for (cell, t, part) in here {
            out[cell].add(game, t, part);
        }
    }
    out
}

/// The record tile `t` draws at frame 0: counter 0 of its first slot (the first of its frames
/// that the neighbour gate takes), and that frame's own frame 0 in turn.
pub fn frame0(game: &Game, t: u16) -> u16 {
    let n = TILE_COUNT.min(game.records.len());
    let (mut t, mut seen) = (t, Vec::new());
    loop {
        let next = game
            .anim
            .iter()
            .filter(|s| s.tile_to_animate == t)
            .flat_map(|s| (0..=u32::from(s.and_mask)).map(move |c| s.frame_tile(c)))
            .find(|&f| f < 0x200 && usize::from(f) < n);
        match next {
            Some(f) if !seen.contains(&t) => {
                seen.push(t);
                t = f;
            }
            _ => return t,
        }
    }
}

/// Draws record `r` with its top-left corner at (x, y), naming it in the source plane: the
/// pixels where it is opaque, the others left as they are.
fn blit(game: &Game, fb: &mut Framebuffer, r: u16, x: usize, y: usize) {
    let Some(rec) = game.records.get(usize::from(r)) else {
        return;
    };
    for v in 0..TILE_W {
        for u in 0..TILE_W {
            let i = v * TILE_W + u;
            if rec.transparent.get(i).is_some_and(|&t| !t) {
                let src = Src {
                    ver: 0,
                    id: r,
                    u: u as u16,
                    v: v as u16,
                    kind: Kind::Tile,
                    flags: 0,
                };
                fb.set_src(x + u, y + v, rec.pixels[i], src);
            }
        }
    }
}

/// Where a record drawn in a cell shows in the frame ([`framed`]): in the cell; and along each
/// of its sides (north, east, south, west), the pixels of the side's line it is opaque at and
/// those of them left in the plane, a bit per pixel along the line (its row on an east or west
/// line, its column on a north or south line).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Shows {
    cell: bool,
    opaque: [u16; 4],
    left: [u16; 4],
}

const NORTH: usize = 0;
const EAST: usize = 1;
const SOUTH: usize = 2;
const WEST: usize = 3;

/// A window's frame.
pub struct Framed {
    /// The classic frame (the window at its top-left, the game's palette) and its plane.
    pub fb: Framebuffer,
    /// An animated tile was drawn at frame 0.
    pub frame0: bool,
    /// A tile of a cell of `want` was drawn as the record wanted there, not as its frame 0: a
    /// join cell as the pair's record, or, where the world draws an item, A's cell as the record
    /// it draws there.
    pub pair_frame: bool,
    /// A window of an item alone ([`final_read`]): a record drawn as itself where its tile's
    /// frame 0 is another record (one the animation replaces), set by the final read in place
    /// of `pair_frame`.
    pub itself: bool,
    /// The window does not show what it was asked to: a cell of `want` where its record does
    /// not show ([`Framed::shows`]); with a pair, the pair not drawn across its join
    /// ([`Framed::joins`]): the window does not show the pair measured.
    pub missing: bool,
    /// The records that show in each cell (row-major), each once, in draw order: some pixel of
    /// a record drawn there is left by the tiles drawn after it (the source plane names it, and
    /// the panels draw it from that plane); a record that draws nothing at all counts where it
    /// is drawn.
    pub drawn: Vec<Vec<u16>>,
    /// Every record drawn in each cell, in draw order (row-major), covered or not.
    pub stacked: Vec<Vec<u16>>,
    /// Where each record of `stacked` shows, each once, per cell.
    shows: Vec<Vec<(u16, Shows)>>,
}

impl Framed {
    /// `samples.tsv`'s `frames`: `frame0`, `pair`, `itself` and `missing` as they apply, joined
    /// by `+`; `-` for none.
    pub fn frames(&self) -> String {
        let marks = [
            (self.frame0, "frame0"),
            (self.pair_frame, "pair"),
            (self.itself, "itself"),
            (self.missing, "missing"),
        ];
        let on: Vec<&str> = marks.iter().filter(|m| m.0).map(|m| m.1).collect();
        if on.is_empty() {
            "-".into()
        } else {
            on.join("+")
        }
    }

    fn shown(&self, (c, r): (usize, usize), rec: u16) -> Option<Shows> {
        let cell = self.shows.get(r * COLS + c)?;
        cell.iter().find(|s| s.0 == rec).map(|s| s.1)
    }

    /// Whether record `rec` shows in the window's cell `(c, r)` ([`Framed::drawn`]).
    pub fn shows(&self, cell: (usize, usize), rec: u16) -> bool {
        self.shown(cell, rec).is_some_and(|s| s.cell)
    }

    /// Whether pair `p` is drawn across the join of the window's cell `a` and the cell `p.edge`
    /// of it, where the gate measures it: the gate steps across the join only on the rows (or
    /// columns) where both records are opaque on their lines at the join, so on one of those
    /// both pixels must be left in the plane. Where the two lines have no such row (the gate
    /// measures nothing across the join) each record shows when it shows in its cell.
    pub fn joins(&self, (c, r): (usize, usize), p: Pair) -> bool {
        let ((bc, br), own, other) = match p.edge {
            Edge::East => ((c + 1, r), EAST, WEST),
            Edge::South => ((c, r + 1), SOUTH, NORTH),
        };
        if bc >= COLS || br >= ROWS {
            return false;
        }
        let (Some(a), Some(b)) = (self.shown((c, r), p.a), self.shown((bc, br), p.b)) else {
            return false;
        };
        if a.opaque[own] & b.opaque[other] != 0 {
            a.left[own] & b.left[other] != 0
        } else {
            a.cell && b.cell
        }
    }
}

/// Where record `rec`, drawn in the window's cell `(c, r)`, shows in the source plane `plane`
/// ([`Framed::drawn`], [`Framed::joins`]): in the cell, where some pixel of it is left (a record
/// that draws nothing: always); along each side, the pixels of the line it is opaque at and
/// those left.
fn shows_in(game: &Game, plane: &[Src], (c, r): (usize, usize), rec: u16) -> Shows {
    let record = game.records.get(usize::from(rec));
    let opaque = |(u, v): (usize, usize)| {
        record.is_some_and(|x| x.transparent.get(v * TILE_W + u).is_some_and(|&t| !t))
    };
    let named = |(u, v): (usize, usize)| {
        let s = plane[(r * TILE_W + v) * WIDTH + c * TILE_W + u];
        s.kind == Kind::Tile && s.id == rec
    };
    // Whether some pixel of the record among `points` is left; `None` when it has none there.
    let left = |points: &mut dyn Iterator<Item = (usize, usize)>| -> Option<bool> {
        let mut has = false;
        for p in points {
            if opaque(p) {
                if named(p) {
                    return Some(true);
                }
                has = true;
            }
        }
        has.then_some(false)
    };
    let cell = left(&mut (0..TILE_W * TILE_W).map(|i| (i % TILE_W, i / TILE_W))).unwrap_or(true);
    let last = TILE_W - 1;
    let (mut opaque_on, mut left_on) = ([0u16; 4], [0u16; 4]);
    for side in [NORTH, EAST, SOUTH, WEST] {
        for k in 0..TILE_W {
            let p = match side {
                NORTH => (k, 0),
                EAST => (last, k),
                SOUTH => (k, last),
                _ => (0, k),
            };
            if opaque(p) {
                opaque_on[side] |= 1 << k;
                if named(p) {
                    left_on[side] |= 1 << k;
                }
            }
        }
    }
    Shows {
        cell,
        opaque: opaque_on,
        left: left_on,
    }
}

/// The window's first cell (column, row of the level) for A's cell `at`.
fn origin(game: &Game, at: At) -> Option<(usize, usize)> {
    let s = game.world.levels.get(at.z)?.tiles();
    (s > 0).then(|| {
        let back = |v: usize, by: usize| (v as i64 - by as i64).rem_euclid(s as i64) as usize;
        (back(at.x, A_CELL.0), back(at.y, A_CELL.1))
    })
}

/// B's cell in the window.
fn b_cell(edge: Edge) -> (usize, usize) {
    match edge {
        Edge::East => (A_CELL.0 + 1, A_CELL.1),
        Edge::South => (A_CELL.0, A_CELL.1 + 1),
    }
}

/// The window's stacks (none off the levels).
fn window(game: &Game, at: At) -> Vec<Stack> {
    match origin(game, at) {
        Some(o) => stacks(game, at.z, o, (COLS, ROWS)),
        None => vec![Stack::default(); COLS * ROWS],
    }
}

/// The frame of the window of A's cell `at` (module docs): the window's cells drawn at the
/// frame's top-left, base first, then their lists; `pair`'s records in its two cells.
/// `missing` too when the pair is not drawn across its join ([`Framed::joins`]).
pub fn frame(game: &Game, at: At, edge: Edge, pair: Option<Pair>) -> Framed {
    let want: Vec<((usize, usize), u16)> = pair
        .map(|p| vec![(A_CELL, p.a), (b_cell(edge), p.b)])
        .unwrap_or_default();
    let mut f = framed(game, &window(game, at), &want);
    if let Some(p) = pair {
        f.missing |= !f.joins(A_CELL, Pair { edge, ..p });
    }
    f
}

/// The frame of the window's `cells` ([`COLS`] × [`ROWS`], row-major): each drawn at the
/// frame's top-left, base first, then its list; in each cell of `want` (column, row) a tile
/// whose frames include its record, or that is the record, drawn as that record (a record the
/// animation always replaces is drawn so only where it is wanted: an item alone), every other
/// tile at frame 0.
/// `missing` when the record of a cell of `want` does not show there: no tile draws it, or the
/// tiles drawn after it cover it ([`Framed::shows`]).
pub fn framed(game: &Game, cells: &[Stack], want: &[((usize, usize), u16)]) -> Framed {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    for (i, c) in game.palette.iter().take(256).enumerate() {
        fb.palette[i] = Rgb::new(c[0], c[1], c[2]);
    }
    let mut out = Framed {
        fb,
        frame0: false,
        pair_frame: false,
        itself: false,
        missing: false,
        drawn: vec![Vec::new(); COLS * ROWS],
        stacked: vec![Vec::new(); COLS * ROWS],
        shows: vec![Vec::new(); COLS * ROWS],
    };
    let mut drawn = Drawn::of(game);
    for r in 0..ROWS {
        for c in 0..COLS {
            let wanted = want.iter().position(|w| w.0 == (c, r));
            let Some(cell) = cells.get(r * COLS + c) else {
                continue;
            };
            for t in cell.tiles() {
                let frames = drawn.records(t);
                let first = frame0(game, t);
                let rec = match wanted {
                    Some(k) if frames.contains(&want[k].1) || t == want[k].1 => {
                        out.pair_frame |= want[k].1 != first;
                        want[k].1
                    }
                    _ => {
                        out.frame0 |= frames != [t];
                        first
                    }
                };
                blit(game, &mut out.fb, rec, c * TILE_W, r * TILE_W);
                out.stacked[r * COLS + c].push(rec);
            }
        }
    }
    // What is left of each record once every tile is drawn: the source plane names it.
    if let Some(plane) = out.fb.src() {
        for r in 0..ROWS {
            for c in 0..COLS {
                let k = r * COLS + c;
                let mut seen: Vec<(u16, Shows)> = Vec::new();
                for &rec in &out.stacked[k] {
                    if !seen.iter().any(|s| s.0 == rec) {
                        seen.push((rec, shows_in(game, plane, (c, r), rec)));
                    }
                }
                out.drawn[k] = seen.iter().filter(|s| s.1.cell).map(|s| s.0).collect();
                out.shows[k] = seen;
            }
        }
    }
    out.missing = want.iter().any(|&(cell, rec)| !out.shows(cell, rec));
    out
}

/// The game's tile records as the compositor asks for them.
pub struct Records<'a>(pub &'a Game);

impl Images for Records<'_> {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        let r = self
            .0
            .records
            .get(usize::from(id))
            .filter(|_| kind == Kind::Tile)?;
        Some(Image {
            ver: 0,
            width: TILE_W,
            height: TILE_W,
            pixels: &r.pixels,
            transparent: Some(&r.transparent),
        })
    }

    fn key(&mut self, kind: Kind, id: u16) -> Option<u64> {
        (kind == Kind::Tile)
            .then(|| self.0.hash(ImageId::Tile(id)))
            .flatten()
    }
}

/// The masters of `dir` as ingest writes them to `slices/` (×8: `t-<record>.png`,
/// `p-<letter>-<item>.png`, `b-<role>.png`), by image; any other file is not read.
pub fn masters(dir: &Path) -> Result<Vec<(ImageId, PathBuf)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for e in entries {
        let path = e.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let Some(stem) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".png"))
        else {
            continue;
        };
        let Some(id) = atlas::parse_image(&stem.replace('-', ":")) else {
            continue;
        };
        if atlas::slice_name(id) != stem || !path.is_file() {
            continue;
        }
        out.push((id, path));
    }
    out.sort();
    Ok(out)
}

/// A pack of tile images at the pack's ×4, by record, each with the content key of the record it
/// was made for.
pub struct Slices {
    images: BTreeMap<u16, (u64, Rgba)>,
    /// The masters read ([`Slices::read`]), by image: the digest of each file's bytes.
    read: BTreeMap<ImageId, String>,
}

impl Slices {
    /// The ×4 images given, keyed by the game's records (a record the game has not is left out).
    pub fn new(game: &Game, images: BTreeMap<u16, Rgba>) -> Slices {
        let images = images
            .into_iter()
            .filter_map(|(r, img)| Some((r, (game.hash(ImageId::Tile(r))?, img))))
            .collect();
        Slices {
            images,
            read: BTreeMap::new(),
        }
    }

    /// The masters of `dir` ([`masters`]), each file read once, into memory, its digest kept
    /// ([`Slices::digests`]); a tile master's image box-averaged to ×4 as `pack` writes it (a
    /// portrait's or a bitmap's is not decoded: nothing is drawn from it).
    pub fn read(game: &Game, dir: &Path) -> Result<Slices, String> {
        let side = TILE_W as u32 * MASTER;
        let (mut images, mut read) = (BTreeMap::new(), BTreeMap::new());
        for (id, path) in masters(dir)? {
            let bytes = img::read_capped(&path, img::PNG_MAX_BYTES)?;
            read.insert(id, work::digest(&[&bytes]));
            if let ImageId::Tile(r) = id {
                let master = img::decode_png(&bytes, side, side)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                images.insert(r, img::shrink(&master, MASTER / SCALE));
            }
        }
        Ok(Slices {
            read,
            ..Slices::new(game, images)
        })
    }

    /// Every master [`Slices::read`] read, by image (one the game has not too), with the digest
    /// of its file's bytes as read.
    pub fn digests(&self) -> &BTreeMap<ImageId, String> {
        &self.read
    }

    /// The masters of `dir` that are not the ones read: read again, each image added, gone or
    /// whose file's bytes now have another digest.
    pub fn changed(&self, dir: &Path) -> Result<Vec<ImageId>, String> {
        let mut now = BTreeMap::new();
        for (id, path) in masters(dir)? {
            let bytes = img::read_capped(&path, img::PNG_MAX_BYTES)?;
            now.insert(id, work::digest(&[&bytes]));
        }
        let ids: BTreeSet<ImageId> = now.keys().chain(self.read.keys()).copied().collect();
        Ok(ids
            .into_iter()
            .filter(|id| now.get(id) != self.read.get(id))
            .collect())
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Record `r`'s ×4 image.
    pub fn get(&self, r: u16) -> Option<&Rgba> {
        self.images.get(&r).map(|(_, img)| img)
    }
}

impl Pack for Slices {
    fn hd(&self, kind: Kind, id: u16, key: u64) -> Option<PackHd<'_>> {
        let (k, img) = self.images.get(&id).filter(|_| kind == Kind::Tile)?;
        (*k == key).then_some(PackHd {
            index: u32::from(id),
            width: img.width as usize,
            height: img.height as usize,
            rgba: &img.pixels,
        })
    }
}

/// A window's panels at one scale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panels {
    /// The classic frame, nearest.
    pub classic: Rgba,
    /// HD on, no pack: MMPX of every record.
    pub plain: Rgba,
    /// HD on with the pack of the slices.
    pub hd: Rgba,
}

/// The window's size in classic pixels.
pub const fn window_px() -> (u32, u32) {
    ((COLS * TILE_W) as u32, (ROWS * TILE_W) as u32)
}

/// The window's part (the frame's top-left) of a composition of `stride` words a row.
fn crop(words: &[u32], stride: usize, w: u32, h: u32) -> Rgba {
    let mut out = Rgba::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = (y * w as usize + x) * 4;
            out.pixels[i..i + 4].copy_from_slice(&words[y * stride + x].to_le_bytes());
        }
    }
    out
}

/// The three panels of `fb` at `scale` (module docs), each by a compositor of its own.
pub fn panels(game: &Game, fb: &Framebuffer, pack: &Rc<Slices>, scale: Scale) -> Panels {
    let s = scale.factor() as u32;
    let (w, h) = window_px();
    let mut classic = Rgba::new(w * s, h * s);
    let px = fb.pixels();
    for y in 0..h * s {
        for x in 0..w * s {
            let c = fb.palette[usize::from(px[(y / s) as usize * WIDTH + (x / s) as usize])];
            img::put(&mut classic, x, y, [c.r, c.g, c.b, 255]);
        }
    }
    let composed = |pack: Option<Rc<dyn Pack>>| {
        let mut c = Compositor::new();
        c.set_pack(pack);
        c.compose(fb, &mut Records(game), scale);
        crop(c.rgba(), WIDTH * s as usize, w * s, h * s)
    };
    let pack: Rc<dyn Pack> = pack.clone();
    Panels {
        classic,
        plain: composed(None),
        hd: composed(Some(pack)),
    }
}

/// A 3 × 5 font: a row per byte, the left pixel the high bit of three.
const GLYPHS: [(char, [u8; 5]); 49] = [
    ('A', [2, 5, 7, 5, 5]),
    ('B', [6, 5, 6, 5, 6]),
    ('C', [3, 4, 4, 4, 3]),
    ('D', [6, 5, 5, 5, 6]),
    ('E', [7, 4, 6, 4, 7]),
    ('F', [7, 4, 6, 4, 4]),
    ('G', [3, 4, 5, 5, 3]),
    ('H', [5, 5, 7, 5, 5]),
    ('I', [7, 2, 2, 2, 7]),
    ('J', [1, 1, 1, 5, 2]),
    ('K', [5, 5, 6, 5, 5]),
    ('L', [4, 4, 4, 4, 7]),
    ('M', [7, 7, 5, 5, 5]),
    ('N', [5, 7, 7, 5, 5]),
    ('O', [2, 5, 5, 5, 2]),
    ('P', [6, 5, 6, 4, 4]),
    ('Q', [2, 5, 5, 6, 3]),
    ('R', [6, 5, 6, 5, 5]),
    ('S', [3, 4, 2, 1, 6]),
    ('T', [7, 2, 2, 2, 2]),
    ('U', [5, 5, 5, 5, 7]),
    ('V', [5, 5, 5, 5, 2]),
    ('W', [5, 5, 7, 7, 5]),
    ('X', [5, 5, 2, 5, 5]),
    ('Y', [5, 5, 2, 2, 2]),
    ('Z', [7, 1, 2, 4, 7]),
    ('0', [7, 5, 5, 5, 7]),
    ('1', [2, 6, 2, 2, 7]),
    ('2', [6, 1, 2, 4, 7]),
    ('3', [6, 1, 2, 1, 6]),
    ('4', [5, 5, 7, 1, 1]),
    ('5', [7, 4, 6, 1, 6]),
    ('6', [3, 4, 7, 5, 7]),
    ('7', [7, 1, 2, 2, 2]),
    ('8', [7, 5, 7, 5, 7]),
    ('9', [7, 5, 7, 1, 6]),
    ('.', [0, 0, 0, 0, 2]),
    ('/', [1, 1, 2, 4, 4]),
    ('-', [0, 0, 7, 0, 0]),
    ('>', [4, 2, 1, 2, 4]),
    ('<', [1, 2, 4, 2, 1]),
    ('=', [0, 7, 0, 7, 0]),
    (',', [0, 0, 0, 2, 4]),
    (':', [0, 2, 0, 2, 0]),
    ('#', [5, 7, 5, 7, 5]),
    ('+', [0, 2, 7, 2, 0]),
    ('(', [2, 4, 4, 4, 2]),
    (')', [2, 1, 1, 1, 2]),
    ('?', [6, 1, 2, 0, 2]),
];

/// Writes `s` (upper case) at (x, y), each font pixel `t × t`, clipped to `max_w` pixels.
fn text(out: &mut Rgba, (x, y): (u32, u32), s: &str, t: u32, max_w: u32) {
    for (i, ch) in s.chars().enumerate() {
        let ch = ch.to_ascii_uppercase();
        let x0 = x + i as u32 * 4 * t;
        if x0 + 3 * t > x + max_w {
            break;
        }
        if ch == ' ' {
            continue;
        }
        let rows = GLYPHS
            .iter()
            .find(|g| g.0 == ch)
            .map_or([6, 1, 2, 0, 2], |g| g.1);
        for (r, bits) in rows.iter().enumerate() {
            for c in 0..3 {
                if bits & (4 >> c) != 0 {
                    img::fill(out, x0 + c * t, y + r as u32 * t, t, t, INK);
                }
            }
        }
    }
}

/// One row of a sheet: a sample's caption, the edge it marks and its panels.
pub struct Line {
    pub caption: String,
    pub edge: Edge,
    pub panels: Panels,
}

/// Where everything of a sheet at scale `s` goes.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub s: u32,
    /// Font pixel size.
    pub text: u32,
    /// The margins around the panels, where the marks are.
    pub margin: u32,
    /// A panel's size.
    pub w: u32,
    pub h: u32,
}

impl Layout {
    pub fn of(s: u32) -> Layout {
        let (w, h) = window_px();
        Layout {
            s,
            text: s / 2 + 1,
            margin: 4 * s,
            w: w * s,
            h: h * s,
        }
    }

    fn line_h(&self) -> u32 {
        6 * self.text
    }

    fn caption_h(&self) -> u32 {
        2 * self.line_h() + self.margin / 2
    }

    pub fn row_h(&self) -> u32 {
        self.caption_h() + self.h + 2 * self.margin
    }

    pub fn width(&self) -> u32 {
        3 * self.w + 4 * self.margin
    }

    pub fn height(&self, rows: usize) -> u32 {
        self.margin / 2 + rows as u32 * self.row_h()
    }

    /// The top-left corner of panel `k` (0 classic, 1 no pack, 2 pack) of row `i`.
    pub fn panel_at(&self, i: usize, k: usize) -> (u32, u32) {
        (
            self.margin + k as u32 * (self.w + self.margin),
            self.margin / 2 + i as u32 * self.row_h() + self.caption_h() + self.margin,
        )
    }

    /// Rows a sheet holds under [`MAX_HEIGHT`] (at least one).
    pub fn rows_max(&self) -> usize {
        (((MAX_HEIGHT - self.margin / 2) / self.row_h()) as usize).max(1)
    }
}

/// The join marks of a panel at `(px, py)`, in its margins: across the join, a tick above the
/// panel (east: on the two columns that meet) or left of it (south: on the two rows); along the
/// edge, a bar left of the panel (east: the edge's rows) or above it (south: its columns).
fn ticks(out: &mut Rgba, l: &Layout, (px, py): (u32, u32), edge: Edge) {
    mark(out, l, (px, py), (A_CELL, edge), MARK);
}

/// [`ticks`] for the join of the cell `(c, r)` of the window and the one `edge` of it, in
/// `colour`; [`MARK_HIDDEN`] dashed (every other pixel, a checkerboard).
fn mark(
    out: &mut Rgba,
    l: &Layout,
    (px, py): (u32, u32),
    ((c, r), edge): ((usize, usize), Edge),
    colour: [u8; 4],
) {
    let cell = TILE_W as u32 * l.s;
    let m = l.margin;
    let (ax, ay) = (c as u32 * cell, r as u32 * cell);
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32| {
        for y in y0..(y0 + h).min(out.height) {
            for x in x0..(x0 + w).min(out.width) {
                if colour != MARK_HIDDEN || (x + y) % 2 == 0 {
                    img::put(out, x, y, colour);
                }
            }
        }
    };
    match edge {
        Edge::East => {
            fill(px + ax + cell - 1, py - m + 2, 2, m - 4);
            fill(px - 5, py + ay, 3, cell);
        }
        Edge::South => {
            fill(px - m + 2, py + ay + cell - 1, m - 4, 2);
            fill(px + ax, py - 5, cell, 3);
        }
    }
}

/// The panel captions under the sample's.
pub const PANEL_NAMES: [&str; 3] = ["classic", "no pack", "hd pack"];

/// A sheet of `lines` at scale `s` (module docs).
pub fn sheet(lines: &[Line], s: u32) -> Rgba {
    let l = Layout::of(s);
    let (w, h) = (l.width(), l.height(lines.len()));
    let mut out = Rgba::new(w, h);
    img::fill(&mut out, 0, 0, w, h, BACK);
    for (i, line) in lines.iter().enumerate() {
        let p = &line.panels;
        for (k, panel) in [&p.classic, &p.plain, &p.hd].into_iter().enumerate() {
            let (px, py) = l.panel_at(i, k);
            let cy = py - l.margin - l.caption_h();
            text(&mut out, (px, cy), &line.caption, l.text, l.w);
            text(&mut out, (px, cy + l.line_h()), PANEL_NAMES[k], l.text, l.w);
            out.paste(panel, px, py);
            ticks(&mut out, &l, (px, py), line.edge);
        }
    }
    out
}

/// Where `--at` puts a control window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// A's cell and the edge.
    Cell { at: At, edge: Edge },
    /// The party leader's start cell, and the failing gen-mmpx pairs near it.
    Leader { edge: Edge },
}

impl Spot {
    /// `z,x,y[,edge]` or `leader[,edge]`; the edge `east` (the default) or `south`.
    pub fn parse(s: &str) -> Result<Spot, String> {
        let bad = || format!("bad --at {s:?} (z,x,y[,east|south] or leader[,east|south])");
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let edge = |p: Option<&&str>| p.map_or(Some(Edge::East), |e| parse_edge(e));
        if parts[0] == "leader" {
            return match parts.len() {
                1 | 2 => Ok(Spot::Leader {
                    edge: edge(parts.get(1)).ok_or_else(bad)?,
                }),
                _ => Err(bad()),
            };
        }
        if !(3..=4).contains(&parts.len()) {
            return Err(bad());
        }
        let n = |i: usize| parts[i].parse::<usize>().map_err(|_| bad());
        Ok(Spot::Cell {
            at: At {
                z: n(0)?,
                x: n(1)?,
                y: n(2)?,
            },
            edge: edge(parts.get(3)).ok_or_else(bad)?,
        })
    }
}

/// Every record drawn in a cell: each frame of each tile stacked there.
fn records_at(stack: &Stack, drawn: &mut Drawn) -> BTreeSet<u16> {
    stack.tiles().flat_map(|t| drawn.records(t)).collect()
}

/// Of the rows of a pair of records from `a` and `b` across `edge`, a failing one first, then
/// the larger ratio / limit, then the first in pair order.
fn measured<'a>(
    by: &BTreeMap<Pair, &'a Row>,
    a: &BTreeSet<u16>,
    b: &BTreeSet<u16>,
    edge: Edge,
) -> Option<&'a Row> {
    let mut best: Option<&Row> = None;
    for &ra in a {
        for &rb in b {
            let Some(&r) = by.get(&Pair { a: ra, b: rb, edge }) else {
                continue;
            };
            let better = best.is_none_or(|o| {
                (r.fail, o.fail) == (true, false) || (r.fail == o.fail && r.excess() > o.excess())
            });
            if better {
                best = Some(r);
            }
        }
    }
    best
}

/// A control window: A's cell, the edge and the row of the pair shown across it, if measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub at: At,
    pub edge: Edge,
    pub row: Option<Row>,
}

/// The control of `at`'s window: the pair drawn across its edge with a row.
fn control_at(game: &Game, by: &BTreeMap<Pair, &Row>, at: At, edge: Edge) -> Control {
    let cells = window(game, at);
    let mut drawn = Drawn::of(game);
    let cell = |(c, r): (usize, usize)| &cells[r * COLS + c];
    let a = records_at(cell(A_CELL), &mut drawn);
    let b = records_at(cell(b_cell(edge)), &mut drawn);
    Control {
        at,
        edge,
        row: measured(by, &a, &b, edge).cloned(),
    }
}

/// The failing gen-mmpx pairs drawn within [`NEAR`] cells of `centre` on its level (A's cell
/// that near), each once at its nearest cell (by the larger of the two distances, then cell
/// and pair order), at most `n`.
fn near(game: &Game, by: &BTreeMap<Pair, &Row>, centre: At, n: usize) -> Vec<Control> {
    let s = game.world.levels.get(centre.z).map_or(0, |l| l.tiles());
    if s == 0 {
        return Vec::new();
    }
    let back = |v: usize| (v as i64 - NEAR as i64).rem_euclid(s as i64) as usize;
    let (x0, y0) = (back(centre.x), back(centre.y));
    let side = 2 * NEAR + 2;
    let cells = stacks(game, centre.z, (x0, y0), (side, side));
    let mut drawn = Drawn::of(game);
    let recs: Vec<BTreeSet<u16>> = cells.iter().map(|c| records_at(c, &mut drawn)).collect();
    let mut found: BTreeMap<Pair, (usize, At)> = BTreeMap::new();
    for r in 0..=2 * NEAR {
        for c in 0..=2 * NEAR {
            let dist = c.abs_diff(NEAR).max(r.abs_diff(NEAR));
            let at = At {
                z: centre.z,
                x: (x0 + c) % s,
                y: (y0 + r) % s,
            };
            for (edge, (bc, br)) in [(Edge::East, (c + 1, r)), (Edge::South, (c, r + 1))] {
                for &ra in &recs[r * side + c] {
                    for &rb in &recs[br * side + bc] {
                        let p = Pair { a: ra, b: rb, edge };
                        let Some(row) = by.get(&p) else { continue };
                        if !row.fail || row.sides != Sides::GenMmpx {
                            continue;
                        }
                        let e = found.entry(p).or_insert((dist, at));
                        *e = (*e).min((dist, at));
                    }
                }
            }
        }
    }
    let mut list: Vec<(usize, At, Pair)> = found.into_iter().map(|(p, (d, a))| (d, a, p)).collect();
    list.sort();
    list.into_iter()
        .take(n)
        .map(|(_, at, p)| Control {
            at,
            edge: p.edge,
            row: by.get(&p).map(|&r| r.clone()),
        })
        .collect()
}

/// The ratio of `p` measured again from the images of `slices` ([`measure`]); `None` when
/// nothing is measured or a side is refused.
pub fn remeasured(game: &Game, slices: &Slices, p: Pair, seam: f32) -> Option<f32> {
    measure(game, slices, p, seam).ok().flatten().map(|r| r.0)
}

/// The (ratio, limit) of `p` measured from the images of `slices` (a record without one is
/// MMPX), by the gate itself: each record's [`Side`], then one pass of [`neighbour::settle`]
/// over the pair alone. `Ok(None)` when nothing is measured (no side generated, a side draws
/// nothing along the edge, a record the game has no image of); `Err` when a side is refused
/// (an image not ×4 of its record).
pub fn measure(
    game: &Game,
    slices: &Slices,
    p: Pair,
    seam: f32,
) -> Result<Option<(f32, f32)>, String> {
    let mut sides = BTreeMap::new();
    for r in [p.a, p.b] {
        let Some(classic) = game.classic(ImageId::Tile(r)) else {
            continue;
        };
        let side = Side::new(classic, slices.get(r).map(|h| (u32::from(r), h)))?;
        sides.insert(r, side);
    }
    let settled = neighbour::settle(&[p], &sides, seam);
    Ok(settled
        .checked
        .first()
        .filter(|c| c.pass == 1)
        .and_then(|c| c.result))
}

/// What `calibrate` is given.
#[derive(Clone, Debug)]
pub struct Options {
    /// The masters to show (`t-<record>.png`, ×8).
    pub slices: PathBuf,
    pub out: PathBuf,
    pub per: usize,
    pub seed: u64,
    pub at: Vec<Spot>,
    /// The seam threshold the ratios are measured again with.
    pub seam: f32,
    /// The rows sampled.
    pub source: Source,
}

/// What `calibrate` wrote, in counts and file names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// The rows sampled.
    pub source: Source,
    /// First-pass pairs measured with a cell, and those skipped.
    pub rows: usize,
    pub skipped: usize,
    pub slices: usize,
    /// Per bucket × sides: rows it had, of the surface map, taken.
    pub cells: Vec<(Bucket, Sides, usize, usize, usize)>,
    /// Control windows, and whether `--at leader` found the leader (`None`: not asked).
    pub controls: usize,
    pub leader: Option<bool>,
    /// Sampled rows whose ratio measured again from the slices is the row's, of those measured.
    pub same_ratio: usize,
    pub remeasured: usize,
    /// Lines of `samples.tsv` (header not counted) and the sheets written.
    pub tsv_rows: usize,
    pub sheets: Vec<String>,
    /// What to say of the out dir ([`out_dir::run_in`]).
    pub out_dir: Vec<String>,
}

impl Summary {
    /// The report: counts and file names only.
    pub fn text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "calibrate (counts only): {} first-pass pairs measured, {} skipped (nothing \
             measured); {} slices",
            self.rows, self.skipped, self.slices
        );
        if self.source != Source::All {
            let _ = writeln!(
                s,
                "source {}: the rows it found, the surface first after one of each other level",
                self.source.name()
            );
        }
        let first = match self.source {
            Source::All => "of the surface map",
            _ => "on the surface",
        };
        for (b, sides, had, surface, taken) in &self.cells {
            let _ = writeln!(
                s,
                "{}{}-{}: {taken} of {had} ({surface} {first})",
                self.source.prefix(),
                b.name(),
                sides.name()
            );
        }
        let leader = match self.leader {
            None => "",
            Some(true) => " (leader's start cell found)",
            Some(false) => " (the files place no leader)",
        };
        let _ = writeln!(s, "control windows: {}{leader}", self.controls);
        let _ = writeln!(
            s,
            "ratio measured again from the slices equal to the row's: {} of {}",
            self.same_ratio, self.remeasured
        );
        let _ = writeln!(s, "samples.tsv: {} rows", self.tsv_rows);
        let _ = writeln!(s, "sheets: {}", self.sheets.join(" "));
        for n in &self.out_dir {
            let _ = writeln!(s, "out dir: {n}");
        }
        s
    }
}

/// `samples.tsv`'s header.
pub const TSV_HEADER: &str = "index\tbucket\tsides\ta\tb\tedge\tratio\tlimit\tratio_slices\t\
                              source\tz\tx\ty\tsheet\tsheet_s2\tpanel\tframes";

/// One window to draw: its caption fields and what the TSV says of it.
struct Entry {
    bucket: &'static str,
    label: &'static str,
    at: At,
    edge: Edge,
    row: Option<Row>,
}

/// Draws `entries` as the sheets `<stem>` (split in parts under [`MAX_HEIGHT`]), numbering them
/// from `*index`; appends their lines to `tsv` and the sheets' names to `sheets`.
#[allow(clippy::too_many_arguments)]
fn draw(
    game: &Game,
    pack: &Rc<Slices>,
    seam: f32,
    out: &Path,
    stem: &str,
    entries: &[Entry],
    index: &mut usize,
    tsv: &mut String,
    sheets: &mut Vec<String>,
    same: &mut (usize, usize),
) -> Result<(), String> {
    let per_part = Layout::of(4).rows_max();
    let parts = entries.len().div_ceil(per_part);
    for (k, part) in entries.chunks(per_part).enumerate() {
        let name = if parts == 1 {
            stem.to_string()
        } else {
            format!("{stem}-p{}", k + 1)
        };
        let (s4, s2) = (format!("{name}.png"), format!("{name}-s2.png"));
        let (mut l4, mut l2) = (Vec::new(), Vec::new());
        for (i, e) in part.iter().enumerate() {
            *index += 1;
            let pair = e.row.as_ref().map(|r| r.pair);
            let framed = frame(game, e.at, e.edge, pair);
            let ratio = |r: &Row| format!("{:.2}/{:.2}", r.ratio, r.limit);
            let caption = match &e.row {
                Some(r) => format!("{}: {} {} {}", index, e.label, r.sides.name(), ratio(r)),
                None => format!("{}: {} not measured", index, e.label),
            };
            for (s, lines) in [(Scale::X4, &mut l4), (Scale::X2, &mut l2)] {
                lines.push(Line {
                    caption: caption.clone(),
                    edge: e.edge,
                    panels: panels(game, &framed.fb, pack, s),
                });
            }
            let again = pair.and_then(|p| remeasured(game, pack, p, seam));
            if let (Some(r), Some(a)) = (&e.row, again) {
                same.1 += 1;
                if a == r.ratio {
                    same.0 += 1;
                }
            }
            let dash = || "-".to_string();
            let f4 = |v: f32| format!("{v:.4}");
            let _ = writeln!(
                tsv,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                index,
                e.bucket,
                e.row.as_ref().map_or("-", |r| r.sides.name()),
                pair.map_or_else(dash, |p| atlas::image_name(ImageId::Tile(p.a))),
                pair.map_or_else(dash, |p| atlas::image_name(ImageId::Tile(p.b))),
                edge_name(e.edge),
                e.row.as_ref().map_or_else(dash, |r| f4(r.ratio)),
                e.row.as_ref().map_or_else(dash, |r| f4(r.limit)),
                again.map_or_else(dash, f4),
                e.row.as_ref().map_or("-", |r| r.source.as_str()),
                e.at.z,
                e.at.x,
                e.at.y,
                s4,
                s2,
                i + 1,
                framed.frames(),
            );
        }
        img::write_png(&out.join(&s4), &sheet(&l4, 4))?;
        img::write_png(&out.join(&s2), &sheet(&l2, 2))?;
        sheets.push(s4);
        sheets.push(s2);
    }
    Ok(())
}

/// Runs `calibrate` (module docs) over the work dir's `gates.jsonl`, written to a fresh
/// directory beside the out dir and swapped in whole at the end ([`out_dir::run_in`]).
pub fn calibrate(game: &Game, work: &Work, o: &Options) -> Result<Summary, String> {
    if o.per == 0 {
        return Err("--per is at least 1".into());
    }
    let (mut s, notes) = out_dir::run_in(out_dir::FIRST, &o.out, |out, _| {
        first_pass(game, work, o, out)
    })?;
    s.out_dir = notes;
    Ok(s)
}

/// [`calibrate`]'s run, written to `out`.
fn first_pass(game: &Game, work: &Work, o: &Options, out: &Path) -> Result<Summary, String> {
    // The snapshot first; only it is read.
    let snapshot = out.join("gates.jsonl");
    let bytes = img::read_capped(&work.path("gates.jsonl"), 1 << 30)
        .map_err(|e| format!("{e} (run ingest first)"))?;
    work::write_file(&snapshot, &bytes)?;
    let text = img::read_capped(&snapshot, 1 << 30)?;
    let text = String::from_utf8(text).map_err(|_| "gates.jsonl is not UTF-8".to_string())?;
    let (rows, skipped) = rows(&text)?;
    let pack = Rc::new(Slices::read(game, &o.slices)?);
    let mut summary = Summary {
        source: o.source.clone(),
        rows: rows.len(),
        skipped,
        slices: pack.len(),
        ..Summary::default()
    };
    let (mut tsv, mut index, mut same) = (format!("{TSV_HEADER}\n"), 0, (0, 0));
    let mut sheets = Vec::new();
    for ((bucket, sides), taken) in sample(&rows, o.per, o.seed, o.source.clone()) {
        summary
            .cells
            .push((bucket, sides, taken.had, taken.surface, taken.rows.len()));
        let entries: Vec<Entry> = taken
            .rows
            .into_iter()
            .map(|r| Entry {
                bucket: bucket.name(),
                label: bucket.label(),
                at: r.at,
                edge: r.pair.edge,
                row: Some(r),
            })
            .collect();
        let stem = format!("{}{}-{}", o.source.prefix(), bucket.name(), sides.name());
        let (t, s) = (&mut tsv, &mut sheets);
        draw(
            game, &pack, o.seam, out, &stem, &entries, &mut index, t, s, &mut same,
        )?;
    }
    let by: BTreeMap<Pair, &Row> = rows.iter().map(|r| (r.pair, r)).collect();
    let mut controls = Vec::new();
    for spot in &o.at {
        match *spot {
            Spot::Cell { at, edge } => controls.push(control_at(game, &by, at, edge)),
            Spot::Leader { edge } => {
                let Some(p) = game.leader else {
                    summary.leader = Some(false);
                    continue;
                };
                summary.leader = Some(true);
                let at = At {
                    z: usize::from(p.z),
                    x: usize::from(p.x),
                    y: usize::from(p.y),
                };
                controls.push(control_at(game, &by, at, edge));
                controls.extend(near(game, &by, at, o.per));
            }
        }
    }
    summary.controls = controls.len();
    if !controls.is_empty() {
        let stem = format!("{}control", o.source.prefix());
        let entries: Vec<Entry> = controls
            .into_iter()
            .map(|c| Entry {
                bucket: "control",
                label: "control",
                at: c.at,
                edge: c.edge,
                row: c.row,
            })
            .collect();
        let (t, s) = (&mut tsv, &mut sheets);
        draw(
            game, &pack, o.seam, out, &stem, &entries, &mut index, t, s, &mut same,
        )?;
    }
    work::write_file(&out.join("samples.tsv"), tsv.as_bytes())?;
    summary.tsv_rows = index;
    summary.sheets = sheets;
    (summary.same_ratio, summary.remeasured) = same;
    Ok(summary)
}
