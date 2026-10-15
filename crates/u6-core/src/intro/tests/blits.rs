//! The byte-wide blits against the per-pixel forms they replaced (spec 06 O3): seeded random
//! items and rectangles drawn on random bitmaps by both, the whole `Chip` compared after every
//! call. The case number drives the alignments and the plane flags, so every combination comes
//! up; the rest (sizes, positions, clips, regions, masks) is drawn from the seed, biased so
//! that most cases draw something (a floor the tests check). Directed cases follow: every
//! shift pair visibly, and a source that the draw writes over.

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

/// The mask bytes of one plane: random, solid, checkered by row, single bits, solid on the
/// left half of each row, or (one time in 20) empty.
fn mask(rng: &mut Rng, row_bytes: usize, rows: usize) -> Vec<u8> {
    let mut m = vec![0; row_bytes * rows];
    match if rng.chance(5) {
        2
    } else {
        rng.pick(&[0, 1, 3, 4, 5])
    } {
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
    let mut drawn = 0;
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
            n if n < 98 => rng.below(0xd8) as u16,
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
        let stored = (flags & 0x1f).count_ones() as usize;
        let plane_size = row_bytes * rows;
        // Mostly where the whole item fits in its region; otherwise anywhere, its planes cut
        // at the region's end or wrapped.
        let file = rng.pick(&[Region::Files, Region::Blocks]);
        let room = file.size().saturating_sub(10 + (stored + 1) * plane_size);
        let item = if room > 0 && rng.chance(85) {
            Ptr {
                region: file,
                offset: rng.below(room as u64) as u32,
            }
        } else {
            ptr_in(&mut rng, file, 10)
        };
        // The item's planes and its mask, written where they fit (a large one keeps the
        // random bytes already there).
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
                offset: item.offset.saturating_add_signed(rng.range(-3000, 3000)),
            },
            _ => ptr_in(&mut rng, item.region, 20),
        };
        // Mostly overlapping the screen with dy < ymax, so most cases draw; the rest anywhere,
        // the clips that draw nothing included.
        let dx = match rng.below(100) {
            n if n < 90 => 8 * rng.range(-(row_bytes.min(60) as i32), 39) + shift,
            n if n < 96 => 8 * rng.range(-60, 46) + shift,
            _ => (i32::from(rng.word() as i16) & !7) | shift,
        };
        let dy = match rng.below(100) {
            n if n < 90 => rng.range(-(rows.min(60) as i32), 199),
            n if n < 96 => rng.range(-250, 230),
            _ => i32::from(rng.word() as i16),
        };
        let ymax = match rng.below(100) {
            n if n < 90 => dy + rng.range(1, 240),
            n if n < 94 => rng.range(-8, 215),
            n if n < 97 => 200,
            _ => i32::from(rng.pick(&[i16::MAX, i16::MIN, 0, 1, 199, 201])),
        };
        let (dx, dy, ymax) = (dx as i16, dy as i16, ymax as i16);
        // The cases that draw something are counted: most of them must.
        let before = a.bytes(Ptr::at(dst.region)).to_vec();
        mem::blit_masked(&mut a, dst, item, &h, dx, dy, ymax);
        mem::blit_masked_ref(&mut b, dst, item, &h, dx, dy, ymax);
        drawn += usize::from(a.bytes(Ptr::at(dst.region)) != &before[..]);
        assert!(
            a == b,
            "case {case}: item {item:?} {h:?} at ({dx}, {dy}) ymax {ymax} into {dst:?}: \
             first difference at {:?}",
            first_difference(&a, &b)
        );
        // Every 8 cases fresh random bytes in one region, so the bitmaps do not settle (a
        // copy from off the bitmap writes zeros).
        if case % 8 == 7 {
            let r = rng.pick(&REGIONS);
            rng.fill(a.bytes_mut(Ptr::at(r)));
            b.bytes_mut(Ptr::at(r)).copy_from_slice(a.bytes(Ptr::at(r)));
        }
    }
    assert!(
        drawn * 10 >= BLIT_CASES * 6,
        "{drawn} of {BLIT_CASES} cases drew"
    );
}

