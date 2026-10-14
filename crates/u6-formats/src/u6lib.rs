//! U6Lib indexed archive — formats.md §2 (N: files/u6_lib_n.cpp:178 parse, :221 sizes, :271 count).
//!
//! An offset table at 0 (u16 LE or u32 LE entries), no size header. Item count =
//! `min(nonzero offset) / entry_size`; offset 0 = empty item; `size(i)` = the next nonzero offset
//! (or the file size) − `offset(i)`. For u32 entries `offset = v & 0xFFFFFF`, `flag = v >> 24`.

use crate::bytes;
use crate::error::{FormatError, Result};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Width of one offset-table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EntryWidth {
    /// u16 LE offsets (`book.dat`).
    U16,
    /// u32 LE offsets with a flag byte on top (`portrait.*`, `converse.*`).
    U32,
}

impl EntryWidth {
    /// Bytes per table entry.
    pub fn bytes(self) -> usize {
        match self {
            EntryWidth::U16 => 2,
            EntryWidth::U32 => 4,
        }
    }
}

/// One archive item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LibItem {
    /// Byte offset of the item in the source file; 0 = empty item.
    pub offset: u32,
    /// Top byte of a u32 entry (always 0 for U16 libs). Every flag is 0 in the Amiga files.
    pub flag: u8,
    /// The item bytes (empty for an empty item).
    pub data: Vec<u8>,
}

impl LibItem {
    /// True when the table entry is 0 (no item stored).
    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

/// A parsed U6Lib archive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct U6Lib {
    pub width: EntryWidth,
    pub items: Vec<LibItem>,
}

impl U6Lib {
    /// Splits `data` into items using Nuvie's count and size rules.
    pub fn parse(data: &[u8], width: EntryWidth) -> Result<Self> {
        let es = width.bytes();
        let read = |i: usize| -> Result<(u32, u8)> {
            let at = i * es;
            Ok(match width {
                EntryWidth::U16 => (u32::from(bytes::u16_le(data, at, "u6lib offset")?), 0),
                EntryWidth::U32 => {
                    let v = bytes::u32_le(data, at, "u6lib offset")?;
                    (v & 0x00FF_FFFF, (v >> 24) as u8)
                }
            })
        };
        // The table ends at the smallest nonzero offset; scan entries until we reach it.
        let mut min = data.len();
        let mut i = 0;
        while (i + 1) * es <= min {
            let (off, _) = read(i)?;
            if off != 0 && (off as usize) < min {
                min = off as usize;
            }
            i += 1;
        }
        if min == data.len() {
            return Err(FormatError::invalid(
                "u6lib offset table",
                0,
                "no nonzero offset, item count is undefined",
            ));
        }
        let count = min / es;
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let (off, flag) = read(i)?;
            if off != 0 && (off as usize) < count * es {
                return Err(FormatError::invalid(
                    "u6lib offset",
                    i * es,
                    format!("item {i} offset {off:#x} points into the offset table"),
                ));
            }
            if off as usize > data.len() {
                return Err(FormatError::invalid(
                    "u6lib offset",
                    i * es,
                    format!(
                        "item {i} offset {off:#x} is past the end ({:#x})",
                        data.len()
                    ),
                ));
            }
            entries.push((off, flag));
        }
        let mut items = Vec::with_capacity(count);
        for (i, &(off, flag)) in entries.iter().enumerate() {
            if off == 0 {
                items.push(LibItem {
                    offset: 0,
                    flag,
                    data: Vec::new(),
                });
                continue;
            }
            let end = entries[i + 1..]
                .iter()
                .find(|&&(o, _)| o != 0)
                .map_or(data.len(), |&(o, _)| o as usize);
            if end < off as usize {
                return Err(FormatError::invalid(
                    "u6lib offset",
                    i * es,
                    format!("item {i} offset {off:#x} is after the next item's ({end:#x})"),
                ));
            }
            items.push(LibItem {
                offset: off,
                flag,
                data: data[off as usize..end].to_vec(),
            });
        }
        Ok(U6Lib { width, items })
    }

    /// Number of items with data.
    pub fn non_empty(&self) -> usize {
        self.items.iter().filter(|it| !it.is_empty()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u16_lib_splits_items() {
        // 3 entries (table = 6 bytes): "ab", empty, "cde".
        let mut d = vec![6, 0, 0, 0, 8, 0];
        d.extend_from_slice(b"abcde");
        let lib = U6Lib::parse(&d, EntryWidth::U16).unwrap();
        assert_eq!(lib.items.len(), 3);
        assert_eq!(lib.items[0].data, b"ab");
        assert!(lib.items[1].is_empty());
        assert_eq!(lib.items[2].data, b"cde");
        assert_eq!(lib.non_empty(), 2);
    }

    #[test]
    fn u32_lib_with_empty_first_item_and_flags() {
        // Item 0 empty (like converse.a); item 1 at 12 with flag 0x01; item 2 at 14.
        let mut d = vec![0, 0, 0, 0, 12, 0, 0, 1, 14, 0, 0, 0];
        d.extend_from_slice(b"xyz");
        let lib = U6Lib::parse(&d, EntryWidth::U32).unwrap();
        assert_eq!(lib.items.len(), 3);
        assert!(lib.items[0].is_empty());
        assert_eq!((lib.items[1].offset, lib.items[1].flag), (12, 1));
        assert_eq!(lib.items[1].data, b"xy");
        assert_eq!(lib.items[2].data, b"z");
    }

    #[test]
    fn rejects_malformed_tables() {
        // All zero: count undefined.
        assert!(U6Lib::parse(&[0, 0, 0, 0], EntryWidth::U16).is_err());
        // Offset past the end.
        assert!(U6Lib::parse(&[4, 0, 9, 0], EntryWidth::U16).is_err());
        // Descending offsets.
        assert!(U6Lib::parse(&[6, 0, 8, 0, 7, 0, 1, 2, 3], EntryWidth::U16).is_err());
        // Truncated table entry.
        assert!(U6Lib::parse(&[8, 0, 0], EntryWidth::U32).is_err());
    }
}
