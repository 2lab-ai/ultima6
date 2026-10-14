//! Synthetic-data tests for the tile and palette decoders, plus one real-data test that skips
//! when the unpacked game files are absent.

use super::*;

/// One synthetic map record: offset, first row, per-row words for planes 0..4, anim ref.
type MapRec<'a> = (usize, u8, &'a [[u16; PLANES]], Option<u16>);

/// Builds `maptiles.ega` with one plane length `n` from `(offset, plane-0..4 row words)` records.
/// Every record is `[first][rows]` + rows; planes 1..4 copy the layout at `+k·n`.
fn maptiles(n: usize, recs: &[MapRec]) -> Vec<u8> {
    let mut f = vec![0u8; 2 + PLANES * n];
    f[..2].copy_from_slice(&(n as u16).to_le_bytes());
    for &(o, first, rows, anim) in recs {
        f[2 + o] = first;
        f[3 + o] = rows.len() as u8;
        for (r, words) in rows.iter().enumerate() {
            for (k, w) in words.iter().enumerate() {
                let at = 2 + k * n + o + 2 + 2 * r;
                f[at..at + 2].copy_from_slice(&w.to_be_bytes());
            }
        }
        if let Some(a) = anim {
            let at = 2 + o + 2 + 2 * rows.len();
            f[at..at + 2].copy_from_slice(&a.to_le_bytes());
        }
    }
    f
}

/// A tileset input where every tile is the empty map record `[16][0]` at offset 0, except the
/// `(tile, masktype, index)` overrides.
fn inputs(over: &[(usize, u8, u16)]) -> (Vec<u8>, Vec<u8>) {
    let mut idx = vec![0u8; TILE_COUNT * 2];
    let mut mt = vec![0u8; TILE_COUNT];
    for t in 0..TILE_COUNT {
        idx[t * 2..t * 2 + 2].copy_from_slice(&0x6000u16.to_le_bytes());
    }
    for &(t, m, i) in over {
        mt[t] = m;
        idx[t * 2..t * 2 + 2].copy_from_slice(&i.to_le_bytes());
    }
    (idx, mt)
}

fn px(tile: &Tile, x: usize, y: usize) -> Option<u8> {
    let i = y * TILE_W + x;
    (!tile.transparent[i]).then_some(tile.pixels[i])
}

#[test]
fn gfx_map_tile_planes_rows_and_anim_ref() {
    // Tile 1 at offset 4: rows 2..4; row 0 = leftmost pixel colour 0b10001, rightmost colour 2.
    let rows: [[u16; PLANES]; 2] = [[0x8000, 0x0001, 0, 0, 0x8000], [0xFFFF; PLANES]];
    let mt_file = maptiles(64, &[(0, 16, &[], None), (4, 2, &rows, Some(0xB300))]);
    let (idx, mt) = inputs(&[(1, 4, 0x6004)]);
    let set = decode_tiles(&mt_file, &[], &idx, &mt).unwrap();
    assert_eq!(set.tiles.len(), TILE_COUNT);
    let t0 = &set.tiles[0];
    assert_eq!((t0.kind, t0.rows), (TileKind::Map, 0));
    assert!(t0.transparent.iter().all(|&b| b));
    let t1 = &set.tiles[1];
    assert_eq!((t1.first_row, t1.rows, t1.anim_ref), (2, 2, Some(0xB300)));
    assert_eq!(px(t1, 0, 1), None);
    assert_eq!(px(t1, 0, 2), Some(0b10001));
    assert_eq!(px(t1, 15, 2), Some(0b00010));
    assert_eq!(px(t1, 7, 2), Some(0));
    assert_eq!(px(t1, 9, 3), Some(31));
    assert_eq!(px(t1, 9, 4), None);
}

