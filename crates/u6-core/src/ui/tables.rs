//! The strings, tables and initial values of the text layer, read from the user's `game` at load
//! time ([`UiTables::load`]); see [`super::image`]. Every item names where it sits in the image.

use u6_formats::FormatError;

use super::text::TextWindow;
use crate::image::GameImage;

/// Raw key codes the table covers (0x00..0x5f; 0x60..0x67 are qualifiers, handled in code).
pub const RAW_KEYS: usize = 0x60;
/// Combat-mode values that have a name (0x110c76 indexes the table with the actor's mode).
pub const COMBAT_MODES: std::ops::RangeInclusive<u8> = 2..=8;

/// The colour indices the text layer uses (words in the data hunk).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Colours {
    /// Original: A4-0x656a (text colour: window byte 6 after every line)
    pub text: u8,
    /// Original: A4-0x656c (paper: window byte 7, panel background)
    pub paper: u8,
    /// Original: A4-0x6576 (low health, `@` keywords)
    pub alert: u8,
    /// Original: A4-0x6574 (poisoned member: status bit 3)
    pub poisoned: u8,
    /// Original: A4-0x6572 (selected command underline, 0x106506)
    pub command: u8,
}

/// Format strings of the side panels (code hunk 8, read PC-relative by 0x1109e0 / 0x110d2c /
/// 0x111c30 / 0x110c76).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelStrings {
    /// Original: 0x110ba4 (roster hit points)
    pub roster_hp: Vec<u8>,
    /// Original: 0x110baa (a member's name and a newline)
    pub name_line: Vec<u8>,
    /// Original: 0x110bae (a bare string: the combat-mode name)
    pub plain: Vec<u8>,
    /// Original: 0x110bb2, 0x110bba, 0x110bc2 (strength, dexterity, intelligence lines)
    pub attributes: [Vec<u8>; 3],
    /// Original: 0x110bca ("Magic" heading)
    pub magic: Vec<u8>,
    /// Original: 0x110bd0 (magic points / maximum)
    pub magic_value: Vec<u8>,
    /// Original: 0x110bd8 ("Health" heading)
    pub health: Vec<u8>,
    /// Original: 0x110be0 (hit points)
    pub hp_value: Vec<u8>,
    /// Original: 0x110be4 (maximum hit points)
    pub hp_max: Vec<u8>,
    /// Original: 0x110bea (level / experience heading)
    pub level_exp: Vec<u8>,
    /// Original: 0x110bf4 (level / experience values)
    pub level_exp_value: Vec<u8>,
    /// Original: 0x110c0c / 0x110c10 (inventory weight labels)
    pub weights: [Vec<u8>; 2],
    /// Original: A4-0x7230 (pointer per combat mode; modes 2..=8 have a name)
    pub combat_modes: Vec<Vec<u8>>,
}

/// Port-only (spec 08, slice K2; 2026-10-24): the panel strings the stats view passes to
/// `printf` as its format ([`PanelStrings`]), for the locale layer's catalogue
/// ([`crate::locale::strings`]): the strength, dexterity and intelligence lines and the magic,
/// health and level / experience headings. Each is drawn at a fixed cell, the portrait or its
/// value beside it, so the catalogue gives each the English's cells as its budget. The
/// roster's hit points (a 3-cell field at column 0x0e of the 17-cell window), the name line (a
/// name and a newline), the bare string the combat-mode name goes through, and the values
/// (magic points, hit points and their maximum, level and experience) hold no word of their
/// own and are not listed: they stay as they are. The inventory view's weight labels are read
/// for display instead ([`SHOWN_AT`]).
pub(crate) const PRINTED: &[u32] = &[
    0x0011_0bb2,
    0x0011_0bba,
    0x0011_0bc2,
    0x0011_0bca,
    0x0011_0bd8,
    0x0011_0bea,
];

