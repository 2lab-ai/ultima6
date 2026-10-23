//! The pointer (mouse) and its sprite: the sample of the handler's pointer the event wait and
//! the idle cursor take (0x14f12c), the shape for the sampled position (0x1051a6, 0x134eae) and
//! the hardware sprite the vertical-blank server 0x162294 places (0x1622d6). The clicks are in
//! [`super::click`].
//!
//! The input.device handler 0x151b08 is the engine's queue: [`crate::engine::Io::take_pointer`]
//! hands the event wait one button change per sample, so every press and every release is seen
//! by the click map once. The sprite is an overlay of the display, never part of the game's
//! screen buffers: the engine draws [`vblank`]'s frame over the published frame after each tick
//! (the Amiga moves the sprite in every vertical blank, whatever the game is doing).

use u6_formats::FormatError;
use u6_render::{Framebuffer, HEIGHT, WIDTH};

use crate::engine::Pointer;
use crate::geom::{self, HALF_H, HALF_W, MAP_X, MAP_Y};
use crate::image::GameImage;
use crate::state::State;

/// Original: A4-0x79c2 (click regions, 4 words each: x0, y0, x1, y1 inclusive; 0x10658e scans
/// them in order: the map window, the command bar, the side panel, the whole screen)
const REGIONS: i32 = -0x79c2;
/// Regions 0x10658e has a case for (the last one covers the screen, so the scan ends there).
const REGION_COUNT: usize = 4;
/// Spec 14 Design 5: the click regions of the side panel's view slots, one per slot of
/// [`geom::SLOTS`] in its order, numbered after the image's (none in Original). The click map
/// scans them after the side panel's ([`super::click::region_at`]).
pub const SLOT_REGIONS: std::ops::Range<usize> = REGION_COUNT..REGION_COUNT + geom::SLOTS.len();
/// Original: A4-0x79c6 / -0x79c4 (words: window cell of the last walk click)
const WALK_CELL: i32 = -0x79c6;
/// Original: 0x1624b2 (long per shape: its sprite data, two control words then two per row)
const SHAPE_DATA: u32 = 0x1624b2;
/// Original: 0x1624da (word per shape: the height in the high byte, added to the start line)
const SHAPE_HEIGHT: u32 = 0x1624da;
/// Original: 0x1624ee / 0x162502 (words per shape: x and y of the top left from the hot spot)
const SHAPE_DX: u32 = 0x1624ee;
const SHAPE_DY: u32 = 0x162502;
/// Original: 0x1624ae (word: the pointer is shown; 0x1622d6 parks a hidden one below the screen)
const SHOWN: u32 = 0x1624ae;
/// Shapes 0x1051a6 picks: 0 the arrow, 1 the cross-hair, 2..=9 the walk arrows (north, clockwise).
const SHAPES: usize = 10;
/// Palette entry before the sprite colours: sprite 0 shows pixel value v in COLOR16 + v.
const SPRITE_COLOURS: u8 = 16;
/// The y 0x1622d6 uses for a hidden pointer: the line below the frame (0xc9, below the 200
/// shown; in Wide below the frame's [`HEIGHT`], not over the map).
const HIDDEN_Y: i16 = HEIGHT as i16 + 1;
/// Pixels per map cell side.
const CELL: i16 = 16;

/// The leader's pixel, the centre of view cell ([`HALF_W`], [`HALF_H`]): where the pointer
/// origin A4-0x7a26 rests (spec 13 Design 3). Original (0x58, 0x58), 5 · 16 + 8 on each axis.
pub const LEADER_PIXEL: (u16, u16) = (
    MAP_X + HALF_W as u16 * CELL as u16 + CELL as u16 / 2,
    MAP_Y + HALF_H as u16 * CELL as u16 + CELL as u16 / 2,
);

/// The view cell under frame pixel (x, y): `((x - MAP_X) >> 4, (y - MAP_Y) >> 4)` (Original
/// `(x >> 4, y >> 4)`, the map at the screen's top left).
pub fn view_cell(x: i16, y: i16) -> (i16, i16) {
    ((x - MAP_X as i16) >> 4, (y - MAP_Y as i16) >> 4)
}

/// The top-left frame pixel of view cell (col, row), where a cursor puts the pointer:
/// `(MAP_X + col · 16, MAP_Y + row · 16)` (Original `(col << 4, row << 4)`).
pub fn cell_pixel(col: i16, row: i16) -> (i16, i16) {
    (MAP_X as i16 + col * CELL, MAP_Y as i16 + row * CELL)
}

/// Frame column `x` is over the map: from the view's left edge to the shown map's right edge
/// (Original `x < 0xa8`, the map at the screen's left edge).
pub fn map_column(x: i16) -> bool {
    (MAP_X as i16..=geom::SHOWN.2 as i16).contains(&x)
}

