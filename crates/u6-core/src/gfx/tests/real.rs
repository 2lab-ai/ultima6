//! Real data: the user's zip (skipped when absent; nothing is copied, only numbers asserted).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::path::Path;

use u6_formats::gfx::TileKind;

use crate::engine::Engine;
use crate::files::Files;
use crate::gfx::anim::{anim_tick, shore_mask};
use crate::gfx::seams::view_window;
use crate::gfx::tables::GameTables;
use crate::gfx::{GfxState, render_window};
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
/// The flattened image REA reads (local, derived; only compared against, never copied).
const IMAGE: &str = "/Users/zhugehyuk/Games/ultima6/re/game.bin";

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

#[test]
fn real_tables_match_the_image() {
    let Some(files) = files() else { return };
    let t = GameTables::from_game(files.get("game").unwrap()).unwrap();
    if let Ok(img) = std::fs::read(IMAGE) {
        let at = |a: usize, n: usize| img[a - 0x0010_0000..a - 0x0010_0000 + n].to_vec();
        let be = |v: &[u16]| v.iter().flat_map(|w| w.to_be_bytes()).collect::<Vec<_>>();
        assert_eq!(
            be(&t.shore_src),
            at(0x0015_2C52, 64),
            "no relocation in the table"
        );
        assert_eq!(be(&t.shore_dst), at(0x0015_2C92, 64));
        assert_eq!(be(&t.dissolve_taps), at(0x0015_1F3C, 30));
        assert_eq!(be(&[t.window_colour, t.window_shown, t.dissolve]), {
            let mut v = at(0x0015_5180, 2);
            v.extend(at(0x0015_3BF8, 2));
            v.extend(at(0x0015_5178, 2));
            v
        });
    } else {
        eprintln!("image comparison skipped: {IMAGE} not present");
    }
    // Shore merges: 4 sources, the 32 shore tiles 16..48 as targets.
    let mut src: Vec<u16> = t.shore_src.iter().map(|w| w / 4).collect();
    src.sort_unstable();
    src.dedup();
    let dst: Vec<u16> = t.shore_dst.iter().map(|w| w / 4).collect();
    println!("shore sources {src:?}, targets {}..={}", dst[0], dst[31]);
    assert!(t.shore_dst.iter().chain(&t.shore_src).all(|w| w % 4 == 0));
    assert_eq!(t.window_shown, 1);
}

#[test]
fn real_shore_masks_fit_and_the_window_draws_the_party() {
    let Some(files) = files() else { return };
    let mut state = State::new_game(&files).unwrap();
    let mut gfx = GfxState::load(&files).unwrap();
    let maps = gfx
        .records
        .iter()
        .filter(|r| r.kind == TileKind::Map)
        .count();
    assert_eq!(maps, 488);
    // Two ticks: the animdata frames reach the water tiles, the second (even) one merges.
    anim_tick(&mut state, &mut gfx);
    anim_tick(&mut state, &mut gfx);
    for i in 0..32 {
        let (skip, masks) = shore_mask(&gfx.animmask.block2, i).expect("mask record");
        assert_eq!(skip % 2, 0);
        let rows = usize::from(skip / 2) + masks.len();
        for w in [gfx.tables.shore_src[i], gfx.tables.shore_dst[i]] {
            let (rec, _, mt) = gfx.record(w / 4);
            assert_eq!((rec.kind, mt & 3), (TileKind::Map, 0), "merge {i}");
            assert!(rows <= usize::from(rec.rows), "merge {i}: {rows} rows");
        }
    }
    let window = view_window(&state);
    let centre = &window.objects[5 * 11 + 5];
    let avatar = state.tables.basetile.base[0x19a]
        + u16::from(state.objects.frame(crate::objects::ObjectId(1)));
    assert_eq!(
        centre.last(),
        Some(&avatar),
        "leader drawn last in cell (5, 5)"
    );
    let listed: usize = window.objects.iter().map(Vec::len).sum();
    println!(
        "centre {:?}: {listed} object tiles in the window, cell (5, 5) = {centre:?}",
        state.map.centre
    );
    let (rec, _, _) = gfx.record(avatar);
    let expect: Vec<(usize, usize, u8)> = (0..256)
        .filter(|&i| !rec.transparent[i])
        .map(|i| (80 + i % 16, 80 + i / 16, rec.pixels[i]))
        .collect();
    let e = {
        let mut e = Engine::with_game(State::empty(), files.clone(), move |_s, io| async move {
            render_window(&mut state, &mut gfx, &io, true);
        });
        e.tick(&[], 0);
        e
    };
    let fb = e.frame();
    assert!(expect.len() > 50);
    for (x, y, p) in expect {
        assert_eq!(fb.get(x, y), Some(p), "avatar pixel at ({x}, {y})");
    }
}
