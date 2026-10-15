//! PowerPacker "PP20" decruncher — port of the former TypeScript intake (itself a port of
//! `tools/pp20.py`, algorithm per Stuart Caie's ppdepack).
//!
//! File: "PP20", 4 offset-length bytes, crunched stream, then 4 trailer bytes:
//! 24-bit big-endian output length and the number of bits to skip first.
//! The stream is consumed from its last byte backwards, each byte LSB first,
//! and the output is written from its end backwards.

use crate::Error;

pub fn is_pp20(data: &[u8]) -> bool {
    data.starts_with(b"PP20")
}

/// Reads the crunched stream from its last byte backwards, LSB first.
struct Bits<'a> {
    src: &'a [u8],
    cursor: usize,
}

impl Bits<'_> {
    fn ensure(&self, count: usize) -> Result<(), Error> {
        if self.cursor + count > self.src.len() * 8 {
            return Err(Error::new("PP20: out of source bits"));
        }
        Ok(())
    }

    /// `count` may exceed 64 on hostile input (offset widths are file bytes);
    /// the value saturates, which still trips the caller's overflow checks.
    fn read(&mut self, count: usize) -> Result<u64, Error> {
        self.ensure(count)?;
        let mut v = 0u64;
        for _ in 0..count {
            let byte = self.src[self.src.len() - 1 - (self.cursor >> 3)];
            v = v
                .saturating_mul(2)
                .saturating_add(u64::from((byte >> (self.cursor & 7)) & 1));
            self.cursor += 1;
        }
        Ok(v)
    }
}

/// Output bytes per stream byte a decrunch can reach at most: 56/3. Each pass of the loop
/// below reads a flag bit, maybe a literal run (8 bits a byte), then a match. A match of class
/// 0-2 reads at least its 2 class bits for at most 4 bytes; a class-3 match reads at least
/// 3 + 3g bits (class, offset-width bit, `g` length groups) for at most 7g + 4 bytes. So no
/// output byte costs less than 3/7 of a bit: a stream of `s` bytes yields at most 56s/3, and a
/// file that declares more runs out of bits before it is done.
pub const MAX_EXPANSION: (u64, u64) = (56, 3);

/// The output length the trailer declares (24 bits), or `None` when `data` is not a PP20 file
/// long enough to hold its header and trailer.
pub fn pp20_declared_len(data: &[u8]) -> Option<usize> {
    let n = data.len();
    (is_pp20(data) && n >= 12).then(|| {
        (usize::from(data[n - 4]) << 16)
            | (usize::from(data[n - 3]) << 8)
            | usize::from(data[n - 2])
    })
}

pub fn decrunch_pp20(data: &[u8]) -> Result<Vec<u8>, Error> {
    // The declared length is 24 bits, so no limit refuses it here.
    decrunch_pp20_within(data, usize::MAX).map(Option::unwrap_or_default)
}

/// As [`decrunch_pp20`], but a file that declares more than `limit` bytes is refused before
/// anything is allocated: `Ok(None)` (the intake passes what is left of its budget). A file
/// that declares more than its stream can hold ([`MAX_EXPANSION`]) is an error, also before
/// anything is allocated.
pub fn decrunch_pp20_within(data: &[u8], limit: usize) -> Result<Option<Vec<u8>>, Error> {
    if !is_pp20(data) {
        return Err(Error::new("not a PP20 file"));
    }
    let Some(dest_len) = pp20_declared_len(data) else {
        return Err(Error::new("PP20 file truncated"));
    };
    let n = data.len();
    // u64: 56 times a stream of hundreds of MiB passes a 32-bit usize (wasm32).
    let (num, den) = MAX_EXPANSION;
    if dest_len as u64 * den > (n as u64 - 12) * num {
        return Err(Error::new(format!(
            "PP20: declares {dest_len} bytes, more than its stream can hold"
        )));
    }
    if dest_len > limit {
        return Ok(None);
    }
    let offset_lens = &data[4..8];
    let mut bits = Bits {
        src: &data[8..n - 4],
        cursor: 0,
    };
    let skip_bits = usize::from(data[n - 1]);
    let mut out = vec![0u8; dest_len];
    let mut pos = dest_len;

    bits.ensure(skip_bits)?;
    bits.cursor += skip_bits;
    while pos > 0 {
        if bits.read(1)? == 0 {
            let mut todo = 1usize;
            loop {
                let x = bits.read(2)? as usize;
                todo += x;
                if x != 3 {
                    break;
                }
            }
            if todo > pos {
                return Err(Error::new("PP20: literal run overflow"));
            }
            for _ in 0..todo {
                pos -= 1;
                out[pos] = bits.read(8)? as u8;
            }
            if pos == 0 {
                break;
            }
        }
        let mut x = bits.read(2)? as usize;
        let mut off_bits = usize::from(offset_lens[x]);
        let mut todo = x + 2;
        let offset = if x == 3 {
            if bits.read(1)? == 0 {
                off_bits = 7;
            }
            let offset = bits.read(off_bits)?;
            loop {
                x = bits.read(3)? as usize;
                todo += x;
                if x != 7 {
                    break;
                }
            }
            offset
        } else {
            bits.read(off_bits)?
        };
        if (pos as u64).saturating_add(offset) >= dest_len as u64 {
            return Err(Error::new("PP20: match overflow"));
        }
        if todo > pos {
            return Err(Error::new("PP20: match run overflow"));
        }
        // pos + offset < dest_len, so every source index below is in range.
        let offset = offset as usize;
        for _ in 0..todo {
            pos -= 1;
            out[pos] = out[pos + 1 + offset];
        }
    }
    Ok(Some(out))
}
