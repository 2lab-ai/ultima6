//! `OBJBLK??` object blocks — formats.md §5 (N: save/save_game.cpp:137-185,
//! core/obj_manager.cpp:1496-1540 loadObj).
//!
//! File = `u16 BE count`, then `count` × 8-byte records. Amiga byte order differs from DOS:
//! bytes 4-5 are a **BE** u16 (`obj_n = v & 0x3FF`, `frame = v >> 10`), byte 6 = quality and
//! byte 7 = qty (DOS: LE word, byte 6 qty, byte 7 quality); stackables use `qty = byte6<<8 | byte7`.

use crate::bytes;
use crate::error::{FormatError, Result};
use crate::position::Position;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Size of one object record.
pub const OBJ_RECORD_LEN: usize = 8;

/// Status bit: may be taken (obj.h:36-56).
pub const STATUS_OK_TO_TAKE: u8 = 0x01;
pub const STATUS_INVISIBLE: u8 = 0x02;
pub const STATUS_CHARMED: u8 = 0x04;
pub const STATUS_TEMPORARY: u8 = 0x20;
/// Egg active / object broken.
pub const STATUS_EGG_ACTIVE: u8 = 0x40;
pub const STATUS_LIT: u8 = 0x80;

/// Where an object lives (status bits 3-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ObjLocation {
    /// 0x00: on the map at `pos`.
    Map,
    /// 0x08: inside the container at list index [`ObjRecord::container_index`].
    Container,
    /// 0x10: in actor [`ObjRecord::actor`]'s inventory.
    Inventory,
    /// 0x18: readied by actor [`ObjRecord::actor`].
    Readied,
}

/// One 8-byte object record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ObjRecord {
    /// Byte 0: location bits 3-4 plus the `STATUS_*` bits.
    pub status: u8,
    /// Bytes 1-3 as a bitfield. Only a map position when [`ObjRecord::location`] is `Map`.
    pub pos: Position,
    /// Bytes 4-5 (BE) low 10 bits.
    pub obj_n: u16,
    /// Bytes 4-5 (BE) high 6 bits.
    pub frame: u8,
    /// Byte 6.
    pub quality: u8,
    /// Byte 7.
    pub qty: u8,
}

impl ObjRecord {
    /// Decodes one record (`b` must hold ≥ 8 bytes).
    pub fn from_bytes(b: &[u8]) -> Self {
        let v = u16::from_be_bytes([b[4], b[5]]);
        ObjRecord {
            status: b[0],
            pos: Position::from_packed([b[1], b[2], b[3]]),
            obj_n: v & 0x3FF,
            frame: (v >> 10) as u8,
            quality: b[6],
            qty: b[7],
        }
    }

    /// The 8-byte Amiga record.
    pub fn to_bytes(&self) -> [u8; 8] {
        let p = self.pos.to_packed();
        let v = (self.obj_n & 0x3FF) | u16::from(self.frame & 0x3F) << 10;
        let [v0, v1] = v.to_be_bytes();
        [
            self.status,
            p[0],
            p[1],
            p[2],
            v0,
            v1,
            self.quality,
            self.qty,
        ]
    }

    pub fn location(&self) -> ObjLocation {
        match self.status & 0x18 {
            0x00 => ObjLocation::Map,
            0x08 => ObjLocation::Container,
            0x10 => ObjLocation::Inventory,
            _ => ObjLocation::Readied,
        }
    }

    /// For a container child: list index of its container in the same file,
    /// `x | (y & 0x3F) << 10` (obj_manager.cpp:1479).
    pub fn container_index(&self) -> Option<u16> {
        (self.location() == ObjLocation::Container)
            .then_some(self.pos.x | (self.pos.y & 0x3F) << 10)
    }

    /// For an inventory or readied object: the owning actor number (`x`, obj_manager.cpp:170).
    pub fn actor(&self) -> Option<u16> {
        matches!(
            self.location(),
            ObjLocation::Inventory | ObjLocation::Readied
        )
        .then_some(self.pos.x)
    }

