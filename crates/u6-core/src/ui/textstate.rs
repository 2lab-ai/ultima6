//! The text layer's globals: windows, the line being assembled, the escape state.

use super::tables::UiTables;
use super::text::{CAPITALISE, SCROLL, TextWindow, WRAP, WindowId};

/// Line buffer size: a window row (at most 40 cells) plus the character that overflowed it.
pub const LINE: usize = 41;
/// Numbered text variables (`#0`..`#9`, `#A`..`#Z`) and strings (`$0`..`$Z`).
pub const VARS: usize = 36;
/// Port-side transcript length kept in `UiState::messages`.
pub const TRANSCRIPT: usize = 64;

/// Everything 0x107e5a / 0x108376 / 0x108658 read and write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextState {
    /// Records of [`WindowId::ALL`], by index.
    pub windows: [TextWindow; 4],
    /// Original: A4-0x7a10 (pointer to the current window record)
    pub current: WindowId,
    /// Original: A4-0x3074 (characters of the line being assembled)
    pub line: [u8; LINE],
    /// Original: A4-0x304b (ink colour of each of those characters)
    pub line_ink: [u8; LINE],
    /// Original: A4-0x7972 (characters in the line buffer)
    pub line_len: u8,
    /// Original: A4-0x7971 (first character not drawn yet)
    pub line_start: u8,
    /// Original: A4-0x7970 (a character other than a space was added to this line)
    pub nonspace: bool,
    /// Original: A4-0x796f (pending escape prefix: one of `#$%/@\^`, 0 = none)
    pub prefix: u8,
    /// Original: A4-0x7a4c (text output on)
    pub on: bool,
    /// Original: A4-0x7a4e (inside an `&`...`&` section: not printed)
    pub hidden: bool,
    /// Original: A4-0x7a54 (the last number printed was not 1: `/` and `\` pick the plural)
    pub plural: bool,
    /// Original: A4-0x7a5a (capitalise the next character: set by a back-quote)
    pub capitalise: bool,
    /// Original: A4-0x7a48 (swallow the next newline in a scrolling window)
    pub skip_newline: bool,
    /// Original: A4-0x2efa (rows left before the more prompt of a paged window)
    pub more_rows: i16,
    /// Original: A4+0x64c8 (text was drawn since the cursor was last shown)
    pub drawn: bool,
    /// Original: A4-0x7a50 (`@` keywords get the alert colour)
    pub keyword_colour: bool,
    /// Port-side: the characters of the current scroll line drawn so far (for the transcript).
    pub transcript_line: Vec<u8>,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            windows: [TextWindow::default(); 4],
            current: WindowId::Default,
            line: [0; LINE],
            line_ink: [0; LINE],
            line_len: 0,
            line_start: 0,
            nonspace: false,
            prefix: 0,
            on: true,
            hidden: false,
            plural: false,
            capitalise: false,
            skip_newline: false,
            more_rows: 0,
            drawn: false,
            keyword_colour: false,
            transcript_line: Vec::new(),
        }
    }
}

impl TextState {
    /// The initial state from the data hunk: the default window current, output on.
    ///
    /// Replaces: 0x162228 (main's call of the two below), 0x162238, 0x16227c
    ///
    /// (Hunk 34 sets four 8-byte windows at 0x15804a to (0, 0, 0x27, 0x18, colour 0xf) with
    /// the pointer 0x158086 to them, and clears the words 0x158072 and 0x158092..0x158096; no
    /// code in `game` reads any of them, so the port keeps none.)
    pub fn from_tables(t: &UiTables) -> Self {
        let mut s = TextState {
            on: t.initial.text_on,
            keyword_colour: t.initial.keyword_colour,
            ..TextState::default()
        };
        s.windows[WindowId::Default.index()] = t.default_window;
        s
    }

    /// The current window record.
    pub fn win(&self) -> &TextWindow {
        &self.windows[self.current.index()]
    }

    pub fn win_mut(&mut self) -> &mut TextWindow {
        &mut self.windows[self.current.index()]
    }

    /// The window record of `id`.
    pub fn window(&self, id: WindowId) -> &TextWindow {
        &self.windows[id.index()]
    }

    /// Original: 0x107d7c (ui: set up window `id` over the inclusive corner cells with the default
    /// ink A4-0x656a, paper A4-0x656c and flags A4-0x7a0c, and make it current)
    ///
    /// The record pointer changes without the flush 0x107dd8 does.
    pub fn open(&mut self, t: &UiTables, id: WindowId, x0: u8, y0: u8, x1: u8, y1: u8) {
        let (fg, bg, f) = (t.colours.text, t.colours.paper, t.initial.window_flags);
        self.windows[id.index()] = TextWindow::new(x0, y0, x1, y1, fg, bg, f);
        self.current = id;
    }

    /// The three windows main sets up (0x1351d8 at 0x13533a..0x1353b6, three calls of 0x107d7c
    /// with corner cells from its code, colours A4-0x656a / -0x656c, flags A4-0x7a0c), leaving
    /// the message scroll current with [`WRAP`] | [`SCROLL`] | [`CAPITALISE`] added. Not a port
    /// of main (WU-P7): the window part only, so the text layer can run before main exists. The
    /// window select between the calls is a plain assignment here; main's 0x107dd8 call flushes
    /// an empty line, which draws nothing.
    pub fn main_windows(&mut self, t: &UiTables) {
        self.open(t, WindowId::Screen, 0, 0, 0x27, 0x18);
        self.open(t, WindowId::Status, 0x16, 1, 0x26, 0x0c);
        self.open(t, WindowId::Messages, 0x16, 0x0e, 0x26, 0x17);
        self.current = WindowId::Messages;
        self.win_mut().flags |= WRAP | SCROLL | CAPITALISE;
    }
}
