//! Packed 3-byte world position — formats.md §5 bytes 1-3 (also used by §4.3 schedule records
//! and §6 objlist actor positions).
//!
//! `x = b1 | (b2 & 3) << 8`, `y = b2 >> 2 | (b3 & 0xF) << 6`, `z = b3 >> 4`.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// World position: x, y in 0..1024 (10 bits each), z in 0..16 (0 = surface, 1..=5 dungeons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Position {
    pub x: u16,
    pub y: u16,
    pub z: u8,
}

impl Position {
    /// Unpacks the 3-byte bitfield.
    pub fn from_packed(b: [u8; 3]) -> Self {
        let [b1, b2, b3] = b.map(u16::from);
        Position {
            x: b1 | (b2 & 0x3) << 8,
            y: b2 >> 2 | (b3 & 0xF) << 6,
            z: (b3 >> 4) as u8,
        }
    }

    /// Packs into the 3-byte bitfield (x, y masked to 10 bits, z to 4 bits).
    pub fn to_packed(self) -> [u8; 3] {
        let (x, y, z) = (self.x & 0x3FF, self.y & 0x3FF, u16::from(self.z & 0xF));
        [
            (x & 0xFF) as u8,
            ((x >> 8) | (y & 0x3F) << 2) as u8,
            ((y >> 6) | z << 4) as u8,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_formats_md_examples() {
        // OBJBLKAB `00 45 c4 02` -> (69,177,0); OBJBLKHH `00 84 03 0e` -> (900,896,0).
        let p = Position::from_packed([0x45, 0xc4, 0x02]);
        assert_eq!((p.x, p.y, p.z), (69, 177, 0));
        let p = Position::from_packed([0x84, 0x03, 0x0e]);
        assert_eq!((p.x, p.y, p.z), (900, 896, 0));
        // schedule record 0 bytes 2..5 `33 71 05` -> (307,348,0).
        let p = Position::from_packed([0x33, 0x71, 0x05]);
        assert_eq!((p.x, p.y, p.z), (307, 348, 0));
    }

    #[test]
    fn round_trips_every_field_bit() {
        for p in [
            Position {
                x: 1023,
                y: 1023,
                z: 15,
            },
            Position { x: 0, y: 0, z: 0 },
            Position {
                x: 0x2AA,
                y: 0x155,
                z: 5,
            },
        ] {
            assert_eq!(Position::from_packed(p.to_packed()), p);
        }
        for b in [[0xFF, 0xFF, 0xFF], [0x12, 0x34, 0x56]] {
            assert_eq!(Position::from_packed(b).to_packed(), b);
        }
    }
}
