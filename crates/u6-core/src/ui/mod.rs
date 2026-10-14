//! UI: message scroll + printf 0x108658, side panels 0x103f64 / 0x110d2c, key read 0x10790a, the
//! raw-key → game-code table 0x151d00 (WU-P4).
//!
//! Domain state = [`UiState`]: the message scroll and the panel state. The input.device handler
//! itself (0x151b08) is replaced by the engine's event queue ([`crate::Io::next_key`]).
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x103f64 [ui] redraw side panel for current panel mode
//! - 0x105016 [ui] leaf; callers 8, from 3 subsystems
//! - 0x1051a6 [ui] unnamed; 2 callees
//! - 0x10598a [ui] unnamed; calls 0x152ffa (tile blitter, entry A (shares~)
//! - 0x1059d4 [ui] unnamed; 2 callees
//! - 0x105a3e [ui] unnamed; calls 0x152ffa (tile blitter, entry A (shares~)
//! - 0x105b14 [ui] unnamed; calls 0x152ffa (tile blitter, entry A (shares~)
//! - 0x105c6a [ui] unnamed; 1 callees
//! - 0x105c8e [ui] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x105d78 [ui] leaf; callers 1, from 1 subsystems
//! - 0x105ebe [ui] unnamed; 1 callees
//! - 0x105f7c [ui] unnamed; 3 callees
//! - 0x106092 [ui] leaf; callers 1, from 1 subsystems
//! - 0x10613a [ui] leaf; callers 2, from 1 subsystems
//! - 0x10619c [ui] uses "It's locked."
//! - 0x106474 [ui] leaf; callers 1, from 1 subsystems
//! - 0x106506 [ui] unnamed; 2 callees
//! - 0x10658e [ui] step of 0x106bbc (wait for next input event, map cl~)
//! - 0x1067d8 [ui] leaf; callers 2, from 1 subsystems
//! - 0x106812 [ui] step of 0x106bbc (wait for next input event, map cl~)
//! - 0x106932 [ui] unnamed; calls 0x153002 (tile blitter, entry B [likely])
//! - 0x106bbc [ui] wait for next input event, map clicks to commands
//! - 0x1078d2 [ui] step of 0x10790a (read a command key; handles panel~)
//! - 0x10790a [ui] read a command key; handles panel/mouse events
//! - 0x107cba [ui] unnamed; 3 callees
//! - 0x107d7c [ui] step of 0x1351d8 (main(): setup, load data, charact~)
//! - 0x107dd8 [ui] unnamed; 1 callees
//! - 0x107e00 [ui] unnamed; 1 callees
//! - 0x107e5a [ui] unnamed; calls 0x10790a (read a command key; handles p~)
//! - 0x10827c [ui] unnamed; 1 callees
//! - 0x10829a [ui] step of 0x108376 (word-wrapped text output [likely])
//! - 0x1082b0 [ui] leaf; callers 3, from 2 subsystems
//! - 0x108376 [ui] word-wrapped text output [likely]
//! - 0x108642 [ui] step of 0x108658 (printf-style output to the messag~)
//! - 0x108658 [ui] printf-style output to the message scroll
//! - 0x1109e0 [ui] party roster panel [likely]
//! - 0x110c76 [ui] print version line
//! - 0x110d2c [ui] party-member stats panel (STR/DEX/INT, HP/MP, level)
//! - 0x110f96 [ui] leaf; callers 8, from 4 subsystems
//! - 0x1110c8 [ui] leaf; callers 4, from 3 subsystems
//! - 0x1110f6 [ui] unnamed; 2 callees
//! - 0x1111ca [ui] unnamed; 2 callees
//! - 0x1112cc [ui] unnamed; calls 0x10ae52 (object stacked on / contained~)
//! - 0x1113fe [ui] unnamed; calls 0x153002 (tile blitter, entry B [likely])
//! - 0x111882 [ui] step of 0x1118c6 (uses "")
//! - 0x1118c6 [ui] uses ""
//! - 0x111be0 [ui] unnamed; 2 callees
//! - 0x111c30 [inventory] inventory panel [likely]
//! - 0x111dca [ui] step of 0x10790a (read a command key; handles panel~)
//! - 0x111e0a [ui] step of 0x10790a (read a command key; handles panel~)
//! - 0x111e3c [ui] leaf; callers 1, from 1 subsystems
//! - 0x111eec [ui] leaf; callers 1, from 1 subsystems
//! - 0x13242e [ui] leaf; callers 2, from 1 subsystems
//! - 0x1324ae [ui] unnamed; calls 0x152ffa (tile blitter, entry A (shares~)
//! - 0x1325e4 [ui] unnamed; calls 0x152ffa (tile blitter, entry A (shares~)
//! - 0x132706 [ui] unnamed; calls 0x102c90 (present/refresh map window [l~)
//! - 0x1331b4 [ui] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x133a1a [ui] unnamed; calls 0x102c90 (present/refresh map window [l~)
//! - 0x133a96 [ui] leaf; callers 1, from 1 subsystems
//! - 0x133b0e [ui] unnamed; calls 0x102c90 (present/refresh map window [l~)
//! - 0x133b28 [ui] unnamed; calls 0x152ffa (tile blitter, entry A (shares~)
//! - 0x133fe0 [ui] unnamed; calls 0x153002 (tile blitter, entry B [likely])
//! - 0x134044 [ui] unnamed; 2 callees
//! - 0x13410a [ui] unnamed; calls 0x10790a (read a command key; handles p~)
//! - 0x151b08 [input] input.device event handler (queue key/mouse events)
//! - 0x151c60 [input] open input.device, install the handler
//! - 0x151cdc [input] remove the input handler
//! - 0x151d00 [input] raw key code to key/command, qualifier state

/// Message scroll and side-panel state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiState {
    /// Original: the message scroll printf 0x108658 writes to (buffer address not identified)
    pub messages: Vec<String>,
    /// Original: the panel mode 0x103f64 redraws for (address and values not identified)
    pub panel_mode: u8,
}
