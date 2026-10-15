//! The byte-wide blits against the per-pixel forms they replaced (spec 06 O3): seeded random
//! items and rectangles drawn on random bitmaps by both, the whole `Chip` compared after every
//! call. The case number drives the alignments and the plane flags, so every combination comes
//! up; the rest (sizes, positions, clips, regions, masks) is drawn from the seed.

use crate::intro::mem::{self, Chip, ItemHeader, Ptr, Region};

const REGIONS: [Region; 7] = [
    Region::Screen0,
    Region::Screen1,
    Region::Work,
    Region::Composite,
    Region::Spare,
    Region::Files,
    Region::Blocks,
];

/// The bitmaps the intro draws into.
const BITMAPS: [Region; 5] = [
    Region::Screen0,
    Region::Screen1,
    Region::Work,
    Region::Composite,
    Region::Spare,
];

/// splitmix64: the tests' only source of randomness.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 0..n.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// lo..=hi.
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }

    /// True `pct` times in 100.
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }

    fn word(&mut self) -> u16 {
        self.next() as u16
    }

    fn pick<T: Copy>(&mut self, of: &[T]) -> T {
        of[self.below(of.len() as u64) as usize]
    }

    fn fill(&mut self, b: &mut [u8]) {
        for c in b.chunks_mut(8) {
            let v = self.next().to_le_bytes();
            c.copy_from_slice(&v[..c.len()]);
        }
    }
}

/// Two equal chips of random bytes in every region.
fn chips(rng: &mut Rng) -> (Chip, Chip) {
    let mut a = Chip::new();
    for r in REGIONS {
        rng.fill(a.bytes_mut(Ptr::at(r)));
    }
    let b = a.clone();
    (a, b)
}

/// The first byte where the chips differ, as (region, offset).
fn first_difference(a: &Chip, b: &Chip) -> Option<(Region, usize)> {
    REGIONS.iter().find_map(|&r| {
        let (x, y) = (a.bytes(Ptr::at(r)), b.bytes(Ptr::at(r)));
        x.iter().zip(y).position(|(p, q)| p != q).map(|i| (r, i))
    })
}

/// A pointer into `region`: mostly its start or a random place in it, sometimes near or past
/// its end, or just below 2^32 (an offset that `Ptr::add` wraps back to the start).
fn ptr_in(rng: &mut Rng, region: Region, start_pct: u64) -> Ptr {
    let size = region.size() as u32;
    let offset = match rng.below(100) {
        n if n < start_pct => 0,
        n if n < 90 => rng.below(u64::from(size)) as u32,
        n if n < 94 => size - rng.below(64) as u32,
        n if n < 97 => size + rng.below(64) as u32,
        _ => u32::MAX - rng.below(16) as u32,
    };
    Ptr { region, offset }
}

/// The mask bytes of one plane: random, solid, empty, checkered by row, single bits or
/// solid on the left half of each row.
fn mask(rng: &mut Rng, row_bytes: usize, rows: usize) -> Vec<u8> {
    let mut m = vec![0; row_bytes * rows];
    match rng.below(6) {
        0 => rng.fill(&mut m),
        1 => m.fill(0xff),
        2 => {}
        3 => {
            for (i, b) in m.iter_mut().enumerate() {
                *b = if (i / row_bytes).is_multiple_of(2) {
                    0xaa
                } else {
                    0x55
                };
            }
        }
        4 => {
            for b in &mut m {
                *b = 1 << rng.below(8);
            }
        }
        _ => {
            for (i, b) in m.iter_mut().enumerate() {
                if i % row_bytes < row_bytes / 2 {
                    *b = 0xff;
                }
            }
        }
    }
    m
}

/// Mostly small values, then wide ones, then the word's extremes.
fn size(rng: &mut Rng, small: i32, wide: i32) -> i32 {
    match rng.below(100) {
        n if n < 55 => rng.range(0, small),
        n if n < 85 => rng.range(0, wide),
        n if n < 92 => rng.range(-40, 0),
        _ => i32::from(rng.pick(&[i16::MAX, i16::MIN, 0x7ff0, -16, 0x140, 0xc8])),
    }
}

const BLIT_CASES: usize = 4096;

