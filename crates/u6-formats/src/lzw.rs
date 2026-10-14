//! U6-LZW codec — formats.md §1 (N: files/u6_lzw.cpp:176-262 decode, :316-349 codeword read).
//!
//! File = `u32 LE` decompressed size (byte 3 always 0), then an LSB-first bitstream of 9..12-bit
//! codes. `0x100` = clear (width back to 9, dictionary emptied, the next code is a literal root),
//! `0x101` = end. The decoder mirrors Nuvie, including the KwKwK case and the width-12 clear the
//! encoder emits when the dictionary fills (`next_free == 0x1000`).

use crate::bytes;
use crate::error::{FormatError, Result};

/// Clear code: reset the dictionary; the following code is a literal byte.
pub const CLEAR: u16 = 0x100;
/// End-of-stream code.
pub const END: u16 = 0x101;
const FIRST_FREE: u16 = 0x102;
const MIN_WIDTH: u32 = 9;
const MAX_WIDTH: u32 = 12;
const DICT_SIZE: usize = 1 << MAX_WIDTH;
const HEADER: usize = 4;

/// Nuvie's validity test (u6_lzw.cpp:89-111): ≥ 6 bytes, header byte 3 = 0, and the first 9-bit
/// code is `0x100` (`buf[4] == 0 && buf[5] & 1 == 1`).
pub fn is_lzw(data: &[u8]) -> bool {
    data.len() >= 6 && data[3] == 0 && data[4] == 0 && data[5] & 1 == 1
}

/// The decompressed size stored in the 4-byte LE header.
pub fn declared_size(data: &[u8]) -> Result<u32> {
    bytes::u32_le(data, 0, "lzw size header")
}

/// Width (9..=12) at which the decoder reads the next code when `next_free` entries exist.
fn width_for(next_free: u16) -> u32 {
    let mut w = MIN_WIDTH;
    while w < MAX_WIDTH && u32::from(next_free) >= 1 << w {
        w += 1;
    }
    w
}

/// LSB-first code reader over the stream that starts after the size header.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    /// Reads `width` bits at the current bit position. The 3-byte window may extend up to 2 bytes
    /// past the end (zero-padded, as formats.md §1 requires); the code's own bits may not.
    fn read(&mut self, width: u32) -> Result<u16> {
        let total_bits = self.data.len() * 8;
        if self.pos + width as usize > total_bits {
            return Err(FormatError::Truncated {
                what: "lzw code (stream ended before the 0x101 end code)",
                offset: HEADER + self.pos / 8,
                needed: 2,
                len: HEADER + self.data.len(),
            });
        }
        let byte = self.pos / 8;
        let get = |i: usize| u32::from(self.data.get(byte + i).copied().unwrap_or(0));
        let window = get(0) | get(1) << 8 | get(2) << 16;
        let code = (window >> (self.pos % 8)) & ((1 << width) - 1);
        self.pos += width as usize;
        Ok(code as u16)
    }

    fn byte_offset(&self) -> usize {
        HEADER + self.pos / 8
    }
}

/// Dictionary of `(prefix code, last byte)` entries; codes below 0x100 are the roots.
struct Dict {
    prefix: Vec<u16>,
    suffix: Vec<u8>,
}

impl Dict {
    fn new() -> Self {
        Dict {
            prefix: vec![0; DICT_SIZE],
            suffix: (0..DICT_SIZE).map(|i| i as u8).collect(),
        }
    }

    /// Appends the string for `code` to `out` and returns its first byte.
    fn emit(&self, code: u16, scratch: &mut Vec<u8>, out: &mut Vec<u8>) -> u8 {
        scratch.clear();
        let mut c = code;
        while c >= FIRST_FREE {
            scratch.push(self.suffix[usize::from(c)]);
            c = self.prefix[usize::from(c)];
        }
        let first = c as u8;
        scratch.push(first);
        out.extend(scratch.iter().rev());
        first
    }
}