    /// Quantity of a stackable object: the whole BE word `byte6 << 8 | byte7`.
    pub fn stack_qty(&self) -> u16 {
        u16::from(self.quality) << 8 | u16::from(self.qty)
    }
}

/// Nuvie's `is_stackable` list for U6 (obj_manager.cpp:562-600): 55, 56, 63, 65-72, 77, 83, 88,
/// 89, 90 (frame ≠ 1), 91, 92, 128, 129, 337.
pub fn is_stackable(obj_n: u16, frame: u8) -> bool {
    match obj_n {
        90 => frame != 1,
        55 | 56 | 63 | 65..=72 | 77 | 83 | 88 | 89 | 91 | 92 | 128 | 129 | 337 => true,
        _ => false,
    }
}

/// Which block a file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ObjBlockId {
    /// `OBJBLK` + X + Y, X = superchunk column `sx`, Y = row `sy` (A..H).
    Surface { sx: u8, sy: u8 },
    /// `OBJBLK{A..E}I`: dungeon level z = 1..=5.
    Dungeon { z: u8 },
}

impl ObjBlockId {
    /// Parses `OBJBLKxy` (any case).
    pub fn from_name(name: &str) -> Option<Self> {
        let n = name.as_bytes();
        if n.len() != 8 || !n[..6].eq_ignore_ascii_case(b"OBJBLK") {
            return None;
        }
        let (x, y) = (n[6].to_ascii_uppercase(), n[7].to_ascii_uppercase());
        match (x, y) {
            (b'A'..=b'E', b'I') => Some(ObjBlockId::Dungeon { z: x - b'A' + 1 }),
            (b'A'..=b'H', b'A'..=b'H') => Some(ObjBlockId::Surface {
                sx: x - b'A',
                sy: y - b'A',
            }),
            _ => None,
        }
    }

    /// The Amiga (uppercase) file name.
    pub fn file_name(&self) -> String {
        let (x, y) = match *self {
            ObjBlockId::Surface { sx, sy } => (b'A' + sx, b'A' + sy),
            ObjBlockId::Dungeon { z } => (b'A' + z - 1, b'I'),
        };
        format!("OBJBLK{}{}", char::from(x), char::from(y))
    }

    /// Surface superchunk index `s = sy*8 + sx` (same `s` as the map).
    pub fn surface_index(&self) -> Option<usize> {
        match *self {
            ObjBlockId::Surface { sx, sy } => Some(usize::from(sy) * 8 + usize::from(sx)),
            ObjBlockId::Dungeon { .. } => None,
        }
    }

    /// All 69 blocks: surface `s = 0..64`, then dungeons z = 1..=5 (Nuvie's load order).
    pub fn all() -> impl Iterator<Item = ObjBlockId> {
        let surface = (0..64u8).map(|s| ObjBlockId::Surface {
            sx: s % 8,
            sy: s / 8,
        });
        surface.chain((1..=5).map(|z| ObjBlockId::Dungeon { z }))
    }
}

/// One parsed `OBJBLK??` file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ObjBlock {
    pub objects: Vec<ObjRecord>,
}

