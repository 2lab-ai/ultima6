//! Synthetic images only: every pixel here is made by the test.

use super::*;

fn generator() -> Generator {
    Generator {
        name: "u6-hd".into(),
        version: "0.1.0".into(),
        engine: "test".into(),
        date: "2026-10-16".into(),
    }
}

/// A `w × h` image of a gradient seeded by `seed`, with a transparent corner when `alpha`.
fn image(w: u32, h: u32, seed: u8, alpha: bool) -> Rgba {
    let mut img = Rgba::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let a = if alpha && x < w / 4 && y < h / 4 {
                0
            } else {
                255
            };
            img.pixels[i..i + 4].copy_from_slice(&[
                (x as u8).wrapping_add(seed),
                (y as u8).wrapping_mul(3),
                seed,
                a,
            ]);
        }
    }
    img
}

fn tile(r: u16, seed: u8, alpha: bool) -> PackImage {
    PackImage {
        id: ImageId::Tile(r),
        hash: u64::from(r) * 7 + 1,
        image: image(TILE_HD, TILE_HD, seed, alpha),
    }
}

fn sample() -> Vec<PackImage> {
    vec![
        tile(40, 1, false),
        tile(3, 2, true),
        PackImage {
            id: ImageId::Portrait {
                letter: b'b',
                item: 7,
            },
            hash: 99,
            image: image(PORTRAIT_HD.0, PORTRAIT_HD.1, 3, false),
        },
        PackImage {
            id: ImageId::Bitmap(Role::Spellbook),
            hash: 5,
            image: image(24 * SCALE, 10 * SCALE, 4, true),
        },
        tile(2047, 5, false),
    ]
}

/// The pack's files, to be changed and zipped again.
fn files_of(pack: &[u8]) -> Vec<(String, Vec<u8>)> {
    zip::read_zip(pack, |_| true).unwrap()
}

fn rezip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    store::write_zip(files)
}

fn with_file(pack: &[u8], name: &str, f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut files = files_of(pack);
    let at = files.iter().position(|(n, _)| n == name).unwrap();
    f(&mut files[at].1);
    rezip(&files)
}

fn manifest_of(pack: &[u8]) -> Manifest {
    let files = files_of(pack);
    let (_, m) = files.iter().find(|(n, _)| n == "manifest.json").unwrap();
    serde_json::from_slice(m).unwrap()
}

fn with_manifest(pack: &[u8], f: impl FnOnce(&mut Manifest)) -> Vec<u8> {
    let mut m = manifest_of(pack);
    f(&mut m);
    with_file(pack, "manifest.json", |b| {
        *b = serde_json::to_vec(&m).unwrap()
    })
}

/// A pack whose index is `entries` (and whose manifest agrees with it), files as they were.
fn with_index(pack: &[u8], entries: &[Entry]) -> Vec<u8> {
    let p = with_file(pack, "index.bin", |b| *b = encode_index(entries));
    with_manifest(&p, |m| {
        m.base = hex(base_digest(entries));
        m.images = entries.len() as u32;
    })
}

fn index_of(pack: &[u8]) -> Vec<Entry> {
    let files = files_of(pack);
    let (_, b) = files.iter().find(|(n, _)| n == "index.bin").unwrap();
    decode_index(b).unwrap()
}

fn written() -> Vec<u8> {
    write_pack(&sample(), &generator(), true).unwrap()
}

#[test]
fn round_trip() {
    let images = sample();
    let pack = written();
    let read = read_pack(&pack).unwrap();
    assert_eq!(read.manifest.format, FORMAT);
    assert_eq!((read.manifest.tile, read.manifest.scale), (16, 4));
    assert!(read.manifest.derived);
    assert_eq!(read.manifest.generator, generator());
    assert_eq!(read.manifest.images, 5);
    assert_eq!(read.images.len(), 5);
    for img in &images {
        assert_eq!(read.get(img.id), Some(img), "{:?}", img.id);
    }
    assert!(read.get(ImageId::Tile(4)).is_none());
    // Index order: tiles by record, then portraits, then bitmaps.
    let ids: Vec<ImageId> = read.images.iter().map(|p| p.id).collect();
    assert_eq!(ids[..3], [3, 40, 2047].map(ImageId::Tile));
    // The same images give the same bytes, whatever their order.
    let mut reversed = images.clone();
    reversed.reverse();
    assert_eq!(write_pack(&reversed, &generator(), true).unwrap(), pack);
    // The entries the writer makes, by name.
    let mut names: Vec<String> = files_of(&pack).into_iter().map(|(n, _)| n).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "atlas-0.webp",
            "bitmap/spellbook.webp",
            "index.bin",
            "manifest.json",
            "portrait/b-7.webp"
        ]
    );
    // A non-derived pack says so.
    let own = write_pack(&images, &generator(), false).unwrap();
    assert!(!read_pack(&own).unwrap().manifest.derived);
}

#[test]
fn tiles_fill_atlases_in_order() {
    let per = (ATLAS_CELLS * ATLAS_CELLS) as u16;
    let images: Vec<PackImage> = (0..per + 1).map(|r| tile(r, r as u8, r == 5)).collect();
    let pack = write_pack(&images, &generator(), true).unwrap();
    let index = index_of(&pack);
    assert_eq!(index[0].cell, Some((0, 0, 0)));
    let next_row = usize::from(ATLAS_CELLS as u16) + 1;
    assert_eq!(index[next_row].cell, Some((0, 1, 1)));
    assert_eq!(index[usize::from(per)].cell, Some((1, 0, 0)));
    let read = read_pack(&pack).unwrap();
    assert_eq!(read.images, images);
    // The second atlas is one row tall.
    let files = files_of(&pack);
    let (_, a1) = files.iter().find(|(n, _)| n == "atlas-1.webp").unwrap();
    let dec = image_webp::WebPDecoder::new(Cursor::new(a1.as_slice())).unwrap();
    assert_eq!(dec.dimensions(), (ATLAS_CELLS * TILE_HD, TILE_HD));
    assert_eq!(ATLAS_CELLS * TILE_HD, 2048);
}