/// Decodes one U6-LZW stream (header included). Fails unless the stream starts with `0x100`,
/// ends with `0x101` inside the input, and yields exactly the declared size.
pub fn decode(data: &[u8]) -> Result<Vec<u8>> {
    let declared = declared_size(data)? as usize;
    if !is_lzw(data) {
        return Err(FormatError::invalid(
            "lzw header",
            0,
            "not a U6-LZW stream (needs header byte 3 = 0 and a leading 0x100 code)",
        ));
    }
    let mut bits = BitReader {
        data: &data[HEADER..],
        pos: 0,
    };
    let mut out = Vec::with_capacity(declared.min(1 << 24));
    let mut dict = Dict::new();
    let mut scratch = Vec::new();
    let mut width = MIN_WIDTH;
    let mut next_free = FIRST_FREE;
    let mut prev: u16 = 0;
    loop {
        let at = bits.byte_offset();
        let code = bits.read(width)?;
        match code {
            CLEAR => {
                width = MIN_WIDTH;
                next_free = FIRST_FREE;
                let at = bits.byte_offset();
                let root = bits.read(width)?;
                if root == END {
                    break;
                }
                if root > 0xFF {
                    return Err(FormatError::invalid(
                        "lzw code after clear",
                        at,
                        format!("{root:#x} is not a literal byte"),
                    ));
                }
                out.push(root as u8);
                prev = root;
            }
            END => break,
            c => {
                let first = if c < next_free {
                    dict.emit(c, &mut scratch, &mut out)
                } else if c == next_free {
                    let first = dict.emit(prev, &mut scratch, &mut out);
                    out.push(first);
                    first
                } else {
                    return Err(FormatError::invalid(
                        "lzw code",
                        at,
                        format!("code {c:#x} is beyond next free entry {next_free:#x}"),
                    ));
                };
                if usize::from(next_free) < DICT_SIZE {
                    dict.prefix[usize::from(next_free)] = prev;
                    dict.suffix[usize::from(next_free)] = first;
                    next_free += 1;
                    width = width_for(next_free);
                }
                prev = c;
            }
        }
    }
    if out.len() != declared {
        return Err(FormatError::invalid(
            "lzw output",
            0,
            format!("decoded {} bytes, header declares {declared}", out.len()),
        ));
    }
    Ok(out)
}

/// LSB-first code writer (inverse of [`BitReader`]).
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    nbits: u32,
}

