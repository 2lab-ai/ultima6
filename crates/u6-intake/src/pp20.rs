//! PowerPacker "PP20" decruncher — port of `web/src/intake/pp20.ts` (itself a port of
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

pub fn decrunch_pp20(data: &[u8]) -> Result<Vec<u8>, Error> {
    if !is_pp20(data) {
        return Err(Error::new("not a PP20 file"));
    }
    if data.len() < 12 {
        return Err(Error::new("PP20 file truncated"));
    }
    let n = data.len();
    let offset_lens = &data[4..8];
    let mut bits = Bits {
        src: &data[8..n - 4],
        cursor: 0,
    };
    let dest_len = (usize::from(data[n - 4]) << 16)
        | (usize::from(data[n - 3]) << 8)
        | usize::from(data[n - 2]);
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
    Ok(out)
}
