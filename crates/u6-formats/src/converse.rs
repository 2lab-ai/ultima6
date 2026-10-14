//! `converse.a` / `converse.b` — formats.md §2 "Converse item" (N: core/converse.cpp:125-145,
//! 657-693). U6Lib with u32 LE offsets; one item per NPC script. If an item's first 4 bytes are
//! 0 the script is `item[4..]` uncompressed, otherwise the whole item is one U6-LZW stream.
//! This module only splits and unwraps the scripts; the bytecode (multi-byte operands
//! LSB-first, converse.cpp:696-728) is not interpreted here.

use crate::error::{FormatError, Result};
use crate::lzw;
use crate::u6lib::{EntryWidth, U6Lib};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// How a script item was stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ScriptStorage {
    /// First 4 bytes 0, script = `item[4..]`.
    Raw,
    /// Whole item is a U6-LZW stream.
    Lzw,
}

/// One NPC script.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConverseScript {
    pub storage: ScriptStorage,
    /// The script bytecode, uncompressed.
    pub bytecode: Vec<u8>,
}

/// A converse library: `scripts[i]` is item `i` (`None` = empty item).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConverseLib {
    pub scripts: Vec<Option<ConverseScript>>,
}

impl ConverseLib {
    /// Splits `converse.a`/`.b` and unwraps every script.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let lib = U6Lib::parse(data, EntryWidth::U32)?;
        let mut scripts = Vec::with_capacity(lib.items.len());
        for (i, it) in lib.items.iter().enumerate() {
            if it.is_empty() {
                scripts.push(None);
                continue;
            }
            let script = match it.data.get(..4) {
                Some([0, 0, 0, 0]) => ConverseScript {
                    storage: ScriptStorage::Raw,
                    bytecode: it.data[4..].to_vec(),
                },
                _ => ConverseScript {
                    storage: ScriptStorage::Lzw,
                    bytecode: lzw::decode(&it.data)
                        .map_err(|e| FormatError::in_item("converse", i, it.offset as usize, e))?,
                },
            };
            scripts.push(Some(script));
        }
        Ok(ConverseLib { scripts })
    }

    /// Number of non-empty scripts.
    pub fn non_empty(&self) -> usize {
        self.scripts.iter().flatten().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(items: &[Option<Vec<u8>>]) -> Vec<u8> {
        let table = items.len() * 4;
        let mut body = Vec::new();
        let mut d = Vec::new();
        for it in items {
            let off = match it {
                Some(b) => {
                    let o = table + body.len();
                    body.extend_from_slice(b);
                    o as u32
                }
                None => 0,
            };
            d.extend_from_slice(&off.to_le_bytes());
        }
        d.extend(body);
        d
    }

    #[test]
    fn splits_raw_and_lzw_scripts() {
        let raw = [vec![0, 0, 0, 0], b"\xff\x01Hi".to_vec()].concat();
        let packed = lzw::encode(b"\xffhello hello hello").unwrap();
        let d = lib(&[None, Some(packed), Some(raw)]);
        let c = ConverseLib::parse(&d).unwrap();
        assert_eq!(c.scripts.len(), 3);
        assert!(c.scripts[0].is_none());
        let s1 = c.scripts[1].as_ref().unwrap();
        assert_eq!(
            (s1.storage, s1.bytecode.as_slice()),
            (ScriptStorage::Lzw, &b"\xffhello hello hello"[..])
        );
        let s2 = c.scripts[2].as_ref().unwrap();
        assert_eq!(
            (s2.storage, s2.bytecode.as_slice()),
            (ScriptStorage::Raw, &b"\xff\x01Hi"[..])
        );
        assert_eq!(c.non_empty(), 2);
    }

    #[test]
    fn bad_lzw_item_reports_index() {
        let d = lib(&[Some(vec![1, 2, 3, 4, 5, 6])]);
        assert!(matches!(
            ConverseLib::parse(&d),
            Err(FormatError::InItem {
                index: 0,
                offset: 4,
                ..
            })
        ));
    }
}
