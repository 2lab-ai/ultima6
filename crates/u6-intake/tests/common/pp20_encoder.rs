//! Test-only PowerPacker PP20 cruncher (greedy LZ, not size-optimal). It emits the
//! exact token grammar the decruncher reads, so round trips and the Python
//! reference can be checked without game data.

pub const OFFSET_LENS: [u8; 4] = [9, 10, 11, 11];
const WINDOW: usize = 1 << 11;

#[derive(Default)]
struct BitStream {
    bits: Vec<u8>,
}

impl BitStream {
    fn put(&mut self, value: usize, n: u8) {
        for i in (0..n).rev() {
            self.bits.push(((value >> i) & 1) as u8);
        }
    }
}

/// Longest usable match for r[j..] at distance d = offset + 1, honouring per-length offset widths.
fn best_match(r: &[u8], j: usize) -> (usize, usize) {
    let mut best = (0, 0);
    for d in 1..=j.min(WINDOW) {
        let mut len = 0;
        while j + len < r.len() && len < 300 && r[j + len] == r[j + len - d] {
            len += 1;
        }
        // Offset field width depends on length class: 2 -> 9 bits, 3 -> 10, 4+ -> 11 (or 7 for short offsets).
        while len >= 2 {
            let bits = match len {
                2 => OFFSET_LENS[0],
                3 => OFFSET_LENS[1],
                _ => OFFSET_LENS[2],
            };
            if d - 1 < (1 << bits) {
                break;
            }
            len -= 1;
        }
        if len >= 2 && len > best.0 {
            best = (len, d);
        }
    }
    best
}

fn put_literals(bs: &mut BitStream, lits: &[u8]) {
    let mut rem = lits.len() - 1;
    while rem >= 3 {
        bs.put(3, 2);
        rem -= 3;
    }
    bs.put(rem, 2);
    for &b in lits {
        bs.put(usize::from(b), 8);
    }
}

fn put_match(bs: &mut BitStream, len: usize, dist: usize) {
    let offset = dist - 1;
    if len <= 4 {
        let x = len - 2;
        bs.put(x, 2);
        bs.put(offset, OFFSET_LENS[x]);
        return;
    }
    bs.put(3, 2);
    if offset < 128 {
        bs.put(0, 1);
        bs.put(offset, 7);
    } else {
        bs.put(1, 1);
        bs.put(offset, OFFSET_LENS[3]);
    }
    let mut rem = len - 5;
    while rem >= 7 {
        bs.put(7, 3);
        rem -= 7;
    }
    bs.put(rem, 3);
}

pub fn crunch_pp20(plain: &[u8]) -> Vec<u8> {
    // The decruncher fills its output from the end, so compress the reversed bytes.
    let r: Vec<u8> = plain.iter().rev().copied().collect();
    let mut bs = BitStream::default();
    let mut lits: Vec<u8> = Vec::new();
    let mut j = 0;
    while j < r.len() {
        let (len, dist) = best_match(&r, j);
        if len < 2 {
            lits.push(r[j]);
            j += 1;
            continue;
        }
        bs.put(usize::from(lits.is_empty()), 1);
        if !lits.is_empty() {
            put_literals(&mut bs, &lits);
        }
        lits.clear();
        put_match(&mut bs, len, dist);
        j += len;
    }
    if !lits.is_empty() {
        bs.put(0, 1);
        put_literals(&mut bs, &lits);
    }

    // Pad at the front of the read order; the trailer's skip count discards it.
    let skip = (8 - bs.bits.len() % 8) % 8;
    let stream: Vec<u8> = std::iter::repeat_n(0, skip).chain(bs.bits).collect();
    let src_len = stream.len() / 8;
    let mut src = vec![0u8; src_len];
    for (k, &bit) in stream.iter().enumerate() {
        if bit != 0 {
            src[src_len - 1 - (k >> 3)] |= 1 << (k & 7);
        }
    }
    let n = plain.len();
    let mut out = b"PP20".to_vec();
    out.extend_from_slice(&OFFSET_LENS);
    out.extend_from_slice(&src);
    out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8, skip as u8]);
    out
}