/// Frame row `y` is over the map: from the view's top edge to the shown map's bottom edge
/// (Original `y < 0xa8`).
pub fn map_row(y: i16) -> bool {
    (MAP_Y as i16..=geom::SHOWN.3 as i16).contains(&y)
}

/// Frame pixel (x, y) is in the shown map [`geom::SHOWN`] (Original 8..=0xa7 on both axes).
pub fn in_shown_map(x: i16, y: i16) -> bool {
    let (x0, y0, x1, y1) = geom::SHOWN;
    (x0 as i16..=x1 as i16).contains(&x) && (y0 as i16..=y1 as i16).contains(&y)
}

/// Click region `i` of `tables` (x0, y0, x1, y1 inclusive), as the click map scans them. In Wide
/// region 0, the image's map window, is the shown map [`geom::SHOWN`] beside the UI; the command bar,
/// the side panel and the screen keep the image's rects (the UI keeps its coordinates); the
/// regions of [`SLOT_REGIONS`] are the slots' rects.
pub fn region(tables: &PointerTables, i: usize) -> Option<[i16; 4]> {
    if SLOT_REGIONS.contains(&i) {
        let (x, y) = geom::SLOTS[i - SLOT_REGIONS.start];
        let (w, h) = (geom::SLOT_W, geom::SLOT_H);
        return Some([x, y, x + w - 1, y + h - 1].map(|v| v as i16));
    }
    let r = tables.regions.get(i).copied()?;
    if cfg!(feature = "wide") && i == 0 {
        let (x0, y0, x1, y1) = geom::SHOWN;
        return Some([x0, y0, x1, y1].map(|v| v as i16));
    }
    Some(r)
}

/// One pointer shape: 16 pixels per row, values 0..=3 (0 = clear), and the offset of its top
/// left from the hot spot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    pub dx: i16,
    pub dy: i16,
    pub rows: Vec<[u8; 16]>,
}

/// The pointer tables, read from the user's `game`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PointerTables {
    /// Original: A4-0x79c2 (the click regions)
    pub regions: Vec<[i16; 4]>,
    /// Original: 0x1624b2 / 0x1624da / 0x1624ee / 0x162502 (the sprite shapes)
    pub shapes: Vec<Shape>,
    /// Original: A4-0x79c6 (initial walk cell)
    pub walk_cell: (i16, i16),
    /// Original: 0x1624ae (initial: shown)
    pub shown: bool,
}

impl PointerTables {
    /// Reads the regions, the ten sprite shapes (two bit planes per 16-pixel row in the CHIP
    /// data hunk 28) and the initial values.
    pub fn load(game: &[u8]) -> Result<PointerTables, FormatError> {
        let img = GameImage::parse(game)?;
        let word = |addr: u32| -> Result<i16, FormatError> {
            let b = img.bytes(addr, 2)?;
            Ok(i16::from_be_bytes([b[0], b[1]]))
        };
        let a4 = |off: i32| word(crate::image::A4.wrapping_add_signed(off));
        let mut regions = Vec::with_capacity(REGION_COUNT);
        for i in 0..REGION_COUNT as i32 {
            let r = REGIONS + 8 * i;
            regions.push([a4(r)?, a4(r + 2)?, a4(r + 4)?, a4(r + 6)?]);
        }
        let mut shapes = Vec::with_capacity(SHAPES);
        for i in 0..SHAPES as u32 {
            let data = img.pointer(SHAPE_DATA + 4 * i)?;
            let height = (word(SHAPE_HEIGHT + 2 * i)? as u16 >> 8) as u32;
            let rows = (0..height)
                .map(|r| {
                    let a = word(data + 4 + 4 * r).map(|w| w as u16)?;
                    let b = word(data + 6 + 4 * r).map(|w| w as u16)?;
                    Ok(std::array::from_fn(|c| {
                        let bit = 15 - c;
                        (((a >> bit) & 1) | (((b >> bit) & 1) << 1)) as u8
                    }))
                })
                .collect::<Result<_, FormatError>>()?;
            shapes.push(Shape {
                dx: word(SHAPE_DX + 2 * i)?,
                dy: word(SHAPE_DY + 2 * i)?,
                rows,
            });
        }
        Ok(PointerTables {
            regions,
            shapes,
            walk_cell: (a4(WALK_CELL)?, a4(WALK_CELL + 2)?),
            shown: word(SHOWN)? != 0,
        })
    }
}

