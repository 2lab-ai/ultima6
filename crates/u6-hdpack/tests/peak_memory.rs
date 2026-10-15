//! The reader's peak heap (the page holds a pack in a browser tab): a counting allocator over
//! the system one, around `read_pack` of a pack of two full atlases, and of a pack with a large
//! entry the index does not name. One test, so nothing else allocates while it measures.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use u6_hdpack::{ATLAS_CELLS, Generator, ImageId, PackImage, Rgba, TILE_HD, read_pack, write_pack};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn the_reader_holds_one_atlas_at_a_time() {
    let per = (ATLAS_CELLS * ATLAS_CELLS) as u16;
    let images: Vec<PackImage> = (0..2 * per)
        .map(|r| {
            let mut img = Rgba::new(TILE_HD, TILE_HD);
            for (i, p) in img.pixels.chunks_exact_mut(4).enumerate() {
                p.copy_from_slice(&[(i as u16 ^ r) as u8, (r >> 3) as u8, i as u8 / 3, 255]);
            }
            PackImage {
                id: ImageId::Tile(r),
                hash: u64::from(r),
                image: img,
            }
        })
        .collect();
    let g = Generator {
        name: "test".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    };
    let pack = write_pack(&images, &g, true).unwrap();
    let output: usize = images.iter().map(|p| p.image.pixels.len()).sum();
    drop(images);
    let atlas = (ATLAS_CELLS * TILE_HD) as usize * (ATLAS_CELLS * TILE_HD) as usize * 4;
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let read = read_pack(&pack).unwrap();
    let peak = PEAK.load(Ordering::SeqCst) - base;
    assert_eq!(read.images.len(), 2 * usize::from(per));
    println!(
        "peak {peak} bytes over the pack's {} bytes: output {output}, one atlas {atlas}",
        pack.len()
    );
    // The images, one atlas and one entry's bytes at a time (measured: 50.5 MB); holding both
    // atlases at once, as the first reader did, is 67 MB.
    let largest = u6_intake::zip::read_zip(&pack, |_| true)
        .unwrap()
        .iter()
        .map(|(_, b)| b.len())
        .max()
        .unwrap();
    let budget = output + atlas + 2 * largest + (1 << 20);
    assert!(peak <= budget, "peak {peak} > budget {budget}");
    drop(read);

    // An entry the index does not name is never inflated: 100 MB of zeros, deflated, declared
    // honestly, beside the pack's own entries.
    let mut files = u6_intake::zip::read_zip(&pack, |_| true).unwrap();
    let zeros = vec![0u8; 100_000_000];
    let bomb = miniz_oxide::deflate::compress_to_vec(&zeros, 6);
    let crc = crc32fast::hash(&zeros);
    drop(zeros);
    files.push(("portrait/a-1.webp".into(), Vec::new()));
    let zip = deflate_last(&files, &bomb, 100_000_000, crc);
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let r = read_pack(&zip);
    let peak = PEAK.load(Ordering::SeqCst) - base;
    assert!(r.is_err());
    println!("peak {peak} bytes refusing a pack with a 100 MB entry it does not name");
    assert!(peak < 4 << 20, "peak {peak}");
}

/// A zip of `files` stored, the last one replaced by `deflated` declaring `full` bytes.
fn deflate_last(files: &[(String, Vec<u8>)], deflated: &[u8], full: u32, crc: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (i, (name, data)) in files.iter().enumerate() {
        let last = i + 1 == files.len();
        let (body, method, size, crc) = if last {
            (deflated, 8u16, full, crc)
        } else {
            (data.as_slice(), 0, data.len() as u32, crc32fast::hash(data))
        };
        let head = |o: &mut Vec<u8>| {
            for v in [20u16, 0, method, 0, 0x21] {
                o.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, body.len() as u32, size] {
                o.extend_from_slice(&v.to_le_bytes());
            }
            o.extend_from_slice(&(name.len() as u16).to_le_bytes());
            o.extend_from_slice(&0u16.to_le_bytes());
        };
        let at = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        head(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        head(&mut central);
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    let n = files.len() as u16;
    for v in [0u16, 0, n, n] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