impl BitWriter {
    fn put(&mut self, code: u16, width: u32) {
        self.acc |= u32::from(code) << self.nbits;
        self.nbits += width;
        while self.nbits >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Encodes `input` as a U6-LZW stream that [`decode`] (and Nuvie) reads back: leading `0x100`,
/// greedy LZW, a width-12 `0x100` once a code is emitted with the dictionary full (the timing
/// formats.md §1 measured: the decoder's `next_free` peaks at `0x1000`), then `0x101`.
/// Fails if `input` is too large for the 3-byte size the format allows.
pub fn encode(input: &[u8]) -> Result<Vec<u8>> {
    let size = u32::try_from(input.len())
        .ok()
        .filter(|&n| n <= 0x00FF_FFFF)
        .ok_or_else(|| {
            FormatError::bad_length("lzw input", "at most 0xFFFFFF bytes", input.len())
        })?;
    let mut w = BitWriter {
        out: size.to_le_bytes().to_vec(),
        acc: 0,
        nbits: 0,
    };
    w.put(CLEAR, MIN_WIDTH);
    let Some((&first, rest)) = input.split_first() else {
        w.put(END, MIN_WIDTH);
        return Ok(w.finish());
    };
    let mut dict = std::collections::HashMap::new();
    // Encoder-side next free code; the decoder's count lags by one (it adds on the next code).
    let mut next_free = FIRST_FREE;
    let mut cur = u16::from(first);
    for &k in rest {
        if let Some(&code) = dict.get(&(cur, k)) {
            cur = code;
            continue;
        }
        w.put(cur, width_for(next_free - 1));
        if usize::from(next_free) < DICT_SIZE {
            dict.insert((cur, k), next_free);
            next_free += 1;
        } else {
            w.put(CLEAR, width_for(next_free));
            dict.clear();
            next_free = FIRST_FREE;
        }
        cur = u16::from(k);
    }
    w.put(cur, width_for(next_free - 1));
    w.put(END, width_for(next_free));
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-local LSB-first packer, independent of `BitWriter`.
    fn pack(size: u32, codes: &[(u16, u32)]) -> Vec<u8> {
        let mut out = size.to_le_bytes().to_vec();
        let total: u32 = codes.iter().map(|&(_, w)| w).sum();
        let mut bits = vec![false; total as usize];
        let mut p = 0;
        for &(c, w) in codes {
            for i in 0..w {
                bits[p] = (c >> i) & 1 == 1;
                p += 1;
            }
        }
        for chunk in bits.chunks(8) {
            out.push(
                chunk
                    .iter()
                    .enumerate()
                    .map(|(i, &b)| u8::from(b) << i)
                    .sum(),
            );
        }
        out
    }

    fn c9(codes: &[u16]) -> Vec<(u16, u32)> {
        codes.iter().map(|&c| (c, 9)).collect()
    }

    #[test]
    fn decodes_dictionary_reference() {
        let s = pack(4, &c9(&[0x100, b'A'.into(), b'B'.into(), 0x102, 0x101]));
        assert!(is_lzw(&s));
        assert_eq!(decode(&s).unwrap(), b"ABAB");
    }

    #[test]
    fn decodes_kwkwk() {
        let s = pack(3, &c9(&[0x100, b'A'.into(), 0x102, 0x101]));
        assert_eq!(decode(&s).unwrap(), b"AAA");
    }

    #[test]
    fn clear_resets_dictionary_and_next_code_is_literal() {
        let codes = [
            0x100,
            b'X'.into(),
            0x100,
            b'Y'.into(),
            b'Z'.into(),
            0x102,
            0x101,
        ];
        assert_eq!(decode(&pack(5, &c9(&codes))).unwrap(), b"XYZYZ");
    }

    #[test]
    fn end_code_in_last_byte_reads_past_end() {
        // 8 codes x 9 bits = exactly 9 stream bytes; the final u24 window overhangs by 2.
        let codes = [0x100, 65, 66, 67, 68, 69, 70, 0x101];
        let s = pack(6, &c9(&codes));
        assert_eq!(s.len(), 4 + 9);
        assert_eq!(decode(&s).unwrap(), b"ABCDEF");
    }

    #[test]
    fn width_grows_at_512() {
        // Root + 254 literals add entries 0x102..=0x1ff; after the 254th literal the decoder's
        // next_free is 0x200, so the next code (here: a dictionary ref, then the end code) is read
        // at width 10. A 9-bit read there would desynchronise the stream.
        let mut codes = vec![(0x100, 9), (0, 9)];
        codes.extend((1..=254u16).map(|i| (i, 9)));
        codes.push((0x102, 10)); // = bytes [0, 1]
        codes.push((0x101, 10));
        let s = pack(257, &codes);
        let out = decode(&s).unwrap();
        assert_eq!(out.len(), 257);
        assert_eq!(out[254], 254);
        assert_eq!(&out[255..], &[0, 1]);
    }

    #[test]
    fn rejects_bad_streams() {
        // Not LZW: first code is not 0x100.
        assert!(decode(&pack(1, &c9(&[0x41, 0x101]))).is_err());
        // Header byte 3 nonzero.
        let mut s = pack(1, &c9(&[0x100, 0x41, 0x101]));
        s[3] = 1;
        assert!(!is_lzw(&s));
        // No end code before the input ends.
        let s = pack(2, &c9(&[0x100, 0x41, 0x42]));
        assert!(matches!(decode(&s), Err(FormatError::Truncated { .. })));
        // Size mismatch.
        assert!(matches!(
            decode(&pack(9, &c9(&[0x100, 0x41, 0x101]))),
            Err(FormatError::Invalid {
                what: "lzw output",
                ..
            })
        ));
        // Code beyond next_free.
        assert!(decode(&pack(2, &c9(&[0x100, 0x41, 0x105, 0x101]))).is_err());
        // Non-literal right after clear.
        assert!(decode(&pack(2, &c9(&[0x100, 0x150, 0x101]))).is_err());
        // Too short for a header.
        assert!(decode(&[1, 0]).is_err());
    }

    #[test]
    fn empty_stream_round_trips() {
        let e = encode(&[]).unwrap();
        assert_eq!(decode(&e).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn round_trip_fills_dictionary_with_width_12_clears() {
        // xorshift noise: incompressible enough to fill the 4096-entry dictionary several times.
        let mut x: u32 = 0x2468_ace1;
        let data: Vec<u8> = (0..40_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 24) as u8 & 0x3F
            })
            .collect();
        let e = encode(&data).unwrap();
        assert_eq!(decode(&e).unwrap(), data);
        let text = b"the quick brown fox jumps over the lazy dog ".repeat(300);
        assert_eq!(decode(&encode(&text).unwrap()).unwrap(), text);
    }
}
