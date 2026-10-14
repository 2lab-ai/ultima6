//! `animdata` — formats.md §4.2 (N: core/tile_manager.cpp:461-490, tick at :351-370). 194 bytes:
//! `u16 BE n` @0, `u16 BE tile_to_animate[32]` @2, `u16 BE first_anim_frame[32]` @66,
//! `u8 and_mask[32]` @130, `u8 shift[32]` @162. Each tick:
//! `tileindex[tile_to_animate[i]] = tileindex[first_anim_frame[i] + ((counter & and_mask[i]) >> shift[i])]`.

use crate::bytes;
use crate::error::{FormatError, Result};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// File size of `animdata`.
pub const ANIMDATA_LEN: usize = 194;
/// Number of animation slots in the file.
pub const ANIM_SLOTS: usize = 32;

/// One animation slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AnimEntry {
    pub tile_to_animate: u16,
    pub first_anim_frame: u16,
    pub and_mask: u8,
    pub shift: u8,
}

impl AnimEntry {
    /// Tile shown for `tile_to_animate` at animation `counter`.
    pub fn frame_tile(&self, counter: u32) -> u16 {
        let step = (counter & u32::from(self.and_mask)) >> self.shift;
        self.first_anim_frame.wrapping_add(step as u16)
    }
}

/// Tile animation table. All 32 slots are kept; only the first `count` are active.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AnimData {
    pub count: u16,
    pub slots: Vec<AnimEntry>,
}

impl AnimData {
    /// Parses `animdata`. Fails unless the file is 194 bytes and `n` ≤ 32.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, ANIMDATA_LEN, "animdata")?;
        let count = bytes::u16_be(data, 0, "animdata count")?;
        if usize::from(count) > ANIM_SLOTS {
            return Err(FormatError::invalid(
                "animdata count",
                0,
                format!("{count} > {ANIM_SLOTS} slots"),
            ));
        }
        let mut slots = Vec::with_capacity(ANIM_SLOTS);
        for i in 0..ANIM_SLOTS {
            slots.push(AnimEntry {
                tile_to_animate: bytes::u16_be(data, 2 + i * 2, "animdata tile_to_animate")?,
                first_anim_frame: bytes::u16_be(data, 66 + i * 2, "animdata first_anim_frame")?,
                and_mask: data[130 + i],
                shift: data[162 + i],
            });
        }
        Ok(AnimData { count, slots })
    }

    /// The active slots (`slots[..count]`).
    pub fn active(&self) -> &[AnimEntry] {
        &self.slots[..usize::from(self.count).min(self.slots.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_be_arrays() {
        let mut d = vec![0u8; ANIMDATA_LEN];
        d[..6].copy_from_slice(&[0x00, 0x02, 0x00, 0x08, 0x00, 0x09]); // n 2, tiles 8, 9
        d[66..70].copy_from_slice(&[0x02, 0x00, 0x02, 0x10]); // 512, 528
        d[130] = 0x06;
        d[162] = 1;
        let a = AnimData::parse(&d).unwrap();
        assert_eq!(a.count, 2);
        assert_eq!(a.active().len(), 2);
        assert_eq!(a.slots[0].tile_to_animate, 8);
        assert_eq!(a.slots[1].first_anim_frame, 528);
        assert_eq!(a.slots[0].frame_tile(0b111), 512 + 3);
        let mut bad = d.clone();
        bad[1] = 33;
        assert!(AnimData::parse(&bad).is_err());
        assert!(AnimData::parse(&d[..193]).is_err());
    }
}