#[test]
fn gfx_map_tile_errors() {
    let f = maptiles(8, &[(0, 16, &[], None)]);
    // index below 0x6000 for a map tile
    let (idx, mt) = inputs(&[(5, 0, 0x0010)]);
    assert!(decode_tiles(&f, &[], &idx, &mt).is_err());
    // rows past the bottom
    let mut bad = f.clone();
    bad[2] = 10;
    bad[3] = 7;
    let (idx, mt) = inputs(&[]);
    assert!(decode_tiles(&bad, &[], &idx, &mt).is_err());
    // rows that leave plane 0
    let mut bad = f.clone();
    bad[2] = 0;
    bad[3] = 4;
    assert!(decode_tiles(&bad, &[], &idx, &mt).is_err());
    // an empty record still names its first row: 16 (the file's) is the last one
    let mut bad = f.clone();
    bad[2] = 17;
    bad[3] = 0;
    assert!(decode_tiles(&bad, &[], &idx, &mt).is_err());
    // truncated planes, wrong tileindx size
    assert!(decode_tiles(&f[..f.len() - 1], &[], &idx, &mt).is_err());
    assert!(decode_tiles(&f, &[], &idx[1..], &mt).is_err());
}

/// One object record: header, mask rows, `(plane, rows)` blocks, terminator, optional anim ref.
fn obj(first: u8, w: usize, mask: &[u16], planes: &[(u8, &[u16])], anim: Option<u16>) -> Vec<u8> {
    let put = |v: &mut Vec<u8>, x: u16| {
        if w == 2 {
            v.extend_from_slice(&x.to_be_bytes());
        } else {
            v.push(x as u8);
        }
    };
    let mut v = vec![first, mask.len() as u8, 0x5A];
    mask.iter().for_each(|&m| put(&mut v, m));
    for &(p, rows) in planes {
        v.push(p);
        rows.iter().for_each(|&r| put(&mut v, r));
    }
    v.push(0xFF);
    if let Some(a) = anim {
        v.extend_from_slice(&a.to_le_bytes());
    }
    v
}

#[test]
fn gfx_object_tiles_mask_planes_and_spans() {
    let map = maptiles(8, &[(0, 16, &[], None)]);
    let mut objs = vec![0u8; 0x200];
    // Tile 2: 16 wide at 0x000, rows 1..3; row 1 opaque on the left half only.
    let full = obj(
        1,
        2,
        &[0x00FF, 0x0000],
        &[(0, &[0x8000, 0x0001]), (4, &[0x4000, 0])],
        Some(0xFC10),
    );
    objs[..full.len()].copy_from_slice(&full);
    // Tile 3: right 8 columns at 0x100 (index 0x10); tile 4: left 8 columns at 0x180 (index 0x18).
    let right = obj(0, 1, &[0x0F], &[(2, &[0x80])], None);
    objs[0x100..0x100 + right.len()].copy_from_slice(&right);
    let left = obj(15, 1, &[0x7F], &[(1, &[0x80])], None);
    objs[0x180..0x180 + left.len()].copy_from_slice(&left);
    let (idx, mt) = inputs(&[(2, 7, 0x0000), (3, 1, 0x0010), (4, 2, 0x0018)]);
    let set = decode_tiles(&map, &objs, &idx, &mt).unwrap();

    let t2 = &set.tiles[2];
    assert_eq!(t2.kind, TileKind::Object(Span::Full));
    assert_eq!(
        (t2.first_row, t2.rows, t2.unused_byte, t2.anim_ref),
        (1, 2, Some(0x5A), Some(0xFC10))
    );
    assert_eq!(px(t2, 0, 0), None);
    assert_eq!(px(t2, 0, 1), Some(0b00001));
    assert_eq!(px(t2, 1, 1), Some(0b10000));
    assert_eq!(px(t2, 2, 1), Some(0));
    assert_eq!(px(t2, 8, 1), None);
    assert_eq!(px(t2, 15, 2), Some(0b00001));
    assert_eq!(px(t2, 0, 3), None);

    let t3 = &set.tiles[3];
    assert_eq!(t3.kind, TileKind::Object(Span::Right));
    assert_eq!(px(t3, 7, 0), None);
    assert_eq!(px(t3, 8, 0), Some(4));
    assert_eq!(px(t3, 11, 0), Some(0));
    assert_eq!(px(t3, 12, 0), None);

    let t4 = &set.tiles[4];
    assert_eq!(t4.kind, TileKind::Object(Span::Left));
    assert_eq!(px(t4, 0, 15), Some(2));
    assert_eq!(px(t4, 1, 15), None);
    assert_eq!(px(t4, 8, 15), None);
}

