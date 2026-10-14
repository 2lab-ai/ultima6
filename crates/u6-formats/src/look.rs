//! `look.lzd` object names — formats.md §4.4 (N: core/look.cpp:58-100 load, :105-140 lookup).
//!
//! Whole-file U6-LZW → entries `[u16 LE tile][NUL-terminated ASCII]` in ascending tile order,
//! ended by a tile ≥ 2048. A tile without its own entry uses the next entry's string; tiles past
//! the last entry use entry 0 ("Nothing"). Strings carry plural markers: letters after `\` are
//! plural-only, letters after `/` singular-only (look.cpp:125).

use crate::bytes;
use crate::error::{FormatError, Result};
use crate::lzw;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Tiles covered by the look table.
pub const LOOK_TILES: u16 = 2048;

/// One look-table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LookEntry {
    pub tile: u16,
    /// The raw string (Latin-1 decoded, markers kept).
    pub text: String,
}

/// The decoded look table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LookTable {
    pub entries: Vec<LookEntry>,
    /// Decoded bytes from the terminating tile word to the end (kept so nothing is lost).
    pub trailer: Vec<u8>,
}

impl LookTable {
    /// Decodes `look.lzd` (LZW) and parses it.
    pub fn from_lzd(file: &[u8]) -> Result<Self> {
        Self::parse(&lzw::decode(file)?)
    }

    /// Parses the LZW-decoded table.
    pub fn parse(decoded: &[u8]) -> Result<Self> {
        let mut entries: Vec<LookEntry> = Vec::new();
        let mut pos = 0;
        loop {
            let tile = bytes::u16_le(decoded, pos, "look tile (no terminator >= 2048)")?;
            if tile >= LOOK_TILES {
                return Ok(LookTable {
                    entries,
                    trailer: decoded[pos..].to_vec(),
                });
            }
            if let Some(prev) = entries.last()
                && tile <= prev.tile
            {
                return Err(FormatError::invalid(
                    "look tile",
                    pos,
                    format!("tile {tile} after {} (not ascending)", prev.tile),
                ));
            }
            let start = pos + 2;
            let len =
                decoded[start..]
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or(FormatError::Truncated {
                        what: "look string (no NUL)",
                        offset: start,
                        needed: decoded.len() - start + 1,
                        len: decoded.len(),
                    })?;
            entries.push(LookEntry {
                tile,
                text: bytes::latin1_until_nul(&decoded[start..]),
            });
            pos = start + len + 1;
        }
    }

    /// The raw string Nuvie shows for `tile`, or `None` for tile ≥ 2048 or an empty table.
    pub fn text_for_tile(&self, tile: u16) -> Option<&str> {
        if tile >= LOOK_TILES {
            return None;
        }
        self.entries
            .iter()
            .find(|e| e.tile >= tile)
            .or(self.entries.first())
            .map(|e| e.text.as_str())
    }

    /// The description for `tile` with the plural markers resolved.
    pub fn description(&self, tile: u16, plural: bool) -> Option<String> {
        self.text_for_tile(tile).map(|t| resolve_plural(t, plural))
    }
}

/// Applies look.cpp:125-140: after `\`, letters are kept only for the plural; after `/`, only for
/// the singular; the marker itself is dropped.
pub fn resolve_plural(text: &str, plural: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' || c == '/' {
            let keep = (plural && c == '\\') || (!plural && c == '/');
            while let Some(&a) = chars.peek().filter(|a| a.is_ascii_alphabetic()) {
                if keep {
                    out.push(a);
                }
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<u8> {
        let mut d = Vec::new();
        for (t, s) in [
            (0u16, "Nothing"),
            (1, "grass"),
            (4, "swamp"),
            (9, "loa/f\\ves"),
        ] {
            d.extend_from_slice(&t.to_le_bytes());
            d.extend_from_slice(s.as_bytes());
            d.push(0);
        }
        d.extend_from_slice(&0x0800u16.to_le_bytes());
        d.extend_from_slice(&[0xAA, 0xBB]);
        d
    }

    #[test]
    fn parses_entries_and_fills_gaps() {
        let t = LookTable::parse(&table()).unwrap();
        assert_eq!(t.entries.len(), 4);
        assert_eq!(t.entries[2].tile, 4);
        assert_eq!(t.trailer, [0x00, 0x08, 0xAA, 0xBB]);
        assert_eq!(t.text_for_tile(0), Some("Nothing"));
        assert_eq!(t.text_for_tile(2), Some("swamp"));
        assert_eq!(t.text_for_tile(4), Some("swamp"));
        assert_eq!(t.text_for_tile(100), Some("Nothing"));
        assert_eq!(t.text_for_tile(2048), None);
        assert_eq!(t.description(9, false).as_deref(), Some("loaf"));
        assert_eq!(t.description(9, true).as_deref(), Some("loaves"));
    }

    #[test]
    fn from_lzd_decodes_first() {
        let file = lzw::encode(&table()).unwrap();
        assert_eq!(
            LookTable::from_lzd(&file).unwrap(),
            LookTable::parse(&table()).unwrap()
        );
    }

    #[test]
    fn rejects_malformed() {
        let d = table();
        assert!(LookTable::parse(&d[..d.len() - 4]).is_err()); // no terminator
        assert!(LookTable::parse(&[0, 0, b'a']).is_err()); // no NUL
        assert!(LookTable::parse(&[5, 0, b'a', 0, 3, 0, b'b', 0, 0, 8]).is_err()); // descending
    }

    #[test]
    fn plural_markers() {
        assert_eq!(resolve_plural("bag\\s", true), "bags");
        assert_eq!(resolve_plural("bag\\s", false), "bag");
        assert_eq!(resolve_plural("m/ouse\\ice", true), "mice");
        assert_eq!(resolve_plural("m/ouse\\ice", false), "mouse");
        assert_eq!(resolve_plural("pile/ of gold", false), "pile of gold");
    }
}