const COPY_CASES: usize = 3072;

#[test]
fn the_byte_wide_rectangle_copy_draws_what_the_per_pixel_one_drew() {
    let mut rng = Rng(0x0006_0003_c0b7);
    let (mut a, mut b) = chips(&mut rng);
    let mut copied = 0;
    for case in 0..COPY_CASES {
        // Every pair of sx & 7 and dx & 7, copying and ANDing, 24 times over; a quarter of the
        // cases on the aligned fast path (which copies whatever `and` says).
        let (s_shift, d_shift) = ((case % 8) as i32, ((case / 8) % 8) as i32);
        let and = (case / 64) % 2 == 1;
        let fast = rng.chance(25);
        // Mostly a rectangle that lands on the screen, so most cases draw; the rest anywhere,
        // the clips that draw nothing included.
        let on_screen = rng.chance(75);
        let (mut sx, mut dx) = if on_screen {
            (
                8 * rng.range(-6, 39) + s_shift,
                8 * rng.range(-6, 39) + d_shift,
            )
        } else if rng.chance(80) {
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
        let (sy, dy) = if on_screen {
            (rng.range(-30, 199), rng.range(-30, 199))
        } else if rng.chance(80) {
            (rng.range(-230, 230), rng.range(-230, 230))
        } else {
            (i32::from(rng.word() as i16), i32::from(rng.word() as i16))
        };
        let (mut w, h) = if on_screen {
            (rng.range(1, 340), rng.range(1, 230))
        } else {
            (size(&mut rng, 48, 340), size(&mut rng, 24, 210))
        };
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
                offset: src.offset.saturating_add_signed(rng.range(-100, 100)),
            },
            _ => ptr_in(&mut rng, region, 30),
        };
        let (sx, sy, dx, dy, w, h) = (
            sx as i16, sy as i16, dx as i16, dy as i16, w as i16, h as i16,
        );
        // The cases that draw something are counted: most of them must.
        let before = a.bytes(Ptr::at(dst.region)).to_vec();
        mem::copy_rect(&mut a, src, (sx, sy), (w, h), dst, (dx, dy), and);
        mem::copy_rect_ref(&mut b, src, (sx, sy), (w, h), dst, (dx, dy), and);
        copied += usize::from(a.bytes(Ptr::at(dst.region)) != &before[..]);
        assert!(
            a == b,
            "case {case}: {w}×{h} from {src:?} ({sx}, {sy}) to {dst:?} ({dx}, {dy}), and \
             {and}: first difference at {:?}",
            first_difference(&a, &b)
        );
        if case % 8 == 7 {
            let r = rng.pick(&REGIONS);
            rng.fill(a.bytes_mut(Ptr::at(r)));
            b.bytes_mut(Ptr::at(r)).copy_from_slice(a.bytes(Ptr::at(r)));
        }
    }
    assert!(
        copied * 10 >= COPY_CASES * 6,
        "{copied} of {COPY_CASES} cases drew"
    );
}

/// One rectangle copy on a fresh pair of random chips, by both forms: they agree and the
/// destination region changed. Returns the chip.
#[allow(clippy::too_many_arguments)]
fn copy_both(
    seed: u64,
    src: Ptr,
    s: (i16, i16),
    wh: (i16, i16),
    dst: Ptr,
    d: (i16, i16),
    and: bool,
) -> Chip {
    let (mut a, mut b) = chips(&mut Rng(seed));
    let before = a.bytes(Ptr::at(dst.region)).to_vec();
    mem::copy_rect(&mut a, src, s, wh, dst, d, and);
    mem::copy_rect_ref(&mut b, src, s, wh, dst, d, and);
    assert!(
        a == b,
        "{wh:?} from {src:?} {s:?} to {dst:?} {d:?}, and {and}: first difference at {:?}",
        first_difference(&a, &b)
    );
    assert!(
        a.bytes(Ptr::at(dst.region)) != &before[..],
        "{wh:?} from {s:?} to {d:?}, and {and}: nothing drawn"
    );
    a
}