/// Port-only (spec 08, slice K2; 2026-10-24): the image addresses of the panel strings read for
/// display as labels, the strings' own addresses (no pointer slot holds them): the inventory
/// view's two weight labels, 2 bytes each in the original, under the 3 a print hook looks up. The
/// view reads each through [`crate::locale::shown`] and puts what it gives without a lookup
/// again (`ui::printf::format`), so the locale layer's catalogue takes each as a label from 1
/// byte, within its English's cells (the next column beside it).
pub(crate) const SHOWN_AT: &[u32] = &[0x0011_0c0c, 0x0011_0c10];

/// Port-only (spec 08, slice K2; 2026-10-24): the pointers of the combat-mode names, A4-0x7230 +
/// 4 × mode for modes 2..=8, labels of the locale layer's catalogue; the combat-mode line reads
/// the name for display through [`crate::locale::shown`]. Logic reads [`PanelStrings`] as it is.
pub(crate) const LABELS: &[u32] = &[
    mode_pointer(2),
    mode_pointer(3),
    mode_pointer(4),
    mode_pointer(5),
    mode_pointer(6),
    mode_pointer(7),
    mode_pointer(8),
];

/// Port-only (spec 08, 2026-10-24): the cells a combat-mode name takes at most, the field its
/// line centres it in: status-window columns 10..=16, x 0x100..=0x137 (the window opens at cell
/// 0x16, x 0xb0; the line's paper clear spans the field).
pub(crate) const MODE_CELLS: u16 = 7;

/// The pointer slot of combat mode `mode`'s name.
const fn mode_pointer(mode: u32) -> u32 {
    crate::image::A4.wrapping_sub(0x7230) + 4 * mode
}

/// Initial values of the text and input globals (data hunk 33).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Initial {
    /// Original: A4-0x7a4c (text output on)
    pub text_on: bool,
    /// Original: A4-0x7a1c (first cursor glyph of the idle cursor)
    pub cursor_glyph: u16,
    /// Original: A4-0x7a46 (key poll drains the queue)
    pub key_drain: bool,
    /// Original: A4-0x7a50 (`@` keywords take the alert colour)
    pub keyword_colour: bool,
    /// Original: A4-0x7a0c (flags a new window starts with: 0x107dca reads the word and stores
    /// its low byte)
    pub window_flags: u8,
    /// Original: A4-0x7a42 (key events go through command processing)
    pub accept: bool,
    /// Original: A4-0x7a40 (upper-case letters before command processing)
    pub upcase: bool,
    /// Original: A4-0x7a2e (no command waiting for a target)
    pub target_command: u16,
    /// Original: A4-0x7a59 (panel cursor column)
    pub panel_cursor_col: i8,
    /// Original: A4-0x7a56 (refresh the map while the cursor idles)
    pub idle_refresh: bool,
    /// Original: A4-0x7a30 (panel mode)
    pub panel_mode: u16,
    /// Original: A4-0x7afe (panel needs a redraw)
    pub panel_dirty: bool,
}

/// Everything the text layer reads from `game`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiTables {
    /// Original: A4-0x3572 (8 bytes per raw key 0..0x5f: plain, shifted, control, alt code)
    pub keymap: Vec<[u16; 4]>,
    /// Original: 0x104ffe (characters that end an `@` keyword, 0x108376)
    pub keyword_end: Vec<u8>,
    pub colours: Colours,
    pub panel: PanelStrings,
    pub initial: Initial,
    /// Original: A4-0x61ce / -0x61c6 (x / y step of each direction 0..7, north clockwise)
    pub step: [(i8, i8); 8],
    /// Original: A4-0x797e (5 words: the tiles of the panel's bottom row of icons; 0x106932
    /// rewrites all but the second)
    pub panel_icons: [u16; 5],
    /// Original: A4-0x79a2 (7 words: panel cursor x per column, rows 0..2)
    pub cursor_x: [u16; 7],
    /// Original: A4-0x7994 (3 × 7 bytes: panel cursor y per row and column)
    pub cursor_y: [[u8; 7]; 3],
    /// Original: A4-0x63c6 (15 bytes: height of the sun or a moon at each sky position)
    pub sky_heights: [u8; 15],
    /// Original: A4-0x7a1a (the window A4-0x7a10 points at before main sets up its own)
    pub default_window: TextWindow,
}