#[test]
fn the_byte_wide_masked_blit_draws_what_the_per_pixel_one_drew() {
    let mut rng = Rng(0x0006_0003_b117);
    let (mut a, mut b) = chips(&mut rng);
    for case in 0..BLIT_CASES {
        // Every dx & 7 with every combination of the five plane flags, 16 times over.
        let shift = (case % 8) as i32;
        let low_flags = ((case / 8) % 32) as u16;
        let flags = if rng.chance(25) {
            low_flags | (rng.word() & !0x1f)
        } else {
            low_flags
        };
        // A huge item gets few rows (so the reference stays quick); rows = 0 at y1 = 0xffff.
        let x1 = match rng.below(100) {
            n if n < 60 => rng.below(0x40) as u16,
            n if n < 93 => rng.below(0x160) as u16,
            _ => 0xfff0 | rng.below(16) as u16,
        };
        let y1 = match rng.below(100) {
            _ if x1 >= 0xfff0 => rng.below(4) as u16,
            n if n < 60 => rng.below(41) as u16,
            n if n < 95 => rng.below(0xd8) as u16,
            _ => 0xffff,
        };
        let h = ItemHeader {
            x0: rng.word() as i16,
            x1,
            y0: rng.word() as i16,
            y1,
            flags,
        };
        let (row_bytes, rows) = (h.row_bytes(), h.rows());
        let file = rng.pick(&[Region::Files, Region::Blocks]);
        let item = ptr_in(&mut rng, file, 10);
        // The item's planes and its mask, written where they fit (a large one keeps the
        // random bytes already there).
        let stored = (flags & 0x1f).count_ones() as usize;
        let plane_size = row_bytes * rows;
        if (stored + 1) * plane_size <= 0x1_0000 {
            let mut data = vec![0; stored * plane_size];
            rng.fill(&mut data);
            data.extend(mask(&mut rng, row_bytes, rows));
            a.write(item.add(10), &data);
            b.write(item.add(10), &data);
        }
        // Mostly a bitmap's start; sometimes a place inside one (planes past its end), or the
        // item's own region, near the item or anywhere in it.
        let bitmap = rng.pick(&BITMAPS);
        let dst = match rng.below(100) {
            n if n < 70 => Ptr::at(bitmap),
            n if n < 80 => ptr_in(&mut rng, bitmap, 0),
            n if n < 90 => Ptr {
                region: item.region,
                offset: item.offset.wrapping_add(rng.range(-3000, 3000) as u32),
            },
            _ => ptr_in(&mut rng, item.region, 20),
        };
        let dx = if rng.chance(93) {
            8 * rng.range(-60, 46) + shift
        } else {
            (i32::from(rng.word() as i16) & !7) | shift
        };
        let dy = if rng.chance(93) {
            rng.range(-250, 230)
        } else {
            i32::from(rng.word() as i16)
        };
        let ymax = match rng.below(100) {
            n if n < 60 => rng.range(-8, 215),
            n if n < 85 => 200,
            _ => i32::from(rng.pick(&[i16::MAX, i16::MIN, 0, 1, 199, 201])),
        };
        let (dx, dy, ymax) = (dx as i16, dy as i16, ymax as i16);
        mem::blit_masked(&mut a, dst, item, &h, dx, dy, ymax);
        mem::blit_masked_ref(&mut b, dst, item, &h, dx, dy, ymax);
        assert!(
            a == b,
            "case {case}: item {item:?} {h:?} at ({dx}, {dy}) ymax {ymax} into {dst:?}: \
             first difference at {:?}",
            first_difference(&a, &b)
        );
        // Now and then fresh random bytes in one region, so the bitmaps do not settle.
        if case % 64 == 63 {
            let r = rng.pick(&REGIONS);
            rng.fill(a.bytes_mut(Ptr::at(r)));
            b.bytes_mut(Ptr::at(r)).copy_from_slice(a.bytes(Ptr::at(r)));
        }
    }
}

const COPY_CASES: usize = 3072;

#[test]
fn the_byte_wide_rectangle_copy_draws_what_the_per_pixel_one_drew() {
    let mut rng = Rng(0x0006_0003_c0b7);
    let (mut a, mut b) = chips(&mut rng);
    for case in 0..COPY_CASES {
        // Every pair of sx & 7 and dx & 7, copying and ANDing, 24 times over; a quarter of the
        // cases on the aligned fast path (which copies whatever `and` says).
        let (s_shift, d_shift) = ((case % 8) as i32, ((case / 8) % 8) as i32);
        let and = (case / 64) % 2 == 1;
        let fast = rng.chance(25);
        let (mut sx, mut dx) = if rng.chance(93) {
            (
                8 * rng.range(-45, 45) + s_shift,
                8 * rng.range(-45, 45) + d_shift,
            )
        } else {
            (
                (i32::from(rng.word() as i16) & !7) | s_shift,
                (i32::from(rng.word() as i16) & !7) | d_shift,
            )
        };
        let sy = if rng.chance(93) {
            rng.range(-230, 230)
        } else {
            i32::from(rng.word() as i16)
        };
        let dy = if rng.chance(93) {
            rng.range(-230, 230)
        } else {
            i32::from(rng.word() as i16)
        };
        let mut w = size(&mut rng, 48, 340);
        let h = size(&mut rng, 24, 210);
        if fast {
            (sx, dx, w) = (sx & !15, dx & !15, w & !15);
        }
        // Mostly two bitmaps; sometimes one bitmap into itself (overlapping), other regions,
        // or places inside a region (planes past its end).
        let (bitmap, region) = (rng.pick(&BITMAPS), rng.pick(&REGIONS));
        let src = if rng.chance(85) {
            Ptr::at(bitmap)
        } else {
            ptr_in(&mut rng, region, 30)
        };
        let (bitmap, region) = (rng.pick(&BITMAPS), rng.pick(&REGIONS));
        let dst = match rng.below(100) {
            n if n < 60 => Ptr::at(bitmap),
            n if n < 80 => Ptr {
                region: src.region,
                offset: src.offset.wrapping_add(rng.range(-100, 100) as u32),
            },
            _ => ptr_in(&mut rng, region, 30),
        };
        let (sx, sy, dx, dy, w, h) = (
            sx as i16, sy as i16, dx as i16, dy as i16, w as i16, h as i16,
        );
        mem::copy_rect(&mut a, src, (sx, sy), (w, h), dst, (dx, dy), and);
        mem::copy_rect_ref(&mut b, src, (sx, sy), (w, h), dst, (dx, dy), and);
        assert!(
            a == b,
            "case {case}: {w}×{h} from {src:?} ({sx}, {sy}) to {dst:?} ({dx}, {dy}), and \
             {and}: first difference at {:?}",
            first_difference(&a, &b)
        );
        if case % 64 == 63 {
            let r = rng.pick(&REGIONS);
            rng.fill(a.bytes_mut(Ptr::at(r)));
            b.bytes_mut(Ptr::at(r)).copy_from_slice(a.bytes(Ptr::at(r)));
        }
    }
}