/// The pointer state of the game (the handler's own position and buttons are the engine's,
/// [`Pointer`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PointerState {
    pub tables: PointerTables,
    /// Original: 0x1624ae (word: the pointer sprite is shown)
    pub shown: bool,
    /// Original: A4+0x1908 (word: the pointer shape, 0x15cff8)
    pub shape: u16,
    /// Original: A4+0x619a / +0x619c (words: the sampled position)
    pub at: (i16, i16),
    /// Original: A4+0x6198 (byte: the left button alone is down)
    pub left: bool,
    /// Original: A4+0x6199 (byte: 2 while the right button alone is down)
    pub right: u8,
    /// Original: A4-0x7a34 (word: the buttons went down at this sample)
    pub pressed: bool,
    /// Original: A4-0x2f06 (word: the buttons of the click; its low byte -0x2f05 is tested)
    pub buttons: u16,
    /// Original: A4-0x3076 (word: the code of the press; while held only that code repeats)
    pub press_code: u16,
    /// Original: A4-0x79c6 / -0x79c4 (words: window cell of the last walk click)
    pub walk_cell: (i16, i16),
    /// Port-only: the engine clock (ms) at which a held walk's next sample is due, kept while
    /// the command loop's last pointer code was a walk ([`crate::ui::target::HELD_WALK_MS`]);
    /// `None` when no walk is paced.
    pub walk_due: Option<u64>,
}

impl PointerState {
    pub fn from_tables(tables: PointerTables) -> PointerState {
        PointerState {
            shown: tables.shown,
            walk_cell: tables.walk_cell,
            tables,
            ..PointerState::default()
        }
    }
}

/// Original: 0x14f12c (os: copy the handler's pointer into the game's sample: the position, and
/// which button alone is down; both down reads as neither)
pub fn sample(s: &mut State, p: Pointer) {
    let ptr = &mut s.ui.pointer;
    ptr.at = (p.x, p.y);
    ptr.left = p.buttons == 1;
    ptr.right = if p.buttons == 2 { 2 } else { 0 };
}

/// Original: 0x134eae (ui: set the pointer shape)
pub fn set_shape(s: &mut State, shape: u16) {
    s.ui.pointer.shape = shape;
}

/// Original: 0x134d34 (ui: show the pointer sprite)
pub fn show(s: &mut State) {
    s.ui.pointer.shown = true;
}

/// Original: 0x1051a6 (ui: the pointer shape for the sampled position: in the map window
/// (8..=0xa7 on both axes; the shown map [`geom::SHOWN`]) the walk arrow of its direction from the
/// pointer origin, the cross-hair on the origin's own cell or while a command waits for its
/// target; elsewhere the arrow, or the cross-hair while a target is pending)
pub fn update_shape(s: &mut State) {
    let (x, y) = s.ui.pointer.at;
    let pending = s.ui.input.target_pending != 0;
    let in_window = in_shown_map(x, y);
    let shape = if pending {
        1
    } else if in_window {
        let dir = crate::inventory::direction_to(s.inventory.pointer_origin, x, y);
        if dir < 0 { 1 } else { dir as u16 + 2 }
    } else {
        0
    };
    set_shape(s, shape);
}

/// The pointer sprite of one frame: its rows (values 0..=3) with the top left at (x, y).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteFrame {
    pub x: i16,
    pub y: i16,
    pub rows: Vec<[u8; 16]>,
}

impl SpriteFrame {
    /// Draws the sprite over `fb`: value v ≠ 0 in colour 16 + v; only the frame shows it
    /// ([`WIDTH`] × [`HEIGHT`]: the 320 × 200 display, in Wide the map beside it too).
    pub fn draw(&self, fb: &mut Framebuffer) {
        for (r, row) in self.rows.iter().enumerate() {
            let y = i32::from(self.y) + r as i32;
            if !(0..HEIGHT as i32).contains(&y) {
                continue;
            }
            for (c, &v) in row.iter().enumerate() {
                let x = i32::from(self.x) + c as i32;
                if v != 0 && (0..WIDTH as i32).contains(&x) {
                    fb.set(x as usize, y as usize, SPRITE_COLOURS + v);
                }
            }
        }
    }
}

/// Original: 0x1622d6 (graphics: place the pointer sprite: the shape's data with its top left
/// at the handler's position plus the shape's offsets; a hidden pointer goes to line 0xc9)
///
/// `None` while the tables are not loaded (synthetic states).
pub fn sprite(ptr: &PointerState, p: Pointer) -> Option<SpriteFrame> {
    let shape = ptr.tables.shapes.get(usize::from(ptr.shape & 0x3fff))?;
    let y = if ptr.shown { p.y } else { HIDDEN_Y };
    Some(SpriteFrame {
        x: p.x.wrapping_add(shape.dx),
        y: y.wrapping_add(shape.dy),
        rows: shape.rows.clone(),
    })
}

/// Original: 0x162294 (graphics: vertical-blank server: pointer shape, sprite) [partial]
///
/// The pointer shape (0x1051a6), then the sprite (0x1622d6); the engine runs it after every
/// tick with the handler's pointer. Not ported: the BEAMCON0
/// write of the PAL / NTSC toggle (no display mode) and the frame flag 0x1624b0 that 0x1622be
/// waits on (the engine's display generation, [`crate::engine::Io::vblank`]).
pub fn vblank(s: &mut State, p: Pointer) -> Option<SpriteFrame> {
    update_shape(s);
    sprite(&s.ui.pointer, p)
}