#[test]
fn gfx_object_tile_errors() {
    let map = maptiles(8, &[(0, 16, &[], None)]);
    let run = |rec: &[u8], m: u8, index: u16| {
        let mut objs = vec![0u8; 0x400];
        objs[..rec.len()].copy_from_slice(rec);
        let (idx, mt) = inputs(&[(9, m, index)]);
        decode_tiles(&map, &objs, &idx, &mt)
    };
    let ok = obj(0, 2, &[0x0000], &[(3, &[0xFFFF])], None);
    assert!(run(&ok, 3, 0).is_ok());
    // an empty record: first row 16 is the file's own, 17 lies past the tile
    assert!(run(&obj(16, 2, &[], &[], None), 3, 0).is_ok());
    assert!(run(&obj(17, 2, &[], &[], None), 3, 0).is_err());
    // object masktype with a map-range index
    assert!(run(&ok, 3, 0x6000).is_err());
    // plane 5
    assert!(run(&obj(0, 2, &[0x0000], &[(5, &[0x0001])], None), 3, 0).is_err());
    // plane bits under a transparent mask bit
    assert!(run(&obj(0, 2, &[0x0001], &[(0, &[0x0001])], None), 3, 0).is_err());
    // record longer than the 0xCC-byte cache slot (16 rows, 6 plane blocks)
    let all: [u16; 16] = [0xFFFF; 16];
    let blocks: Vec<(u8, &[u16])> = [0, 1, 2, 3, 4, 0].iter().map(|&p| (p, &all[..])).collect();
    let big = obj(0, 2, &[0; 16], &blocks, None);
    assert!(big.len() > 0xCC);
    assert!(run(&big, 3, 0).is_err());
    // missing anim ref at the end of the file
    let tail = obj(0, 1, &[0x00], &[], None);
    let (idx, mt) = inputs(&[(9, 5, 0x0001)]);
    let mut objs = vec![0u8; 0x10];
    objs.extend_from_slice(&tail);
    assert!(decode_tiles(&map, &objs, &idx, &mt).is_err());
}

#[test]
fn gfx_masktype_lzw_or_decoded() {
    let map = maptiles(8, &[(0, 16, &[], None)]);
    let (idx, mt) = inputs(&[]);
    let packed = crate::lzw::encode(&mt).unwrap();
    assert_ne!(packed.len(), TILE_COUNT);
    assert_eq!(
        decode_tiles(&map, &[], &idx, &packed).unwrap(),
        decode_tiles(&map, &[], &idx, &mt).unwrap()
    );
    assert!(decode_tiles(&map, &[], &idx, &mt[..100]).is_err());
}