#[test]
fn hashes_cover_mask_size_and_order() {
    let mut px = vec![0u8; 256];
    let mut clear = vec![false; 256];
    let opaque = tile_hash(&px, &clear).unwrap();
    clear[17] = true;
    let masked = tile_hash(&px, &clear);
    assert_ne!(Some(opaque), masked, "a transparent pixel is not colour 0");
    // Only a whole record has a hash (no hash of the shorter of two slices).
    assert_eq!(tile_hash(&px[..255], &clear), None);
    assert_eq!(tile_hash(&px, &clear[..255]), None);
    assert_eq!(tile_hash(&[0; 257], &[false; 257]), None);
    px[17] = 9;
    assert_eq!(
        tile_hash(&px, &clear),
        masked,
        "under the mask colour is ignored"
    );
    let bytes = [1u8, 2, 3, 4, 5, 6];
    assert_ne!(image_hash(3, 2, bytes), image_hash(2, 3, bytes));
    assert_ne!(
        image_hash(3, 2, bytes),
        image_hash(3, 2, [2u8, 1, 3, 4, 5, 6])
    );
    // FNV-1a 64 of nothing after the size words is fixed: the engine must reproduce it.
    assert_eq!(fnv(FNV_OFFSET, []), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv(FNV_OFFSET, *b"a"), 0xaf63_dc4c_8601_ec8c);
}

#[test]
fn refuses_what_is_not_a_pack() {
    assert!(matches!(read_pack(b"not a zip"), Err(PackError::Zip(_))));
    let pack = written();
    assert!(matches!(
        read_pack(&pack[..pack.len() - 30]),
        Err(PackError::Zip(_))
    ));
}

#[test]
fn refuses_unknown_repeated_and_unreferenced_entries() {
    let pack = written();
    for name in [
        "readme.txt",
        "atlas-01.webp",
        "portrait/",
        "portrait/B-1.webp",
        "bitmap/map.webp",
        "../atlas-0.webp",
    ] {
        let mut files = files_of(&pack);
        files.push((name.into(), vec![1]));
        assert!(
            matches!(read_pack(&rezip(&files)), Err(PackError::Entry(_))),
            "{name}"
        );
    }
    let mut files = files_of(&pack);
    files.push(files[1].clone());
    assert!(matches!(
        read_pack(&rezip(&files)),
        Err(PackError::Entry(_))
    ));
    // A file of a known shape the index does not name.
    let mut files = files_of(&pack);
    let (_, p) = files
        .iter()
        .find(|(n, _)| n.starts_with("portrait/"))
        .unwrap()
        .clone();
    files.push(("portrait/a-1.webp".into(), p));
    assert!(matches!(
        read_pack(&rezip(&files)),
        Err(PackError::Files(_))
    ));
    // One it names, missing.
    for gone in [
        "atlas-0.webp",
        "bitmap/spellbook.webp",
        "index.bin",
        "manifest.json",
    ] {
        let files: Vec<_> = files_of(&pack)
            .into_iter()
            .filter(|(n, _)| n != gone)
            .collect();
        assert!(
            matches!(read_pack(&rezip(&files)), Err(PackError::Files(_))),
            "{gone}"
        );
    }
}

#[test]
fn refuses_a_wrong_manifest() {
    let pack = written();
    type Change = Box<dyn Fn(&mut Manifest)>;
    let cases: [(&str, Change); 6] = [
        ("format", Box::new(|m| m.format = "u6-hdpack/2".into())),
        ("tile", Box::new(|m| m.tile = 8)),
        ("scale", Box::new(|m| m.scale = 8)),
        ("base", Box::new(|m| m.base = hex(1))),
        ("images", Box::new(|m| m.images += 1)),
        (
            "generator",
            Box::new(|m| m.generator.engine = "x".repeat(MAX_TEXT + 1)),
        ),
    ];
    for (what, f) in cases {
        assert!(
            matches!(
                read_pack(&with_manifest(&pack, f)),
                Err(PackError::Manifest(_))
            ),
            "{what}"
        );
    }
    let extra = with_file(&pack, "manifest.json", |b| {
        let mut v: serde_json::Value = serde_json::from_slice(b).unwrap();
        v["extra"] = serde_json::json!(1);
        *b = serde_json::to_vec(&v).unwrap();
    });
    assert!(matches!(read_pack(&extra), Err(PackError::Manifest(_))));
    let no_derived = with_file(&pack, "manifest.json", |b| {
        let mut v: serde_json::Value = serde_json::from_slice(b).unwrap();
        v.as_object_mut().unwrap().remove("derived");
        *b = serde_json::to_vec(&v).unwrap();
    });
    assert!(matches!(
        read_pack(&no_derived),
        Err(PackError::Manifest(_))
    ));
}

#[test]
fn refuses_a_malformed_index() {
    let pack = written();
    let raw = |f: &dyn Fn(&mut Vec<u8>)| read_pack(&with_file(&pack, "index.bin", f));
    assert!(matches!(raw(&|b| b[0] = b'X'), Err(PackError::Index(_))));
    assert!(matches!(raw(&|b| b[8] = 2), Err(PackError::Index(_))));
    assert!(matches!(
        raw(&|b| b.truncate(b.len() - 1)),
        Err(PackError::Index(_))
    ));
    assert!(matches!(raw(&|b| b.push(0)), Err(PackError::Index(_))));
    // Entry 0 (a tile): reserved byte, an unknown flag, an unknown kind.
    assert!(matches!(
        raw(&|b| b[INDEX_HEAD + 5] = 1),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        raw(&|b| b[INDEX_HEAD + 1] = 2),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        raw(&|b| b[INDEX_HEAD] = 4),
        Err(PackError::Index(_))
    ));
    // The bitmap entry (the last): an unknown role.
    let last = INDEX_HEAD + ENTRY_LEN * 4;
    assert!(matches!(
        raw(&|b| b[last + 2] = 9),
        Err(PackError::Index(_))
    ));
    // The portrait entry: a cell on an own file.
    let portrait = INDEX_HEAD + ENTRY_LEN * 3;
    assert!(matches!(
        raw(&|b| b[portrait + 8] = 1),
        Err(PackError::Index(_))
    ));
}