/// `N` big-endian words at `A4 + off`.
fn words<const N: usize>(img: &GameImage<'_>, off: i32) -> Result<[u16; N], FormatError> {
    let b = img.a4_bytes(off, N * 2)?;
    Ok(std::array::from_fn(|i| {
        u16::from_be_bytes([b[2 * i], b[2 * i + 1]])
    }))
}

impl UiTables {
    /// Reads every item from the decrunched `game` executable.
    pub fn load(game: &[u8]) -> Result<Self, FormatError> {
        let img = GameImage::parse(game)?;
        let s = |addr| img.cstr(addr);
        let colour = |off: i32| img.a4_word(off).map(|w| w as u8);
        let keys = img.a4_bytes(-0x3572, RAW_KEYS * 8)?;
        let keymap = keys
            .chunks_exact(8)
            .map(|e| std::array::from_fn(|i| u16::from_be_bytes([e[2 * i], e[2 * i + 1]])))
            .collect();
        let mut combat_modes = Vec::new();
        for mode in COMBAT_MODES {
            let ptr = img.pointer(mode_pointer(u32::from(mode)))?;
            combat_modes.push(img.cstr(ptr)?);
        }
        let w = img.a4_bytes(-0x7a1a, 9)?;
        let (dx, dy) = (img.a4_bytes(-0x61ce, 8)?, img.a4_bytes(-0x61c6, 8)?);
        let byte = |off: i32| img.a4_bytes(off, 1).map(|b| b[0]);
        Ok(UiTables {
            keymap,
            keyword_end: s(0x104ffe)?,
            colours: Colours {
                text: colour(-0x656a)?,
                paper: colour(-0x656c)?,
                alert: colour(-0x6576)?,
                poisoned: colour(-0x6574)?,
                command: colour(-0x6572)?,
            },
            panel: PanelStrings {
                roster_hp: s(0x110ba4)?,
                name_line: s(0x110baa)?,
                plain: s(0x110bae)?,
                attributes: [s(0x110bb2)?, s(0x110bba)?, s(0x110bc2)?],
                magic: s(0x110bca)?,
                magic_value: s(0x110bd0)?,
                health: s(0x110bd8)?,
                hp_value: s(0x110be0)?,
                hp_max: s(0x110be4)?,
                level_exp: s(0x110bea)?,
                level_exp_value: s(0x110bf4)?,
                weights: [s(0x110c0c)?, s(0x110c10)?],
                combat_modes,
            },
            initial: Initial {
                text_on: img.a4_word(-0x7a4c)? != 0,
                cursor_glyph: img.a4_word(-0x7a1c)?,
                key_drain: img.a4_word(-0x7a46)? != 0,
                keyword_colour: img.a4_word(-0x7a50)? != 0,
                window_flags: img.a4_word(-0x7a0c)? as u8,
                accept: img.a4_word(-0x7a42)? != 0,
                upcase: img.a4_word(-0x7a40)? != 0,
                target_command: img.a4_word(-0x7a2e)?,
                panel_cursor_col: byte(-0x7a59)? as i8,
                idle_refresh: byte(-0x7a56)? != 0,
                panel_mode: img.a4_word(-0x7a30)?,
                panel_dirty: byte(-0x7afe)? != 0,
            },
            step: std::array::from_fn(|i| (dx[i] as i8, dy[i] as i8)),
            panel_icons: words(&img, -0x797e)?,
            cursor_x: words(&img, -0x79a2)?,
            sky_heights: {
                let b = img.a4_bytes(-0x63c6, 15)?;
                std::array::from_fn(|i| b[i])
            },
            cursor_y: {
                let b = img.a4_bytes(-0x7994, 21)?;
                std::array::from_fn(|r| std::array::from_fn(|c| b[r * 7 + c]))
            },
            default_window: TextWindow::from_bytes(w),
        })
    }

    /// The name of combat mode `mode`; modes without a table entry have none.
    pub fn combat_mode_name(&self, mode: u8) -> &[u8] {
        let names = &self.panel.combat_modes;
        mode.checked_sub(*COMBAT_MODES.start())
            .and_then(|i| names.get(usize::from(i)))
            .map_or(&[], Vec::as_slice)
    }
}