#[test]
fn gfx_animmask_overlay_and_frame() {
    // Block 1: one record at offset 2 (ref 0xB302): rows 3..5, frame f row i = f << i.
    let mut b1 = vec![0xEE, 0xEE, 3, 2];
    for f in 0..ANIM_FRAMES as u16 {
        for i in 0..2 {
            b1.extend_from_slice(&((1u16 << f) << (8 * i)).to_be_bytes());
        }
    }
    let b2 = vec![0, 0];
    let mut file = (b1.len() as u16).to_le_bytes().to_vec();
    file.extend_from_slice(&b1);
    file.extend_from_slice(&(b2.len() as u16).to_le_bytes());
    file.extend_from_slice(&b2);
    let am = AnimMask::parse(&file).unwrap();
    let ov = am.overlay(0xB302).unwrap();
    assert_eq!((ov.first_row, ov.rows), (3, 2));
    assert_eq!(ov.frames[7], vec![0x0080, 0x8000]);
    assert_eq!(am.overlay(0xFC00).unwrap().rows, 0);
    assert!(am.overlay(0xB2FF).is_err()); // negative offset
    assert!(am.overlay(0xB323).is_err()); // header runs off block 1
    assert!(AnimMask::parse(&file[..file.len() - 1]).is_err());

    let rows: [[u16; PLANES]; 16] = [[0xFFFF, 0, 0, 0, 0]; 16];
    let map = maptiles(64, &[(0, 0, &rows, Some(0xB302))]);
    let (idx, mt) = inputs(&[(0, 4, 0x6000)]);
    let set = decode_tiles(&map, &[], &idx, &mt).unwrap();
    let t = &set.tiles[0];
    let f0 = t.frame(&am, 8).unwrap(); // 8 wraps to frame 0
    assert_eq!(f0[3 * 16 + 15], 1 | 8);
    assert_eq!(f0[3 * 16 + 14], 1);
    assert_eq!(f0[4 * 16 + 7], 1 | 8);
    assert_eq!(t.frame(&am, 1).unwrap()[3 * 16 + 14], 1 | 8);
    assert_eq!(set.tiles[1].frame(&am, 3).unwrap(), set.tiles[1].pixels);
}

#[test]
fn gfx_palette_words_and_exe_checks() {
    let mut words = vec![0u8; 64];
    words[2..4].copy_from_slice(&0x0F80u16.to_be_bytes());
    words[62..64].copy_from_slice(&0x0FFFu16.to_be_bytes());
    let p = palette_from_ocs_words(&words).unwrap();
    assert_eq!(p[0], Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(
        p[1],
        Rgb {
            r: 255,
            g: 136,
            b: 0
        }
    );
    assert_eq!(
        p[31],
        Rgb {
            r: 255,
            g: 255,
            b: 255
        }
    );
    words[4] = 0x10;
    assert!(palette_from_ocs_words(&words).is_err());

    let mut exe = vec![0u8; GAME_EXE_LEN];
    exe[..4].copy_from_slice(&[0, 0, 3, 0xF3]);
    exe[GAME_HUNK33_BLOCK..GAME_HUNK33_BLOCK + 8]
        .copy_from_slice(&[0, 0, 3, 0xEA, 0, 0, 0x13, 0xC3]);
    exe[GAME_PALETTE_OFFSET + 62..GAME_PALETTE_OFFSET + 64].copy_from_slice(&[0x0F, 0xFF]);
    assert_eq!(
        palette(&exe).unwrap()[31],
        Rgb {
            r: 255,
            g: 255,
            b: 255
        }
    );
    let mut bad = exe.clone();
    bad[GAME_HUNK33_BLOCK + 7] = 0;
    assert!(palette(&bad).is_err());
    let mut bad = exe.clone();
    bad[GAME_PALETTE_OFFSET] = 0x10;
    assert!(palette(&bad).is_err());
    assert!(palette(&exe[1..]).is_err());
}

/// Directory with the PowerPacker-decrunched disk tree (`d1/INSTALL`, `d2/DATA1`), outside the repo.
const UNPACKED: &str = "/Users/zhugehyuk/Games/ultima6/unpacked";
/// Where the contact sheets go (outside the repo: they are game-derived).
const SHEET_DIR: &str = "/Users/zhugehyuk/Games/ultima6/re";

/// Paints tiles `range` into an RGB image, `cols` tiles per row, each scaled by `scale`, with a
/// 2-pixel grey gap. Transparent pixels get a dark teal checkerboard: the palette has magentas
/// but no teal, so teal reads as "not drawn".
fn sheet(
    set: &Tileset,
    pal: &[Rgb; PALETTE_LEN],
    am: &AnimMask,
    range: std::ops::Range<usize>,
    cols: usize,
    scale: usize,
) -> (u32, u32, Vec<u8>) {
    let cell = TILE_W * scale + 2;
    let rows = range.len().div_ceil(cols);
    let (w, h) = (cols * cell + 2, rows * cell + 2);
    let mut img = vec![0x40u8; w * h * 3];
    for (n, t) in range.clone().enumerate() {
        let tile = &set.tiles[t];
        let pixels = tile.frame(am, 0).unwrap();
        let (ox, oy) = (2 + (n % cols) * cell, 2 + (n / cols) * cell);
        for y in 0..TILE_H * scale {
            for x in 0..TILE_W * scale {
                let i = (y / scale) * TILE_W + x / scale;
                let rgb = if tile.transparent[i] {
                    if ((x / scale) + (y / scale)).is_multiple_of(2) {
                        [0x00, 0x80, 0x80]
                    } else {
                        [0x00, 0x50, 0x50]
                    }
                } else {
                    let c = pal[usize::from(pixels[i])];
                    [c.r, c.g, c.b]
                };
                let o = ((oy + y) * w + ox + x) * 3;
                img[o..o + 3].copy_from_slice(&rgb);
            }
        }
    }
    (w as u32, h as u32, img)
}

#[allow(clippy::disallowed_methods, clippy::disallowed_types)]
fn write_png(path: &std::path::Path, (w, h, data): (u32, u32, Vec<u8>)) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&data).unwrap();
}

