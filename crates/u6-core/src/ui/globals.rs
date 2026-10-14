//! The input and panel globals the key read and the side panels share.

use super::input::Qualifiers;
use super::tables::UiTables;
use crate::actors::Actors;

/// No command is waiting for a target (A4-0x7a2e, A4-0x7b09 use 0x8f / 0xff for "none").
pub const NO_COMMAND: u16 = 0x8f;

/// Input state of 0x10790a / 0x106bbc / 0x105c8e and the raw map.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    pub qualifiers: Qualifiers,
    /// Original: A4-0x7a42 (key events go through command processing; 0 while the more prompt
    /// waits, so any key returns at once)
    pub accept: bool,
    /// Original: A4-0x7a1c (first glyph of the 4-frame idle cursor: 5, or 1 while waiting at
    /// `*` or a more prompt)
    pub cursor_glyph: u16,
    /// Original: A4-0x3078 (idle-cursor frame counter)
    pub cursor_frame: u8,
    /// Original: A4-0x7a46 (key poll drains the queue and keeps the last code). The engine queue
    /// replaces this policy (spec 03 §1: input is consumed in order); the flag is kept and
    /// written as the original writes it.
    pub drain: bool,
    /// Original: A4-0x7a57 (inside the line editor 0x107cba: return ends the line)
    pub line_input: bool,
    /// Original: A4-0x7a40 (letters are upper-cased before command processing)
    pub upcase: bool,
    /// Original: A4-0x7a44 (the line editor ended by escape)
    pub line_cancelled: bool,
    /// Original: A4-0x7a52 (clear the paged flag of the current window at the next poll)
    pub unpage: bool,
    /// Original: A4-0x7974 (deliver 0x8e at the next key read: repeat the last command)
    pub repeat: bool,
    /// Original: A4-0x2efc (direction of the last 0x80 code: 0 = north, clockwise to 7)
    pub direction: i16,
    /// Original: A4-0x7a60 (cursor mode: 0 off, 1 map cross-hair, 2 panel cursor, 3 spell)
    pub targeting: u8,
    /// Original: A4-0x7a2e (command waiting for a target, [`NO_COMMAND`] when none)
    pub target_command: u16,
    /// Original: A4-0x7a32 (the command loop waits for a target: keep the cursor mode)
    pub target_pending: u16,
    /// Original: A4-0x7a5b (the cross-hair was started by Tab, not by a command)
    pub cursor_by_tab: bool,
    /// Original: A4-0x7a5f / -0x7a5e (cross-hair cell in the 11 × 11 map window)
    pub cursor: (i8, i8),
    /// Original: A4-0x7a5d (cross-hair range from the centre; -1 = a direction selects at once)
    pub range: i8,
    /// Original: A4-0x7a59 / -0x7a58 (panel cursor column / row)
    pub panel_cursor: (i8, i8),
    /// Original: A4-0x2f0a / -0x2f08 (pointer position in pixels, also set from the cursors)
    pub pointer: (i16, i16),
    /// Original: A4-0x2f04 (selected object / member, -1 none)
    pub selection: i16,
    /// Original: A4-0x2f02 / -0x2f00 (selected map location)
    pub target: (i16, i16),
    /// Original: A4-0x2efe (the selected cell shows nothing: tile 0xff or 0x1bc)
    pub target_void: bool,
    /// Original: A4-0x7a56 (refresh the map view while the cursor idles)
    pub idle_refresh: bool,
}

impl InputState {
    pub fn from_tables(t: &UiTables) -> Self {
        let i = &t.initial;
        InputState {
            accept: i.accept,
            cursor_glyph: i.cursor_glyph,
            drain: i.key_drain,
            upcase: i.upcase,
            target_command: i.target_command,
            panel_cursor: (i.panel_cursor_col, 0),
            idle_refresh: i.idle_refresh,
            ..InputState::default()
        }
    }
}

/// Side-panel state (0x103f64 and the panel codes of 0x10790a).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelState {
    /// Original: A4-0x7a30 (panel mode: 0x90 member stats, 0x91 party roster, 0x92 inventory,
    /// 0x9e and 0xa1 the other modes)
    pub mode: u16,
    /// Original: A4-0x7a3e (party index of the member the panel shows)
    pub member: i16,
    /// Original: A4-0x7afe (redraw the panel at the next 0x103f64)
    pub dirty: bool,
    /// Original: A4-0x722c (first member of the roster list)
    pub roster_first: u8,
    /// Original: A4-0x722a (first row of the inventory list)
    pub inventory_first: u8,
    /// Original: A4+0x1346 (object whose contents the inventory panel shows)
    pub container: u16,
    /// Original: A4+0x1380 (container opened in the panel instead of a member, 0 = none)
    pub open_container: u16,
    /// Original: A4-0x797e (the bottom row of panel icons, tiles; see `UiTables::panel_icons`)
    pub icons: [u16; 5],
    /// Original: A4+0x1340 (objects in the inventory list)
    pub inventory_count: u8,
    /// Original: A4+0x1370 (byte per party member, read by 0x106932 / 0x111c30; meaning not
    /// identified)
    pub member_flags: [u8; 16],
    /// Original: A4+0x1382 (word read with A4+0x1380 by 0x106932; meaning not identified)
    pub container_flag: bool,
    /// Original: A4+0x1820 (no panel row above / below the inventory grid; meaning not
    /// identified)
    pub no_rows: bool,
}

impl PanelState {
    /// The flag byte of party member `member` (A4+0x1370).
    pub fn member_flag(&self, member: i16) -> bool {
        usize::try_from(member)
            .ok()
            .and_then(|m| self.member_flags.get(m))
            .is_some_and(|&b| b != 0)
    }
}

impl PanelState {
    pub fn from_tables(t: &UiTables) -> Self {
        PanelState {
            mode: t.initial.panel_mode,
            dirty: t.initial.panel_dirty,
            icons: t.panel_icons,
            ..PanelState::default()
        }
    }
}

/// Offsets into the 0x82-byte objlist block the game keeps at A4-0x7b84 (`Actors::objlist_tail`):
/// these ui globals live there, so they are saved with the game.
const TAIL_LAST_COMMAND: usize = 0x7b;
const TAIL_SOLO: usize = 0x79;

/// Original: A4-0x7b09 (the command whose icon is underlined, 0xff none; objlist block byte
/// 0x7b)
pub fn last_command(a: &Actors) -> u8 {
    a.objlist_tail
        .get(TAIL_LAST_COMMAND)
        .copied()
        .unwrap_or(0xff)
}

pub fn set_last_command(a: &mut Actors, cmd: u8) {
    if let Some(b) = a.objlist_tail.get_mut(TAIL_LAST_COMMAND) {
        *b = cmd;
    }
}

/// Original: A4-0x7b0b (party index of the solo member, negative in party mode; objlist block
/// byte 0x79)
pub fn solo_member(a: &Actors) -> i8 {
    a.objlist_tail.get(TAIL_SOLO).map_or(-1, |&b| b as i8)
}