#[test]
fn refuses_wrong_entries() {
    let pack = written();
    let base = index_of(&pack);
    let refused = |f: &dyn Fn(&mut Vec<Entry>)| {
        let mut e = base.clone();
        f(&mut e);
        read_pack(&with_index(&pack, &e))
    };
    // Out of order, and the same image twice.
    assert!(matches!(
        refused(&|e| e.swap(0, 1)),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[1].id = e[0].id),
        Err(PackError::Index(_))
    ));
    // Sizes: a tile, a portrait, bitmaps not 8x or past the screen.
    assert!(matches!(
        refused(&|e| e[0].width = 128),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[3].height = PORTRAIT_HD.1 as u16 - 1),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[4].width = 95),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[4].width = (BITMAP_MAX.0 + 1) as u16 * SCALE as u16),
        Err(PackError::Index(_))
    ));
    // A record past the 2048, a portrait letter that is not a-z.
    assert!(matches!(
        refused(&|e| e[2].id = ImageId::Tile(2048)),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[3].id = ImageId::Portrait {
            letter: b'{',
            item: 0
        }),
        Err(PackError::Index(_))
    ));
    // Cells: two tiles in one, out of the atlas, an atlas skipped.
    assert!(matches!(
        refused(&|e| e[1].cell = e[0].cell),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[1].cell = Some((0, ATLAS_CELLS as u16, 0))),
        Err(PackError::Index(_))
    ));
    assert!(matches!(
        refused(&|e| e[2].cell = Some((2, 0, 0))),
        Err(PackError::Index(_))
    ));
    // An atlas the index names, missing; an image that is not the size its entry says.
    assert!(matches!(
        refused(&|e| e[2].cell = Some((1, 0, 0))),
        Err(PackError::Files(_))
    ));
    assert!(matches!(
        refused(&|e| e[2].cell = Some((0, 0, 1))),
        Err(PackError::Image(_))
    ));
    assert!(matches!(
        refused(&|e| e[4].width = 32 * SCALE as u16),
        Err(PackError::Image(_))
    ));
    // The alpha flag must say what the pixels say.
    assert!(matches!(
        refused(&|e| e[0].alpha = !e[0].alpha),
        Err(PackError::Image(_))
    ));
}

#[test]
fn a_full_pack_fits_the_limit() {
    // Every record, 300 portraits and two screen-sized bitmaps, as the index counts them.
    let mut entries: Vec<Entry> = (0..TILE_COUNT as u16)
        .map(|r| {
            let per = (ATLAS_CELLS * ATLAS_CELLS) as u16;
            let k = r % per;
            Entry {
                id: ImageId::Tile(r),
                alpha: true,
                cell: Some((r / per, k % ATLAS_CELLS as u16, k / ATLAS_CELLS as u16)),
                width: TILE_HD as u16,
                height: TILE_HD as u16,
                hash: 0,
            }
        })
        .collect();
    entries.extend((0..300).map(|i| Entry {
        id: ImageId::Portrait {
            letter: b'a',
            item: i,
        },
        alpha: false,
        cell: None,
        width: PORTRAIT_HD.0 as u16,
        height: PORTRAIT_HD.1 as u16,
        hash: 0,
    }));
    entries.extend(Role::ALL.map(|role| Entry {
        id: ImageId::Bitmap(role),
        alpha: false,
        cell: None,
        width: (BITMAP_MAX.0 * SCALE) as u16,
        height: (BITMAP_MAX.1 * SCALE) as u16,
        hash: 0,
    }));
    assert_eq!(check_entries(&entries).unwrap(), [32, 32]);
}

#[test]
fn refuses_past_the_limits() {
    let pack = written();
    // More than 4096 images: refused from the count alone.
    let many = with_file(&pack, "index.bin", |b| {
        b[12..16].copy_from_slice(&(MAX_ENTRIES as u32 + 1).to_le_bytes());
    });
    assert!(matches!(read_pack(&many), Err(PackError::Limit(_))));
    let entries: Vec<Entry> = (0..=MAX_ENTRIES as u16)
        .map(|i| Entry {
            id: ImageId::Portrait {
                letter: b'a',
                item: i,
            },
            alpha: false,
            cell: None,
            width: PORTRAIT_HD.0 as u16,
            height: PORTRAIT_HD.1 as u16,
            hash: 0,
        })
        .collect();
    assert!(matches!(check_entries(&entries), Err(PackError::Limit(_))));
    // 654 portraits are 150.01 MB decoded: refused from the index, before any file is looked at.
    let over = 654;
    assert!(over as u64 * u64::from(PORTRAIT_HD.0 * PORTRAIT_HD.1) * 4 > MAX_DECODED_BYTES);
    let portraits = with_index(&pack, &entries[..over]);
    assert!(matches!(read_pack(&portraits), Err(PackError::Limit(_))));
    assert!(check_entries(&entries[..over - 1]).is_ok());
    // What the zip's entries declare is bounded too, before anything is inflated: a deflated
    // entry of 5 bytes that declares one byte past the limit.
    let bomb = deflated_zip("bitmap/paper.webp", MAX_FILE_BYTES as u32 + 1);
    assert!(matches!(read_pack(&bomb), Err(PackError::Limit(_))));
    assert!(matches!(
        read_pack(&deflated_zip("bitmap/paper.webp", 0)),
        Err(PackError::Files(_))
    ));
    // The writer will not write past them either.
    let images: Vec<PackImage> = (0..over as u16)
        .map(|i| PackImage {
            id: ImageId::Portrait {
                letter: b'a',
                item: i,
            },
            hash: 0,
            image: Rgba::new(PORTRAIT_HD.0, PORTRAIT_HD.1),
        })
        .collect();
    assert!(matches!(
        write_pack(&images, &generator(), true),
        Err(PackError::Limit(_))
    ));
}

