//! Spec 07 H2's memory budget for a loaded HD pack, measured with a counting allocator over the
//! system one around [`HdPack::load`] of a full-size synthetic pack (every tile record, 206
//! portraits, two screen-sized panel bitmaps: about the owner's pack, ~86 MB decoded): the page
//! decodes every picture once at load and keeps them, so what a loaded pack holds is its pictures
//! decoded and a few words per picture, never a copy of the file; the peak while loading is
//! those, one atlas and the reader's entry bytes on top of the file. One test, so nothing else
//! allocates while it measures.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use u6_hdpack::{
    ATLAS_CELLS, Generator, ImageId, PORTRAIT_HD, PackImage, Rgba, Role, SCALE, TILE_HD, write_pack,
};
use u6_web::HdPack;

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

/// A `w × h` picture of a few flat bands (quick to encode), seeded.
fn picture(w: u32, h: u32, seed: u16) -> Rgba {
    let mut img = Rgba::new(w, h);
    for (i, p) in img.pixels.chunks_exact_mut(4).enumerate() {
        let band = (i as u32 / w / 8 + u32::from(seed)) % 5;
        p.copy_from_slice(&[(band * 50) as u8, seed as u8, (seed >> 8) as u8, 255]);
    }
    img
}

#[test]
fn a_loaded_pack_holds_its_pictures_decoded_and_no_copy_of_the_file() {
    let mut images: Vec<PackImage> = (0..2048u16)
        .map(|r| PackImage {
            id: ImageId::Tile(r),
            hash: u64::from(r) + 1,
            image: picture(TILE_HD, TILE_HD, r),
        })
        .collect();
    images.extend((0..206u16).map(|i| PackImage {
        id: ImageId::Portrait {
            letter: b'a' + (i / 100) as u8,
            item: i % 100,
        },
        hash: 10_000 + u64::from(i),
        image: picture(PORTRAIT_HD.0, PORTRAIT_HD.1, i),
    }));
    images.extend(Role::ALL.map(|role| PackImage {
        id: ImageId::Bitmap(role),
        hash: 20_000 + role as u64,
        image: picture(320 * SCALE, 200 * SCALE, role as u16),
    }));
    let decoded: usize = images.iter().map(|p| p.image.pixels.len()).sum();
    let largest_picture = images.iter().map(|p| p.image.pixels.len()).max().unwrap();
    let g = Generator {
        name: "test".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    };
    let bytes = write_pack(&images, &g, true).unwrap();
    drop(images);
    let file = bytes.len();
    let largest_entry = u6_intake::zip::read_zip(&bytes, |_| true)
        .unwrap()
        .iter()
        .map(|(_, b)| b.len())
        .max()
        .unwrap();
    let atlas = (ATLAS_CELLS * TILE_HD) as usize * (ATLAS_CELLS * TILE_HD) as usize * 4;

    // The file is handed over (the page's copy into wasm); it is freed once the pack is read.
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    // (A refusal's JsError is not made off wasm: only the loaded case runs here.)
    let Ok(pack) = HdPack::load(bytes) else {
        panic!("the pack loads")
    };
    let peak = PEAK.load(Ordering::SeqCst) - base;
    let held = NOW.load(Ordering::SeqCst) + file - base;
    assert_eq!(pack.counts(), [2048, 206, 2]);
    assert_eq!(pack.decoded_bytes() as usize, decoded);
    println!(
        "a full pack: file {file} bytes, {decoded} decoded; held after the load {held}, \
         peak during it {peak} over the file"
    );
    // Held: the pictures and a few words per picture; no copy of the file.
    assert!(
        held >= decoded && held <= decoded + (1 << 20),
        "held {held}, decoded {decoded}"
    );
    // Peak: the pictures, one atlas decoded, an entry's bytes and the largest picture being cut
    // or decoded, and the reader's small tables.
    let budget = decoded + atlas + 2 * largest_entry.max(largest_picture) + (1 << 20);
    assert!(peak <= budget, "peak {peak}, budget {budget}");
    drop(pack);
    assert!(
        NOW.load(Ordering::SeqCst) <= base - file,
        "everything freed with the pack"
    );
}
