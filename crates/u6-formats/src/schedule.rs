//! `schedule` — formats.md §4.3 (N: actors/actor_manager.cpp:675-720, actor.cpp:1184-1214).
//!
//! `u16 BE first[256]` @0 (index of each actor's first record), `u16 BE total` @512, then
//! `total` × 5-byte records @514. Record: `[0]` bits0-4 hour, bits5-7 day of week; `[1]`
//! worktype; `[2..5)` x,y,z packed as formats.md §5 bytes 1-3.

use crate::bytes;
use crate::error::{FormatError, Result};
use crate::position::Position;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of actors with a `first` index.
pub const ACTOR_COUNT: usize = 256;
const RECORDS_AT: usize = 514;
const RECORD_LEN: usize = 5;

/// One schedule record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ScheduleEntry {
    /// Byte 0 bits 0-4.
    pub hour: u8,
    /// Byte 0 bits 5-7.
    pub day_of_week: u8,
    pub worktype: u8,
    pub pos: Position,
}

impl ScheduleEntry {
    fn from_bytes(b: &[u8]) -> Self {
        ScheduleEntry {
            hour: b[0] & 0x1F,
            day_of_week: b[0] >> 5,
            worktype: b[1],
            pos: Position::from_packed([b[2], b[3], b[4]]),
        }
    }

    /// The 5-byte record.
    pub fn to_bytes(&self) -> [u8; 5] {
        let p = self.pos.to_packed();
        [
            (self.hour & 0x1F) | self.day_of_week << 5,
            self.worktype,
            p[0],
            p[1],
            p[2],
        ]
    }
}

/// The schedule file as stored: the per-actor `first` table plus all records in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Schedules {
    /// `first[a]` = index into `records` of actor `a`'s first record (256 entries).
    pub first: Vec<u16>,
    pub records: Vec<ScheduleEntry>,
}

impl Schedules {
    /// Parses `schedule`. Fails unless the size is exactly `514 + 5 * total`.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let first = (0..ACTOR_COUNT)
            .map(|a| bytes::u16_be(data, a * 2, "schedule first[]"))
            .collect::<Result<Vec<_>>>()?;
        let total = usize::from(bytes::u16_be(data, 512, "schedule total")?);
        let want = RECORDS_AT + total * RECORD_LEN;
        if data.len() != want {
            return Err(FormatError::bad_length(
                "schedule",
                format!("{want} (514 + 5 * total {total})"),
                data.len(),
            ));
        }
        let records = data[RECORDS_AT..]
            .chunks_exact(RECORD_LEN)
            .map(ScheduleEntry::from_bytes)
            .collect();
        Ok(Schedules { first, records })
    }

    /// Records of actor `a`, by Nuvie's count rule (actor_manager.cpp:686-703): none if
    /// `first[a] > total−1`; up to `total` for actor 255 or when `first[a+1] > total−1`; else up
    /// to `first[a+1]`.
    pub fn for_actor(&self, a: usize) -> &[ScheduleEntry] {
        let total = self.records.len();
        let Some(&start) = self.first.get(a) else {
            return &[];
        };
        let start = usize::from(start);
        if start >= total {
            return &[];
        }
        let end = match self.first.get(a + 1) {
            Some(&next) if a != ACTOR_COUNT - 1 && usize::from(next) < total => usize::from(next),
            _ => total,
        };
        self.records.get(start..end).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(first: &[(usize, u16)], records: &[[u8; 5]]) -> Vec<u8> {
        let mut d = vec![0u8; 514];
        for a in 0..256 {
            // Default: past the end -> no records.
            d[a * 2..a * 2 + 2].copy_from_slice(&0xFFFFu16.to_be_bytes());
        }
        for &(a, f) in first {
            d[a * 2..a * 2 + 2].copy_from_slice(&f.to_be_bytes());
        }
        d[512..514].copy_from_slice(&(records.len() as u16).to_be_bytes());
        records.iter().for_each(|r| d.extend_from_slice(r));
        d
    }

    #[test]
    fn parses_records_and_splits_per_actor() {
        // Made-up record `2e 91 f4 61 19` -> hour 14, day 1, worktype 0x91, (500,600,1).
        let recs = [
            [0x2e, 0x91, 0xf4, 0x61, 0x19],
            [0x2C, 0x01, 0, 0, 0x10],
            [0xF7, 2, 1, 0, 0],
        ];
        let d = build(&[(5, 0), (6, 2), (255, 2)], &recs);
        let s = Schedules::parse(&d).unwrap();
        assert_eq!(s.records.len(), 3);
        let r = s.records[0];
        assert_eq!((r.hour, r.day_of_week, r.worktype), (14, 1, 0x91));
        assert_eq!((r.pos.x, r.pos.y, r.pos.z), (500, 600, 1));
        assert_eq!((s.records[1].hour, s.records[1].day_of_week), (12, 1));
        assert_eq!(s.records[1].pos.z, 1);
        // actor 5: first 0, next (actor 6) = 2 -> 2 records; actor 6 -> 1; actor 255 -> 1.
        assert_eq!(s.for_actor(5).len(), 2);
        assert_eq!(s.for_actor(6).len(), 1);
        assert_eq!(s.for_actor(255).len(), 1);
        assert!(s.for_actor(0).is_empty());
        assert!(s.for_actor(300).is_empty());
        assert_eq!(r.to_bytes(), recs[0]);
        assert_eq!(s.records[2].to_bytes(), recs[2]);
    }

    #[test]
    fn last_actor_with_records_runs_to_total() {
        // Actor 5 first 0, actor 6 has none (first past total): actor 5 gets everything.
        let d = build(&[(5, 0)], &[[1, 0, 0, 0, 0], [2, 0, 0, 0, 0]]);
        assert_eq!(Schedules::parse(&d).unwrap().for_actor(5).len(), 2);
    }

    #[test]
    fn rejects_size_mismatch() {
        let mut d = build(&[], &[[0; 5]]);
        d.push(0);
        assert!(matches!(
            Schedules::parse(&d),
            Err(FormatError::BadLength { .. })
        ));
        assert!(Schedules::parse(&d[..100]).is_err());
    }
}