#[test]
fn refuses_images_that_are_not_lossless_stills() {
    let pack = written();
    let bad = with_file(&pack, "portrait/b-7.webp", |b| b.truncate(b.len() / 2));
    assert!(matches!(read_pack(&bad), Err(PackError::Image(_))));
    let png = with_file(&pack, "portrait/b-7.webp", |b| {
        *b = b"\x89PNG\r\n\x1a\n".to_vec()
    });
    assert!(matches!(read_pack(&png), Err(PackError::Image(_))));
    // A lossy (VP8) header of the right size: refused by kind, before its data is read.
    let (w, h) = (PORTRAIT_HD.0 as u16, PORTRAIT_HD.1 as u16);
    let lossy = with_file(&pack, "portrait/b-7.webp", |b| *b = vp8_header(w, h));
    match read_pack(&lossy) {
        Err(PackError::Image(d)) => assert!(d.contains("not a lossless still"), "{d}"),
        other => panic!("{other:?}"),
    }
}

/// One entry of [`zip_of`]: name, the bytes stored, method (0 stored, 8 deflate), the
/// uncompressed size declared in the central and in the local header, the CRC.
#[derive(Clone)]
struct Raw {
    name: String,
    data: Vec<u8>,
    method: u16,
    central_size: u32,
    local_size: u32,
    crc: u32,
}

impl Raw {
    /// `plain` deflated, declaring `declared` bytes in both headers.
    fn deflated(name: &str, plain: &[u8], declared: u32) -> Raw {
        Raw {
            name: name.into(),
            data: miniz_oxide::deflate::compress_to_vec(plain, 6),
            method: 8,
            central_size: declared,
            local_size: declared,
            crc: crc32fast::hash(plain),
        }
    }

    fn stored(name: &str, plain: &[u8]) -> Raw {
        Raw {
            name: name.into(),
            data: plain.to_vec(),
            method: 0,
            central_size: plain.len() as u32,
            local_size: plain.len() as u32,
            crc: crc32fast::hash(plain),
        }
    }
}

/// A zip of `entries`, headers written field by field.
fn zip_of(entries: &[Raw]) -> Vec<u8> {
    let head = |e: &Raw, size: u32| {
        let mut h = Vec::new();
        for v in [20u16, 0, e.method, 0, 0x21] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        for v in [e.crc, e.data.len() as u32, size] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h
    };
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let at = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&head(e, e.local_size));
        out.extend_from_slice(e.name.as_bytes());
        out.extend_from_slice(&e.data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&head(e, e.central_size));
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(e.name.as_bytes());
    }
    let cd = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    let n = entries.len() as u16;
    for v in [0u16, 0, n, n] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// The pack's entries, all deflated as declared, one of them (`name`) declaring `declared`.
fn redeflated(pack: &[u8], name: &str, declared: impl Fn(usize) -> u32) -> Vec<u8> {
    let raws: Vec<Raw> = files_of(pack)
        .iter()
        .map(|(n, b)| {
            let size = if n == name {
                declared(b.len())
            } else {
                b.len() as u32
            };
            Raw::deflated(n, b, size)
        })
        .collect();
    zip_of(&raws)
}

#[test]
fn inflating_is_bounded_by_what_the_entries_declare() {
    let pack = written();
    // The same pack with deflated entries, each declaring its size: it reads.
    let ok = redeflated(&pack, "portrait/b-7.webp", |n| n as u32);
    assert_eq!(read_pack(&ok).unwrap(), read_pack(&pack).unwrap());
    // One entry declares less than it inflates to: inflating stops at the declared size and
    // the pack is refused, not read short.
    let under = redeflated(&pack, "portrait/b-7.webp", |n| n as u32 / 2);
    assert!(matches!(read_pack(&under), Err(PackError::Zip(_))));
    let under = redeflated(&pack, "index.bin", |n| n as u32 - 1);
    assert!(matches!(read_pack(&under), Err(PackError::Zip(_))));
    // A byte past the entries' limit in zeros, a few hundred kB of deflate, declared
    // honestly: refused from the declaration, nothing inflated.
    let zeros = vec![0u8; MAX_FILE_BYTES as usize + 1];
    let mut raws: Vec<Raw> = files_of(&pack)
        .iter()
        .map(|(n, b)| Raw::stored(n, b))
        .collect();
    raws.push(Raw::deflated(
        "bitmap/paper.webp",
        &zeros,
        zeros.len() as u32,
    ));
    let mut big = zip_of(&raws);
    assert!(big.len() < 1 << 20);
    assert!(matches!(read_pack(&big), Err(PackError::Limit(_))));
    // The same entry declaring 1 MiB, which the index does not name: refused before it is
    // inflated.
    let last = raws.len() - 1;
    raws[last].central_size = 1 << 20;
    raws[last].local_size = 1 << 20;
    big = zip_of(&raws);
    match read_pack(&big) {
        Err(PackError::Files(d)) => assert!(d.contains("not in the index"), "{d}"),
        other => panic!("{other:?}"),
    }
    // In place of an image the index names, declaring 1 MiB: inflating stops there, refused.
    raws.pop();
    let k = raws
        .iter()
        .position(|r| r.name == "portrait/b-7.webp")
        .unwrap();
    raws[k] = Raw::deflated("portrait/b-7.webp", &zeros, 1 << 20);
    drop(zeros);
    assert!(matches!(read_pack(&zip_of(&raws)), Err(PackError::Zip(_))));
}

#[test]
fn local_and_central_records_must_agree() {
    let pack = written();
    let mut raws: Vec<Raw> = files_of(&pack)
        .iter()
        .map(|(n, b)| Raw::stored(n, b))
        .collect();
    assert_eq!(
        read_pack(&zip_of(&raws)).unwrap(),
        read_pack(&pack).unwrap()
    );
    // A local header with another name than its central record: refused by the zip reader.
    let mut renamed = zip_of(&raws);
    let at = renamed
        .windows(b"index.bin".len())
        .position(|w| w == b"index.bin")
        .unwrap();
    renamed[at] = b'I';
    assert!(matches!(read_pack(&renamed), Err(PackError::Zip(_))));
    // A local header declaring another size than the central one: the reader goes by the
    // central record (u6-intake), so the bytes read are the same and the pack is the same.
    let k = raws.iter().position(|r| r.name == "index.bin").unwrap();
    raws[k].local_size += 7;
    let differs = zip_of(&raws);
    assert_eq!(read_pack(&differs).unwrap(), read_pack(&pack).unwrap());
}