/// A 48×12 item of random planes under a solid mask, `flags` stored, at `item`; then one blit
/// of it at (dx, dy) by both forms: they agree and the destination region changed.
fn blit_both(seed: u64, item: Ptr, flags: u16, dst: Ptr, dx: i16, dy: i16) {
    let mut rng = Rng(seed);
    let (mut a, mut b) = chips(&mut rng);
    let h = ItemHeader {
        x0: 0,
        x1: 0x2f,
        y0: 0,
        y1: 11,
        flags,
    };
    let plane_size = h.row_bytes() * h.rows();
    let mut data = vec![0; (flags & 0x1f).count_ones() as usize * plane_size];
    rng.fill(&mut data);
    data.extend(vec![0xff; plane_size]);
    a.write(item.add(10), &data);
    b.write(item.add(10), &data);
    let before = a.bytes(Ptr::at(dst.region)).to_vec();
    mem::blit_masked(&mut a, dst, item, &h, dx, dy, 200);
    mem::blit_masked_ref(&mut b, dst, item, &h, dx, dy, 200);
    assert!(
        a == b,
        "item {item:?} flags {flags:#x} at ({dx}, {dy}) into {dst:?}: first difference at {:?}",
        first_difference(&a, &b)
    );
    assert!(
        a.bytes(Ptr::at(dst.region)) != &before[..],
        "item at ({dx}, {dy}) into {dst:?}: nothing drawn"
    );
}

#[test]
fn every_shift_pair_copies_and_ands_visibly_as_the_per_pixel_copy_did() {
    let (src, dst) = (Ptr::at(Region::Work), Ptr::at(Region::Screen0));
    // A 37-pixel width starts and ends inside a byte at every shift; x = 40 and 80 are not on
    // a 16-pixel boundary, so none of these takes the fast path.
    for s in 0..8 {
        for d in 0..8 {
            for and in [false, true] {
                let seed = 0x0006_0003_5000 + 16 * s + 2 * d + u64::from(and);
                let (sx, dx) = (40 + s as i16, 80 + d as i16);
                copy_both(seed, src, (sx, 10), (37, 5), dst, (dx, 50), and);
            }
        }
    }
    // The aligned fast path copies whatever `and` says.
    let fast = |and| copy_both(0x0006_0003_5fa5, src, (48, 10), (48, 5), dst, (96, 50), and);
    assert!(fast(true) == fast(false), "the fast path ANDed");
}

#[test]
fn every_dx_shift_blits_visibly_as_the_per_pixel_blit_did() {
    let (item, dst) = (Ptr::at(Region::Files).add(0x100), Ptr::at(Region::Screen0));
    for shift in 0..8 {
        for flags in [0b00000, 0b11111, 0b10110, 0b01001] {
            let seed = 0x0006_0003_b000 + 32 * shift + u64::from(flags);
            blit_both(seed, item, flags, dst, 100 + shift as i16, 20);
        }
    }
}

#[test]
fn a_copy_onto_its_own_source_moved_down_and_right_reads_the_source_as_it_was() {
    // Each row written is the next row read: the copy must read every source row before the
    // rows above it are written over.
    let work = Ptr::at(Region::Work);
    for k in 1..8 {
        for and in [false, true] {
            let seed = 0x0006_0003_a000 + 2 * k + u64::from(and);
            let (sx, sy) = (40, 10);
            copy_both(
                seed,
                work,
                (sx, sy),
                (60, 20),
                work,
                (sx + k as i16, sy + 1),
                and,
            );
        }
    }
}

#[test]
fn a_blit_into_the_items_own_region_reads_the_item_as_it_was() {
    // The bitmap is the item's region from its start: rows 20..23 of plane 0 are the item's
    // header, planes and mask (bytes 800..), and the item's rows land on them as it is drawn.
    let files = Ptr::at(Region::Files);
    for shift in 0..8 {
        for dy in [17, 19, 20, 21] {
            let seed = 0x0006_0003_a100 + 8 * shift + dy as u64;
            blit_both(seed, files.add(800), 0b00011, files, 80 + shift as i16, dy);
        }
    }
}