#[test]
#[allow(clippy::disallowed_methods)]
fn gfx_real_data_all_tiles_and_contact_sheet() {
    let root = std::path::Path::new(UNPACKED);
    if !root.is_dir() {
        eprintln!("skipped: {UNPACKED} not present");
        return;
    }
    let read = |p: &str| std::fs::read(root.join(p)).unwrap();
    let set = decode_tiles(
        &read("d2/DATA1/maptiles.ega"),
        &read("d2/DATA1/objtiles.ega"),
        &read("d2/DATA1/tileindx.ega"),
        &read("d2/DATA1/masktype.ega"),
    )
    .unwrap();
    let am = AnimMask::parse(&read("d2/DATA1/animmask.ega")).unwrap();
    let pal = palette(&read("d1/INSTALL/game")).unwrap();

    assert_eq!(set.tiles.len(), TILE_COUNT);
    let count = |k: TileKind| set.tiles.iter().filter(|t| t.kind == k).count();
    assert_eq!(count(TileKind::Map), 488);
    assert_eq!(count(TileKind::Object(Span::Full)), 1470);
    assert_eq!(count(TileKind::Object(Span::Left)), 41);
    assert_eq!(count(TileKind::Object(Span::Right)), 49);
    let animated: Vec<&Tile> = set.tiles.iter().filter(|t| t.anim_ref.is_some()).collect();
    assert_eq!(animated.len(), 98);
    for t in &animated {
        let ov = am.overlay(t.anim_ref.unwrap()).unwrap();
        // Overlay bits land only on pixels the tile itself draws (measured: 0 of 12412 bits).
        for f in 0..ANIM_FRAMES {
            for (i, bits) in ov.frames[f].iter().enumerate() {
                let y = usize::from(ov.first_row) + i;
                for x in 0..TILE_W {
                    if bits & (0x8000 >> x) != 0 {
                        assert!(!t.transparent[y * TILE_W + x]);
                    }
                }
            }
        }
    }
    // Structure only (the table itself stays in the user's file): black first, white at 15.
    assert_eq!(pal[0], Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(
        pal[15],
        Rgb {
            r: 255,
            g: 255,
            b: 255
        }
    );

    let out = std::path::Path::new(SHEET_DIR);
    if out.is_dir() {
        write_png(
            &out.join("tiles.png"),
            sheet(&set, &pal, &am, 0..TILE_COUNT, 64, 2),
        );
        for page in 0..4 {
            let r = page * 512..(page + 1) * 512;
            write_png(
                &out.join(format!("tiles-{page}.png")),
                sheet(&set, &pal, &am, r, 32, 3),
            );
        }
    }
}