/// A zip of one deflated entry `name` (an empty final stored block) declaring `declared` bytes.
fn deflated_zip(name: &str, declared: u32) -> Vec<u8> {
    let data = [0x01, 0x00, 0x00, 0xff, 0xff];
    let crc = crc32fast::hash(&[]);
    let mut head = Vec::new();
    for v in [20u16, 0, 8, 0, 0x21] {
        head.extend_from_slice(&v.to_le_bytes());
    }
    for v in [crc, data.len() as u32, declared] {
        head.extend_from_slice(&v.to_le_bytes());
    }
    head.extend_from_slice(&(name.len() as u16).to_le_bytes());
    head.extend_from_slice(&0u16.to_le_bytes());
    let mut out = 0x0403_4b50u32.to_le_bytes().to_vec();
    out.extend_from_slice(&head);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&data);
    let cd = out.len() as u32;
    out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&head);
    // Comment length, disk, attributes (2 + 4), the local header at 0.
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(name.as_bytes());
    let size = out.len() as u32 - cd;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, 1, 1] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&cd.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A valid animated WebP of one frame: the encoder's lossless frame of `img` inside the
/// VP8X, ANIM and ANMF chunks.
fn animated_webp(img: &Rgba) -> Vec<u8> {
    let still = encode_webp(img).unwrap();
    // RIFF, size, WEBP, then the one VP8L chunk (tag, size, data, pad).
    let vp8l = &still[12..];
    let u24 = |v: u32| v.to_le_bytes()[..3].to_vec();
    let chunk = |tag: &[u8], body: &[u8]| {
        let mut c = tag.to_vec();
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 {
            c.push(0);
        }
        c
    };
    let mut vp8x = vec![0x02 | 0x10, 0, 0, 0];
    vp8x.extend(u24(img.width - 1));
    vp8x.extend(u24(img.height - 1));
    let anim = [0u8, 0, 0, 0, 0, 0];
    let mut anmf = Vec::new();
    for v in [0, 0, img.width - 1, img.height - 1, 100] {
        anmf.extend(u24(v));
    }
    // Not blended onto the canvas (the decoder's blend is not exact): the frame replaces it.
    anmf.push(0x02);
    anmf.extend_from_slice(vp8l);
    let mut body = b"WEBP".to_vec();
    body.extend(chunk(b"VP8X", &vp8x));
    body.extend(chunk(b"ANIM", &anim));
    body.extend(chunk(b"ANMF", &anmf));
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

#[test]
fn an_animated_webp_is_refused() {
    let pack = written();
    let img = image(PORTRAIT_HD.0, PORTRAIT_HD.1, 3, false);
    let anim = animated_webp(&img);
    // It is a valid animation of that size, whose one frame decodes to the image's pixels.
    let mut dec = image_webp::WebPDecoder::new(Cursor::new(anim.as_slice())).unwrap();
    assert!(dec.is_animated());
    assert_eq!(dec.num_frames(), 1);
    assert_eq!(dec.dimensions(), (PORTRAIT_HD.0, PORTRAIT_HD.1));
    assert!(dec.has_alpha());
    let mut frame = vec![0; dec.output_buffer_size().unwrap()];
    dec.read_frame(&mut frame).unwrap();
    assert!(frame == img.pixels, "the frame is not the image");
    let bad = with_file(&pack, "portrait/b-7.webp", |b| *b = anim.clone());
    match read_pack(&bad) {
        Err(PackError::Image(d)) => assert!(d.contains("not a lossless still"), "{d}"),
        other => panic!("{other:?}"),
    }
}

/// The headers of a lossy WebP key frame of `w × h` (no image data after them).
fn vp8_header(w: u16, h: u16) -> Vec<u8> {
    let mut frame = vec![0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a];
    frame.extend_from_slice(&w.to_le_bytes());
    frame.extend_from_slice(&h.to_le_bytes());
    frame.extend_from_slice(&[0; 16]);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(4 + 8 + frame.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WEBPVP8 ");
    out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    out.extend_from_slice(&frame);
    out
}

#[test]
fn the_writer_refuses_what_the_reader_would() {
    let mut images = sample();
    images.push(tile(40, 9, false));
    assert!(matches!(
        write_pack(&images, &generator(), true),
        Err(PackError::Index(_))
    ));
    let mut images = sample();
    images[0].image = image(TILE_HD * 2, TILE_HD * 2, 0, false);
    assert!(matches!(
        write_pack(&images, &generator(), true),
        Err(PackError::Index(_))
    ));
    let mut images = sample();
    images[0].image.pixels.pop();
    assert!(matches!(
        write_pack(&images, &generator(), true),
        Err(PackError::Image(_))
    ));
    let mut g = generator();
    g.date = "x".repeat(MAX_TEXT + 1);
    assert!(matches!(
        write_pack(&sample(), &g, true),
        Err(PackError::Manifest(_))
    ));
}

/// What the probe told of `file`: not a pack (`None`), or a pack and its refusal's kind (`""`
/// for none); and the ranges it read, in order.
fn probe(file: &[u8]) -> (Option<&'static str>, Vec<(u64, u64)>) {
    let mut p = Probe::new(file.len() as u64);
    let mut reads = Vec::new();
    while let Some((start, end)) = p.need() {
        reads.push((start, end));
        assert!(reads.len() <= 4, "a few reads");
        p.give(&file[start as usize..end as usize]);
    }
    let told = match p.is_pack().expect("told once nothing is needed") {
        false => {
            assert!(p.refusal().is_none());
            None
        }
        true => Some(match p.refusal() {
            None => "",
            Some(PackError::Zip(_)) => "zip",
            Some(PackError::Entry(_)) => "entry",
            Some(PackError::Limit(_)) => "limit",
            Some(PackError::Manifest(_)) => "manifest",
            Some(e) => panic!("not a probe refusal: {e}"),
        }),
    };
    (told, reads)
}

/// The range of the root `manifest.json` entry of `file`, as the reader slices it.
fn manifest_range(file: &[u8]) -> (u64, u64) {
    let zip::Directory::Entries(found) =
        zip::directory(file.len() as u64, 0, file, |n| n == "manifest.json").unwrap()
    else {
        panic!("a whole file")
    };
    (found[0].start, found[0].end)
}

/// The end of `file` the probe reads first.
fn tail(file: &[u8]) -> (u64, u64) {
    (zip::tail_start(file.len() as u64), file.len() as u64)
}

#[test]
fn the_probe_tells_a_pack_from_its_directory_and_manifest() {
    let pack = written();
    let (told, reads) = probe(&pack);
    assert_eq!(told, Some(""), "a pack, nothing refused");
    assert_eq!(
        reads,
        [tail(&pack), manifest_range(&pack)],
        "the end, then the manifest"
    );
    // The manifest is the writer's first entry: its range starts the file.
    assert_eq!(manifest_range(&pack).0, 0);
    // Its entries deflated (the owner zipped it again): still a pack.
    assert_eq!(probe(&redeflated(&pack, "", |n| n as u32)).0, Some(""));
    // The key written as a JSON escape is the same key: a pack.
    let escaped = with_file(&pack, "manifest.json", |b| {
        let text = String::from_utf8(b.clone()).unwrap();
        assert!(text.contains("\"format\""));
        *b = text
            .replacen("\"format\"", "\"\\u0066ormat\"", 1)
            .into_bytes();
    });
    assert_eq!(probe(&escaped).0, Some(""));
    assert!(read_pack(&escaped).is_ok());
    // A zip of game-like files: the end of it only. One whose manifest is not a JSON object
    // whose root format is the pack's (another format, one that only starts like it, a web
    // manifest naming the format elsewhere, the format named deeper in it, no format while
    // mentioning it, a format that is not a string, not an object, not JSON; the object with
    // data after it, or after a UTF-8 byte order mark; the format's name as a string root, the
    // object inside a list root): not a pack.
    let other = rezip(&[
        ("game".into(), vec![1; 300]),
        ("Disk 1.adf".into(), vec![2; 900]),
    ]);
    assert_eq!(probe(&other), (None, vec![tail(&other)]));
    for m in [
        &br#"{"format": "webmanifest"}"#[..],
        br#"{"format": "u6-hdpack/10"}"#,
        br#"{"format": "webmanifest", "note": "u6-hdpack/1"}"#,
        br#"{"a": {"format": "u6-hdpack/1"}}"#,
        br#"{"name": "game", "description": "supports u6-hdpack/1"}"#,
        br#"{"format": 1}"#,
        br#"["u6-hdpack/1"]"#,
        br#""format": u6-hdpack/1"#,
        b"u6-hdpack/1 manifest",
        TRAILING,
        BOM,
        STRING_ROOT,
        LIST_ROOT,
    ] {
        let web = with_file(&pack, "manifest.json", |b| *b = m.to_vec());
        assert_eq!(probe(&web).0, None, "{}", String::from_utf8_lossy(m));
    }
    // A manifest of the format that is wrong otherwise: a pack, refused by its manifest.
    let wrong = with_manifest(&pack, |m| m.tile = 8);
    assert_eq!(probe(&wrong).0, Some("manifest"));
    assert!(matches!(read_pack(&wrong), Err(PackError::Manifest(_))));
    // A manifest in a subfolder is not the pack's.
    let nested: Vec<(String, Vec<u8>)> = files_of(&pack)
        .into_iter()
        .map(|(n, b)| (format!("dir/{n}"), b))
        .collect();
    assert_eq!(probe(&rezip(&nested)).0, None);
    // Not a zip, empty, or bytes that are not the range asked for.
    assert_eq!(probe(b"not a zip at all").0, None);
    assert_eq!(probe(b""), (None, vec![]));
    let mut p = Probe::new(pack.len() as u64);
    p.give(&pack[..10]);
    assert_eq!(p.is_pack(), Some(false));
}

/// Manifests that are not a JSON object whose root `format` is the pack's, though close to one:
/// the object followed by other data, after a UTF-8 byte order mark (JSON has none), the name
/// as a string root, the object inside a list root.
const TRAILING: &[u8] = br#"{"format": "u6-hdpack/1"} x"#;
const BOM: &[u8] = b"\xEF\xBB\xBF{\"format\": \"u6-hdpack/1\"}";
const STRING_ROOT: &[u8] = br#""u6-hdpack/1""#;
const LIST_ROOT: &[u8] = br#"[{"format": "u6-hdpack/1"}]"#;

/// Two root `manifest.json` entries, another format's first and the pack's second: the first is
/// the manifest (as the directory lists them), so it is not a pack; the reader refuses the name
/// twice. The same before a whole pack's entries.
#[test]
fn of_two_root_manifests_the_first_is_the_one() {
    let two = zip_of(&[
        Raw::stored("manifest.json", br#"{"format": "webmanifest"}"#),
        Raw::stored("manifest.json", br#"{"format": "u6-hdpack/1"}"#),
    ]);
    assert_eq!(probe(&two).0, None);
    let mut files = vec![(
        "manifest.json".to_string(),
        br#"{"format": "webmanifest"}"#.to_vec(),
    )];
    files.extend(files_of(&written()));
    let pack = rezip(&files);
    assert_eq!(probe(&pack).0, None, "before a whole pack");
    assert!(matches!(read_pack(&pack), Err(PackError::Entry(_))));
}

/// The written pack with its manifest padded with spaces after the object to `len` bytes
/// unpacked (stored).
fn padded_manifest(len: u64) -> Vec<u8> {
    let mut files = files_of(&written());
    let m = &mut files
        .iter_mut()
        .find(|(n, _)| n == "manifest.json")
        .unwrap()
        .1;
    m.resize(len as usize, b' ');
    rezip(&files)
}

/// The written pack with its manifest deflated and followed, up to the next entry's local
/// header, by bytes the stream does not reach (inflating stops at its final block), so the
/// manifest's range is `range` bytes.
fn ranged_manifest(range: u64) -> Vec<u8> {
    let files = files_of(&written());
    let (_, manifest) = files.iter().find(|(n, _)| n == "manifest.json").unwrap();
    let mut data = miniz_oxide::deflate::compress_to_vec(manifest, 6);
    data.resize((range - 30 - 13) as usize, 0xA5);
    let raws: Vec<Raw> = files
        .iter()
        .map(|(n, b)| {
            if n == "manifest.json" {
                Raw {
                    name: n.clone(),
                    data: data.clone(),
                    method: 8,
                    central_size: b.len() as u32,
                    local_size: b.len() as u32,
                    crc: crc32fast::hash(b),
                }
            } else {
                Raw::stored(n, b)
            }
        })
        .collect();
    zip_of(&raws)
}

/// The limits are inclusive: a manifest of exactly 64 KiB unpacked ([`padded_manifest`]), and
/// one whose range is exactly [`MAX_MANIFEST_RANGE`] ([`ranged_manifest`]), read and make a
/// pack; the reader takes both packs too.
#[test]
fn a_manifest_at_the_limits_reads() {
    let z = padded_manifest(MAX_MANIFEST);
    let (start, end) = manifest_range(&z);
    assert_eq!(
        end - start,
        30 + 13 + MAX_MANIFEST,
        "exactly 64 KiB of data"
    );
    assert_eq!(probe(&z), (Some(""), vec![tail(&z), (start, end)]));
    assert!(read_pack(&z).is_ok(), "the reader takes it");
    let z = ranged_manifest(MAX_MANIFEST_RANGE);
    let (start, end) = manifest_range(&z);
    assert_eq!(
        end - start,
        MAX_MANIFEST_RANGE,
        "a range of exactly the probe's limit"
    );
    assert_eq!(probe(&z), (Some(""), vec![tail(&z), (start, end)]));
    assert!(read_pack(&z).is_ok(), "the reader takes it");
}

/// One byte over either limit, built as its twin at the limit: a manifest of 64 KiB + 1
/// unpacked, and one whose range is [`MAX_MANIFEST_RANGE`] + 1, are not read (the tail only) and
/// are not packs. The reader refuses the first (its limit is the unpacked size) and takes the
/// second (the range limit is the probe's own).
#[test]
fn a_manifest_one_byte_over_the_limits_is_not_read() {
    let z = padded_manifest(MAX_MANIFEST + 1);
    let (start, end) = manifest_range(&z);
    assert_eq!(
        end - start,
        30 + 13 + MAX_MANIFEST + 1,
        "64 KiB and a byte of data"
    );
    assert_eq!(probe(&z), (None, vec![tail(&z)]));
    assert!(matches!(read_pack(&z), Err(PackError::Limit(_))));
    let z = ranged_manifest(MAX_MANIFEST_RANGE + 1);
    let (start, end) = manifest_range(&z);
    assert_eq!(
        end - start,
        MAX_MANIFEST_RANGE + 1,
        "a range of the probe's limit and a byte"
    );
    assert_eq!(probe(&z), (None, vec![tail(&z)]));
    assert!(read_pack(&z).is_ok(), "the reader has no range limit");
}

/// Spec 07 H2's rule for a manifest that does not read or parse: not a pack, so the zip goes to
/// the import (which takes only disk images): a manifest with a wrong CRC-32 (one of the pack's
/// format included), cut short, a bare declaration that is not JSON, the format named deeper in
/// it, no format while mentioning it, over the reader's limit (not read at all), deflated and
/// broken; beside disk images and alone.
#[test]
fn a_manifest_that_does_not_read_or_parse_is_not_a_pack() {
    let disks = |manifest: Raw| {
        zip_of(&[
            Raw::stored("Ultima VI Disk 1.adf", &[1; 900]),
            manifest,
            Raw::stored("Ultima VI Disk 2.adf", &[2; 700]),
        ])
    };
    let bad = |m: &[u8]| {
        let mut r = Raw::stored("manifest.json", m);
        r.crc ^= 1;
        r
    };
    let format = br#"{"format": "u6-hdpack/1"}"#;
    let nested = br#"{"a": {"format": "u6-hdpack/1"}}"#;
    let over = [
        br#"{"format": "u6-hdpack/1"}"#.as_slice(),
        &vec![b' '; MAX_MANIFEST as usize],
    ]
    .concat();
    let mut deflated = Raw::deflated("manifest.json", format, format.len() as u32);
    deflated.crc ^= 1;
    let cases: Vec<(&str, Raw, bool)> = vec![
        ("the format, a wrong CRC-32", bad(format), true),
        (
            "cut short",
            Raw::stored("manifest.json", br#"{"format": "u6-hdpack/1", "tile": 16"#),
            true,
        ),
        (
            "a bare declaration",
            Raw::stored("manifest.json", b"\"format\": \"u6-hdpack/1\""),
            true,
        ),
        ("named deeper", Raw::stored("manifest.json", nested), true),
        ("named deeper, a wrong CRC-32", bad(nested), true),
        (
            "mentioned",
            Raw::stored("manifest.json", br#"{"description": "u6-hdpack/1"}"#),
            true,
        ),
        (
            "mentioned, a wrong CRC-32",
            bad(br#"{"description": "u6-hdpack/1"}"#),
            true,
        ),
        ("over the limit", Raw::stored("manifest.json", &over), false),
        ("deflated, a wrong CRC-32", deflated, true),
        (
            "data after the object",
            Raw::stored("manifest.json", TRAILING),
            true,
        ),
        (
            "a byte order mark first",
            Raw::stored("manifest.json", BOM),
            true,
        ),
        (
            "a string root",
            Raw::stored("manifest.json", STRING_ROOT),
            true,
        ),
        ("a list root", Raw::stored("manifest.json", LIST_ROOT), true),
    ];
    for (what, m, read) in cases {
        for (z, beside) in [
            (disks(m.clone()), "beside the disks"),
            (zip_of(std::slice::from_ref(&m)), "alone"),
        ] {
            let (told, reads) = probe(&z);
            assert_eq!(told, None, "{what}, {beside}: not a pack");
            let want = if read {
                vec![tail(&z), manifest_range(&z)]
            } else {
                vec![tail(&z)]
            };
            assert_eq!(reads, want, "{what}, {beside}: the ranges read");
        }
    }
}

/// A manifest whose local header carries an extra field (its length above 0, which the central
/// record does not have) reads as it would without one: the pack is a pack, and reads.
#[test]
fn a_manifest_with_a_local_extra_field_reads() {
    let files = files_of(&written());
    let extra = [0x55u8, 0x54, 4, 0, 1, 2, 3, 4];
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in &files {
        let at = out.len() as u32;
        let local_extra: &[u8] = if name == "manifest.json" { &extra } else { &[] };
        let head = |extra_len: u16| {
            let mut h = Vec::new();
            for v in [20u16, 0, 0, 0, 0x21] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc32fast::hash(data), data.len() as u32, data.len() as u32] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&extra_len.to_le_bytes());
            h
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&head(local_extra.len() as u16));
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(local_extra);
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&head(0));
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
    assert_eq!(
        &out[28..30],
        &(extra.len() as u16).to_le_bytes(),
        "the manifest's extra length"
    );
    let (told, reads) = probe(&out);
    assert_eq!(told, Some(""), "a pack");
    assert_eq!(reads, [tail(&out), manifest_range(&out)]);
    assert!(read_pack(&out).is_ok(), "and the reader reads it");
}

/// The probe's reads are bounded by what it asks for: a manifest over the reader's limits (a
/// stored one holding 1 MiB, though it declares 100 bytes; a deflated one declaring 100 bytes
/// whose data runs on for 1 MiB) is not read at all, and is not a pack's; a
/// directory starting more than 16 MiB before the end, or an entry whose local header lies past
/// the file, is refused from the end read first; a directory that starts before the end read is
/// read from where it starts.
#[test]
fn the_probe_reads_only_the_ranges_it_names() {
    let mut huge = Raw::stored(
        "manifest.json",
        &[br#"{"format": "u6-hdpack/1"}"#.as_slice(), &[b' '; 1 << 20]].concat(),
    );
    huge.central_size = 100;
    huge.local_size = 100;
    let z = zip_of(&[huge]);
    let (start, end) = manifest_range(&z);
    assert!(end - start > 1 << 20, "its range holds the 1 MiB");
    assert_eq!(
        probe(&z),
        (None, vec![tail(&z)]),
        "over the limit: not read, not a pack"
    );
    // Deflated, declaring 100 bytes unpacked (within the limit) while its data runs on for
    // 1 MiB: its range is past what is read, so it is over the limit too (not read at all).
    let data = [br#"{"format": "u6-hdpack/1"}"#.as_slice(), &[b' '; 1 << 20]].concat();
    let long = Raw {
        name: "manifest.json".into(),
        crc: crc32fast::hash(&data),
        data,
        method: 8,
        central_size: 100,
        local_size: 100,
    };
    let z = zip_of(&[long]);
    let (start, end) = manifest_range(&z);
    assert!(end - start > MAX_MANIFEST_RANGE);
    assert_eq!(
        probe(&z),
        (None, vec![tail(&z)]),
        "its range over the limit: not read"
    );
    // A directory over the 16 MiB cap: its end record names it 17 MiB before the end.
    let mut far = vec![0u8; 17 << 20];
    far.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, 1, 1] {
        far.extend_from_slice(&v.to_le_bytes());
    }
    far.extend_from_slice(&(17u32 << 20).to_le_bytes());
    far.extend_from_slice(&0u32.to_le_bytes());
    far.extend_from_slice(&0u16.to_le_bytes());
    assert_eq!(probe(&far), (None, vec![tail(&far)]));
    // A manifest whose local header offset lies past the file.
    let mut z = zip_of(&[Raw::stored(
        "manifest.json",
        br#"{"format": "u6-hdpack/1"}"#,
    )]);
    let cd = z
        .windows(4)
        .rposition(|w| w == 0x0201_4b50u32.to_le_bytes())
        .unwrap();
    z[cd + 42..cd + 46].copy_from_slice(&0x00ff_0000u32.to_le_bytes());
    assert_eq!(probe(&z), (None, vec![tail(&z)]));
    // Many entries: the directory starts before the tail read first; it is read from its start.
    let mut files: Vec<(String, Vec<u8>)> = (0..3000)
        .map(|i| {
            (
                format!("filler/a-long-name-to-fill-the-directory-{i:05}.bin"),
                vec![],
            )
        })
        .collect();
    files.push((
        "manifest.json".into(),
        br#"{"format": "u6-hdpack/1"}"#.to_vec(),
    ));
    let z = rezip(&files);
    let eocd = z.len() - 22;
    let dir = u64::from(u32::from_le_bytes(
        z[eocd + 16..eocd + 20].try_into().unwrap(),
    ));
    assert!(dir < tail(&z).0);
    let (told, reads) = probe(&z);
    assert_eq!(reads, [tail(&z), (dir, z.len() as u64), manifest_range(&z)]);
    assert_eq!(told, Some("entry"), "the filler entries are not a pack's");
}

/// The engine and the tool hash portraits and bitmaps with one definition: a portrait's 56 × 64
/// bytes, a bitmap's first `width × height` (none of a shorter one).
#[test]
fn portraits_and_bitmaps_hash_by_one_definition() {
    let px: Vec<u8> = (0..56 * 64).map(|i| (i % 251) as u8).collect();
    assert_eq!(
        portrait_hash(&px),
        Some(image_hash(56, 64, px.iter().copied()))
    );
    assert_eq!(portrait_hash(&px[1..]), None);
    let bmp: Vec<u8> = (0..30).collect();
    assert_eq!(
        bitmap_hash(5, 6, &bmp),
        Some(image_hash(5, 6, bmp.iter().copied()))
    );
    assert_eq!(
        bitmap_hash(5, 5, &bmp),
        Some(image_hash(5, 5, bmp[..25].iter().copied()))
    );
    assert_eq!(bitmap_hash(6, 6, &bmp), None);
    assert_ne!(
        bitmap_hash(5, 6, &bmp),
        bitmap_hash(6, 5, &bmp),
        "the size is hashed"
    );
}