impl ObjBlock {
    /// Parses an object block. Fails unless `2 + 8 * count == len`.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let count = usize::from(bytes::u16_be(data, 0, "objblk count")?);
        let want = 2 + count * OBJ_RECORD_LEN;
        if data.len() != want {
            return Err(FormatError::bad_length(
                "objblk",
                format!("{want} (2 + 8 * count {count})"),
                data.len(),
            ));
        }
        let objects = data[2..]
            .chunks_exact(OBJ_RECORD_LEN)
            .map(ObjRecord::from_bytes)
            .collect();
        Ok(ObjBlock { objects })
    }

    /// The Amiga file bytes (BE count + records).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = (self.objects.len() as u16).to_be_bytes().to_vec();
        self.objects
            .iter()
            .for_each(|o| out.extend_from_slice(&o.to_bytes()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_amiga_byte_order() {
        // Made-up records: on the map `00 f4 61 09 | 00 c8 | 00 00` -> (500,600,0) obj 200 frame
        // 0; gold in a container `08 06 00 00 | 00 58 | 01 90` -> container 6, obj 88, qty 400;
        // a sign `01 .. | 05 4c | 09 00`: obj 332 frame 1, quality 9.
        let mut d = vec![0x00, 0x03];
        d.extend_from_slice(&[0x00, 0xf4, 0x61, 0x09, 0x00, 0xc8, 0x00, 0x00]);
        d.extend_from_slice(&[0x08, 0x06, 0x00, 0x00, 0x00, 0x58, 0x01, 0x90]);
        d.extend_from_slice(&[0x01, 0x20, 0x00, 0x00, 0x05, 0x4c, 0x09, 0x00]);
        let b = ObjBlock::parse(&d).unwrap();
        let [a, g, s] = [b.objects[0], b.objects[1], b.objects[2]];
        assert_eq!(
            (a.pos.x, a.pos.y, a.pos.z, a.obj_n, a.frame),
            (500, 600, 0, 200, 0)
        );
        assert_eq!(a.location(), ObjLocation::Map);
        assert_eq!(a.container_index(), None);
        assert_eq!(g.location(), ObjLocation::Container);
        assert_eq!(g.container_index(), Some(6));
        assert_eq!((g.obj_n, g.stack_qty()), (88, 400));
        assert!(is_stackable(g.obj_n, g.frame));
        assert_eq!((s.obj_n, s.frame, s.quality, s.qty), (332, 1, 9, 0));
        assert_eq!(s.status & STATUS_OK_TO_TAKE, 1);
        assert_eq!(b.to_bytes(), d);
    }

    #[test]
    fn inventory_owner_and_container_high_bits() {
        let inv = ObjRecord::from_bytes(&[0x10, 0x07, 0, 0, 0, 1, 0, 0]);
        assert_eq!(
            (inv.location(), inv.actor()),
            (ObjLocation::Inventory, Some(7))
        );
        let rd = ObjRecord::from_bytes(&[0x18, 0x01, 0, 0, 0, 1, 0, 0]);
        assert_eq!((rd.location(), rd.actor()), (ObjLocation::Readied, Some(1)));
        // container index uses y's low 6 bits: y = 1 -> +1024.
        let c = ObjRecord::from_bytes(&[0x08, 0x02, 0x04, 0, 0, 1, 0, 0]);
        assert_eq!(c.container_index(), Some(1024 + 2));
    }

    #[test]
    fn stackable_list() {
        assert!(is_stackable(90, 0) && !is_stackable(90, 1));
        assert!(is_stackable(65, 0) && is_stackable(72, 3) && is_stackable(337, 0));
        assert!(!is_stackable(64, 0) && !is_stackable(332, 0));
    }

    #[test]
    fn rejects_count_mismatch() {
        assert!(ObjBlock::parse(&[0x00, 0x01, 0, 0, 0]).is_err());
        assert!(ObjBlock::parse(&[0x00]).is_err());
        assert_eq!(ObjBlock::parse(&[0, 0]).unwrap().objects.len(), 0);
    }

    #[test]
    fn block_names() {
        let ab = ObjBlockId::from_name("OBJBLKAB").unwrap();
        assert_eq!(ab, ObjBlockId::Surface { sx: 0, sy: 1 });
        assert_eq!(ab.surface_index(), Some(8));
        assert_eq!(
            ObjBlockId::from_name("objblkei"),
            Some(ObjBlockId::Dungeon { z: 5 })
        );
        assert_eq!(ObjBlockId::from_name("OBJBLKFI"), None);
        assert_eq!(ObjBlockId::from_name("OBJBLKAJ"), None);
        assert_eq!(ObjBlockId::from_name("OBJLIST"), None);
        let all: Vec<_> = ObjBlockId::all().collect();
        assert_eq!(all.len(), 69);
        for (s, id) in all.iter().take(64).enumerate() {
            assert_eq!(id.surface_index(), Some(s));
            assert_eq!(ObjBlockId::from_name(&id.file_name()), Some(*id));
        }
        assert_eq!(all[64].file_name(), "OBJBLKAI");
    }
}
