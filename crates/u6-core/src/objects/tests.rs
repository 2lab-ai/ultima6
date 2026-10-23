//! Unit tests on synthetic object sets.

use super::*;
use crate::geom::{HALF_H, HALF_W, VIEW_H, VIEW_W};
use crate::state::State;
use u6_formats::basetile::BaseTiles;
use u6_formats::look::{LookEntry, LookTable};
use u6_formats::objblk::{ObjBlock, ObjBlockId, ObjRecord};
use u6_formats::tileflag::TileFlags;

/// Tile of type `t` frame `f` in the test world: 4 * t + f.
pub(super) fn tile(t: u16, f: u16) -> u16 {
    t * 4 + f
}

/// An empty world: tiles 4 * type + frame, no flags, the 8 compass directions, two stackable
/// types (60, 61) and slots reset.
pub(super) fn world() -> State {
    let mut s = State::empty();
    s.tables.basetile = BaseTiles {
        base: (0..1024).map(|t| t * 4).collect(),
    };
    s.tables.tileflags = TileFlags {
        flags1: vec![0; 2048],
        flags2: vec![0; 2048],
        weight: vec![0; 1024],
        flags3: vec![0; 2048],
    };
    let d = &mut s.objects.defs;
    d.dir_dx = vec![0, 1, 1, 1, 0, -1, -1, -1];
    d.dir_dy = vec![-1, -1, 0, 1, 1, 1, 0, -1];
    d.stackable = vec![60, 61];
    // Synthetic stand-ins for the game's strings.
    d.nothing = "<none>".into();
    d.nothing_lower = "<none-lc>".into();
    d.articles = ["A:".into(), "AN:".into(), "THE:".into()];
    d.count_format = "%d#".into();
    init_slots(&mut s.objects);
    s.map.centre.x = 500;
    s.map.centre.y = 500;
    s
}

/// Map object of type `t` at (x, y, 0).
pub(super) fn put(s: &mut State, t: u16, x: u16, y: u16) -> Slot {
    create_on_map(&mut s.objects, t, 0, 0, x, y, 0)
}

/// The list from the head, as slots.
pub(super) fn list(o: &Objects) -> Vec<Slot> {
    let mut v = Vec::new();
    let mut c = o.next_of(HEAD);
    while c >= 0 && v.len() < OBJECT_SLOTS {
        v.push(c);
        c = o.next_of(c);
    }
    v
}

/// Every map object in the list sorts at or after the previous one by (y, x).
fn assert_sorted(o: &Objects) {
    let keys: Vec<(u16, u16)> = list(o)
        .into_iter()
        .filter(|&c| o.place(c) == 0)
        .map(|c| (o.y(c), o.x(c)))
        .collect();
    assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");
}

fn at(s: &mut State, x: u16, y: u16) -> Vec<(u16, u16)> {
    let pos = u6_formats::position::Position { x, y, z: 0 };
    objects_at(s, pos)
        .into_iter()
        .map(|(id, part)| (id.0, part))
        .collect()
}

#[test]
fn init_chains_free_slots_and_empties_the_list() {
    let s = world();
    let o = &s.objects;
    assert_eq!(o.free, (0x101, 0xaff));
    assert_eq!(o.free_actors, (0xe0, 0x20));
    assert_eq!(o.next_of(HEAD), NONE);
    assert_eq!(o.next_of(0x101), 0x102);
    assert_eq!(o.next_of(0xbff), NONE);
    assert_eq!(o.next_of(0xff), NONE);
    assert_eq!(o.loaded_surface, [-1; 4]);
}

#[test]
fn created_objects_keep_the_list_sorted_and_head_their_cell() {
    let mut s = world();
    let a = put(&mut s, 10, 5, 7);
    let b = put(&mut s, 11, 3, 7);
    let c = put(&mut s, 12, 9, 2);
    let d = put(&mut s, 13, 5, 7);
    assert_eq!(list(&s.objects), [c, b, d, a]);
    assert_sorted(&s.objects);
    // The newest object at a tile comes first, and heads the grid cell.
    assert_eq!(s.objects.grid[7 * GRID_SIDE + 5], d);
    assert_eq!(s.objects.status(d), TEMPORARY);
    assert_eq!(s.objects.free.1, 0xaff - 4);
}

#[test]
fn location_iteration_yields_tile_then_neighbours_in_list_order() {
    let mut s = world();
    // Type 20 frame 0 is double width, type 21 double height, type 22 both.
    s.tables.tileflags.flags2[usize::from(tile(20, 0))] = 0x80;
    s.tables.tileflags.flags2[usize::from(tile(21, 0))] = 0x40;
    s.tables.tileflags.flags2[usize::from(tile(22, 0))] = 0xc0;
    let a = put(&mut s, 10, 5, 5);
    let b = put(&mut s, 11, 5, 5);
    let wide = put(&mut s, 20, 6, 5);
    let _plain_east = put(&mut s, 10, 6, 5);
    let tall = put(&mut s, 21, 5, 6);
    let both = put(&mut s, 22, 6, 6);
    let _far = put(&mut s, 10, 7, 5);
    // A contained object and one on another level at the tile are skipped.
    let bag = put(&mut s, 30, 5, 5);
    create_in(&mut s, 31, IN_CONTAINER, 0, bag);
    create_on_map(&mut s.objects, 32, 0, 0, 5, 5, 1);
    let got = at(&mut s, 5, 5);
    let ids = |v: &[Slot]| v.iter().map(|&c| c as u16).collect::<Vec<_>>();
    let order: Vec<u16> = got.iter().map(|g| g.0).collect();
    assert_eq!(order, ids(&[bag, b, a, wide, tall, both]));
    let parts: Vec<u16> = got.iter().map(|g| g.1).collect();
    assert_eq!(parts, [0, 0, 0, 1, 1, 3]);
}

#[test]
fn iteration_outside_the_window_walks_the_whole_list() {
    let mut s = world();
    let a = put(&mut s, 10, 300, 300);
    let b = put(&mut s, 11, 300, 300);
    put(&mut s, 12, 301, 300);
    assert_eq!(at(&mut s, 300, 300), [(b as u16, 0), (a as u16, 0)]);
    assert!(at(&mut s, 299, 300).is_empty());
}

#[test]
fn move_relinks_contents_and_repairs_both_cells() {
    let mut s = world();
    let a = put(&mut s, 10, 5, 5);
    let bag = put(&mut s, 30, 5, 5);
    let coin = create_in(&mut s, 31, IN_CONTAINER, 0, bag);
    let other = put(&mut s, 12, 8, 9);
    assert_eq!(s.objects.grid[5 * GRID_SIDE + 5], bag);
    assert_eq!(move_to(&mut s, bag, 8, 9, 0), bag);
    // The bag and its coin left (5, 5) together; the cell now starts with `a`.
    assert_eq!(s.objects.grid[5 * GRID_SIDE + 5], a);
    assert_eq!(s.objects.grid[9 * GRID_SIDE + 8], bag);
    assert_eq!(list(&s.objects), [a, bag, coin, other]);
    assert_eq!(s.objects.parent(coin), bag);
    assert_sorted(&s.objects);
    // Moving the last object off a tile empties its cell.
    move_to(&mut s, a, 20, 20, 0);
    assert_eq!(s.objects.grid[5 * GRID_SIDE + 5], NONE);
    // Type 0x4c never moves.
    let fixed = put(&mut s, 0x4c, 1, 1);
    assert_eq!(move_to(&mut s, fixed, 2, 2, 0), NONE);
    assert_eq!(s.objects.x(fixed), 1);
}

#[test]
fn containers_take_contents_and_refuse_cycles() {
    let mut s = world();
    let chest = put(&mut s, 30, 5, 5);
    let bag = put(&mut s, 31, 6, 5);
    let gem = put(&mut s, 32, 7, 5);
    assert_eq!(move_into(&mut s, gem, bag, IN_CONTAINER), gem);
    assert_eq!(move_into(&mut s, bag, chest, IN_CONTAINER), bag);
    assert_eq!(list(&s.objects), [chest, bag, gem]);
    assert!(s.objects.contained_in(gem, chest));
    assert_eq!(s.objects.last_inside(chest), gem);
    // Into itself or into something it holds: refused.
    assert_eq!(move_into(&mut s, chest, gem, IN_CONTAINER), NONE);
    assert_eq!(move_into(&mut s, bag, bag, IN_CONTAINER), NONE);
    // Direct children only; deeper contents are passed over.
    assert_eq!(first_child(&mut s.objects, chest), bag);
    assert_eq!(next_child(&mut s.objects), NONE);
    assert_eq!(first_child(&mut s.objects, bag), gem);
    // Removing the chest drops the bag (with the gem) on its tile and frees the slot.
    let free = s.objects.free.1;
    remove(&mut s, chest);
    assert_eq!(s.objects.tf(chest), 0);
    assert_eq!(s.objects.place(bag), 0);
    assert_eq!((s.objects.x(bag), s.objects.y(bag)), (5, 5));
    assert_eq!(s.objects.parent(gem), bag);
    assert_eq!(list(&s.objects), [bag, gem]);
    assert_eq!(s.objects.free, (chest, free + 1));
    assert_eq!(s.objects.last_moved_child, bag);
}

#[test]
fn actors_carry_inventory_and_stacks_fill_to_0x90() {
    let mut s = world();
    let actor = create_actor(&mut s, 0x100, 5, 5, 0);
    assert_eq!(actor, 0xe0);
    assert_eq!(s.objects.status(actor), 0x21);
    // Quantity stacks (type 60) fill one stack of the quality to 0x90, then start another.
    let a = add_to_owner(&mut s, actor, 60, 0x0380);
    assert_eq!(s.objects.qq_of(a), 0x0380);
    assert_eq!(quantity(&s.objects, a), 0x80);
    let b = add_to_owner(&mut s, actor, 60, 0x0320);
    assert_ne!(a, b);
    assert_eq!(s.objects.qq_of(a), 0x0390);
    assert_eq!(s.objects.qq_of(b), 0x0310);
    // Gold adds the whole word to the one gold object.
    let g = add_to_owner(&mut s, actor, names::GOLD, 300);
    assert_eq!(add_to_owner(&mut s, actor, names::GOLD, 200), g);
    assert_eq!(quantity(&s.objects, g), 500);
    // Anything else: one object per unit, into the inventory.
    add_to_owner(&mut s, actor, 70, 2);
    let mut kinds = Vec::new();
    let mut c = first_child(&mut s.objects, actor);
    while c >= 0 {
        assert_eq!(s.objects.place(c), IN_INVENTORY);
        kinds.push(s.objects.kind(c));
        c = next_child(&mut s.objects);
    }
    assert_eq!(kinds, [70, 70, names::GOLD, 60, 60]);
}

#[test]
fn list_order_puts_carried_objects_before_map_objects() {
    let mut s = world();
    let actor = create_actor(&mut s, 0x100, 5, 5, 0);
    let item = create_in(&mut s, 70, IN_INVENTORY, 0, actor);
    let floor = put(&mut s, 10, 5, 5);
    let o = &s.objects;
    // Anything carried sorts before every map object (0x10da94 re-attaches it afterwards).
    assert!(o.compare(actor, item) > 0);
    assert!(o.compare(item, actor) < 0);
    assert!(o.compare(floor, item) > 0);
    assert_eq!(o.compare(NONE, floor), 1);
    assert_eq!(o.compare(floor, NONE), -1);
    let east = put(&mut s, 10, 6, 5);
    assert!(s.objects.compare(floor, east) < 0);
}

#[test]
fn pack_xyz_keeps_the_68000_byte_overflow() {
    assert_eq!(pack_xyz(1023, 1023, 15).to_packed(), [0xff, 0xff, 0xff]);
    // x = 1024: x >> 8 = 4 lands in y's lowest bit.
    let p = pack_xyz(1024, 0, 0);
    assert_eq!((p.x, p.y), (0, 1));
}

/// A block of records: `(status, x, y, z, type, qq)`.
fn block(recs: &[(u8, u16, u16, u8, u16, u16)]) -> ObjBlock {
    ObjBlock {
        objects: recs
            .iter()
            .map(|&(status, x, y, z, obj_n, qq)| ObjRecord {
                status,
                pos: u6_formats::position::Position { x, y, z },
                obj_n,
                frame: 0,
                quality: (qq >> 8) as u8,
                qty: qq as u8,
            })
            .collect(),
    }
}

fn with_blocks(s: &mut State) {
    s.objects.blocks = ObjBlockId::all()
        .map(|id| (id, ObjBlock { objects: vec![] }))
        .collect();
}

#[test]
fn a_block_loads_into_sorted_slots_and_writes_back() {
    let mut s = world();
    with_blocks(&mut s);
    // Superchunk 0: a chest at (9, 9) holding a gem (record index 0), two loose objects.
    s.objects.blocks[0].1 = block(&[
        (0x00, 9, 9, 0, 30, 0),
        (0x08, 0, 0, 0, 32, 0x0105),
        (0x01, 3, 9, 0, 33, 0),
        (0x00, 50, 2, 0, 34, 0),
    ]);
    load_block(&mut s.objects, 0);
    let l = list(&s.objects);
    let kinds: Vec<u16> = l.iter().map(|&c| s.objects.kind(c)).collect();
    assert_eq!(kinds, [34, 33, 30, 32]);
    assert_eq!(
        s.objects.parent(l[3]),
        l[2],
        "record index 0 became the chest's slot"
    );
    assert_eq!(s.objects.qq_of(l[3]), 0x0105);
    assert_eq!(s.objects.free.1, 0xaff - 4);
    write_back_block(&mut s, 0);
    assert!(list(&s.objects).is_empty());
    assert_eq!(s.objects.free.1, 0xaff);
    assert_eq!(s.objects.tmp_flag[0], 1);
    // The .tmp file holds the four records in list order, the gem pointing at record 2.
    let tmp = s.objects.tmp[0].clone().unwrap();
    let back = ObjBlock::parse(&tmp).unwrap();
    assert_eq!(back.objects.len(), 4);
    assert_eq!(back.objects[3].container_index(), Some(2));
    // Reading again comes from the .tmp file.
    load_block(&mut s.objects, 0);
    assert_eq!(list(&s.objects).len(), 4);
}

#[test]
fn inventory_loaded_from_a_block_follows_its_actor() {
    let mut s = world();
    with_blocks(&mut s);
    // Actor 7 stands at (20, 20); the block holds its sword and a rock under it.
    s.objects.type_frame[7] = 0x150;
    s.objects.set_xyz(7, 20, 20, 0);
    link_actors(&mut s.objects);
    s.objects.blocks[0].1 = block(&[(0x00, 20, 20, 0, 40, 0), (0x10, 7, 0, 0, 41, 0)]);
    load_block(&mut s.objects, 0);
    s.objects.fix_inventory();
    let kinds: Vec<u16> = list(&s.objects)
        .iter()
        .map(|&c| s.objects.kind(c))
        .collect();
    assert_eq!(kinds, [40, 0x150, 41]);
}

#[test]
fn region_load_keeps_the_four_nearest_superchunks() {
    let mut s = world();
    with_blocks(&mut s);
    for b in 0..64u16 {
        let (x, y) = ((b % 8) * 128 + 10, (b / 8) * 128 + 10);
        s.objects.blocks[usize::from(b)].1 = block(&[(0, x, y, 0, 100 + b, 0)]);
    }
    // The base is the superchunk holding (x - 64, y - 64).
    load_region(&mut s, 100, 100, 0);
    assert_eq!(s.objects.surface_base, 0);
    let mut have = s.objects.loaded_surface;
    have.sort();
    assert_eq!(have, [0, 1, 8, 9]);
    assert_eq!(list(&s.objects).len(), 4);
    // It moves one column east at HALF_W tiles from the block's east edge (Original 251 tiles
    // into it), back west below HALF_W into the next one (Original 5).
    let (east, west) = (256 - HALF_W as u16, 128 + HALF_W as u16);
    load_region(&mut s, east - 1, 100, 0);
    assert_eq!(s.objects.surface_base, 0);
    load_region(&mut s, east, 100, 0);
    assert_eq!(s.objects.surface_base, 1);
    let mut have = s.objects.loaded_surface;
    have.sort();
    assert_eq!(have, [1, 2, 9, 10]);
    assert_eq!(
        (s.objects.tmp_flag[0], s.objects.tmp_flag[8]),
        (1, 1),
        "written back"
    );
    load_region(&mut s, west, 100, 0);
    assert_eq!(s.objects.surface_base, 1);
    load_region(&mut s, west - 1, 100, 0);
    assert_eq!(s.objects.surface_base, 0);
    load_region(&mut s, east, 100, 0);
    let kinds: Vec<u16> = list(&s.objects)
        .iter()
        .map(|&c| s.objects.kind(c))
        .collect();
    assert_eq!(kinds, [101, 102, 109, 110]);
}

#[test]
fn look_text_takes_the_next_entry_and_names_actors_by_party_name() {
    let mut s = world();
    s.tables.look = LookTable {
        entries: vec![
            LookEntry {
                tile: 0,
                text: "zero".into(),
            },
            LookEntry {
                tile: 40,
                text: "loa/f\\ves".into(),
            },
        ],
        trailer: vec![0x00, 0x08, b'z', 0],
    };
    assert_eq!(look_text(&s, 39), "loa/f\\ves");
    assert_eq!(
        look_text(&s, 41),
        "z",
        "past the last entry the terminator's text"
    );
    s.tables.tileflags.flags3[usize::from(tile(10, 0))] = 0x40;
    let loaf = put(&mut s, 10, 1, 1);
    assert_eq!(described_name(&s, loaf), "A:loaf");
    s.objects.type_frame[loaf as usize] = 60 | (2 << 10);
    s.objects.qq[loaf as usize] = 3;
    // Type 60 is a quantity stack only for frame 0 (0x10cef2 compares the whole word): the
    // prefix counts by type, the quantity by type + frame.
    assert_eq!(name_prefix_text(&s, loaf), ("3#".to_string(), 3));
    assert_eq!(display_name(&s, NONE), "<none>");
    s.actors.attrs.status = vec![0; 256];
    s.actors.attrs.status[3] = 0x80;
    s.actors.party.members[0] = 3;
    s.actors.party.size = 1;
    s.actors.party.names = vec!["<member 3>".into()];
    assert_eq!(display_name(&s, 3), "<member 3>");
}

#[test]
fn korean_names_have_no_article_and_no_plural_letters() {
    let mut s = world();
    s.tables.look = LookTable {
        entries: vec![LookEntry {
            tile: 40,
            text: "loa/f\\ves".into(),
        }],
        trailer: vec![0x00, 0x08, b'z', 0],
    };
    s.tables.tileflags.flags3[usize::from(tile(10, 0))] = 0x40;
    let loaf = put(&mut s, 10, 1, 1);
    assert_eq!(described_name(&s, loaf), "A:loaf");
    assert_eq!(article_text(&s, tile(10, 0)), "A:");
    s.ui.text.set_language(crate::ui::Language::Korean);
    // Spec 08: the article is empty, `/` and `\\` and the letters after them print nothing.
    assert_eq!(article_text(&s, tile(10, 0)), "");
    assert_eq!(described_name(&s, loaf), "loa");
    s.ui.text.set_language(crate::ui::Language::English);
    assert_eq!(described_name(&s, loaf), "A:loaf");
}

#[test]
fn draw_lists_put_wide_objects_in_two_cells_and_top_tiles_last() {
    let mut s = world();
    s.tables.tileflags.flags2[usize::from(tile(20, 0))] = 0x80;
    s.tables.tileflags.flags2[usize::from(tile(21, 0))] = 0x10;
    let (lx, ly) = (500, 500);
    put(&mut s, 21, lx, ly);
    put(&mut s, 20, lx + 1, ly);
    put(&mut s, 10, lx, ly);
    build_draw_lists(&mut s);
    let d = &s.objects.draw;
    // The leader's cell (HALF_W, HALF_H): the plain object, the top tile (0x10) behind it; the
    // wide object's west half (mode 1, not a top tile) goes in front.
    let (hx, hy) = (HALF_W, HALF_H);
    assert_eq!(d.tiles(hx, hy), [tile(20, 0) - 1, tile(10, 0), tile(21, 0)]);
    assert_eq!(d.tiles(hx + 1, hy), [tile(20, 0)]);
    // Hidden cells draw nothing.
    s.objects.draw.base[hy * VIEW_W + hx] = 0xff;
    build_draw_lists(&mut s);
    assert!(s.objects.draw.tiles(hx, hy).is_empty());
}

/// The objects drawn, over every view cell.
fn drawn(s: &State) -> usize {
    (0..VIEW_H)
        .flat_map(|row| (0..VIEW_W).map(move |col| (col, row)))
        .map(|(col, row)| s.objects.draw.tiles(col, row).len())
        .sum()
}

/// A view with an object on every cell, more than a byte of node numbers: each object in its
/// own cell. Wide draws all of them; Original's pool of 256 nodes draws the first 255, as the
/// original does (one object a cell is 121 there, so it takes three a cell).
#[test]
fn an_all_object_view_draws_every_object_the_pool_holds() {
    let mut s = world();
    let (lx, ly) = (s.map.centre.x, s.map.centre.y);
    let per_cell = (usize::from(u8::MAX) + 1).div_ceil(DRAW_CELLS);
    let mut placed = 0;
    for row in 0..VIEW_H {
        for col in 0..VIEW_W {
            let (x, y) = (
                lx + col as u16 - HALF_W as u16,
                ly + row as u16 - HALF_H as u16,
            );
            for k in 0..per_cell {
                // Type 0x80 + i / 4, frame i % 4: tile 0x200 + i, one of its own for each object
                // (types without a rule of their own in the builder).
                let i = (row * VIEW_W + col) * per_cell + k;
                let tf = (0x80 + i / 4) as u16 | ((i % 4) as u16) << 10;
                create_on_map(&mut s.objects, tf, 0, 0, x, y, 0);
                placed += 1;
            }
        }
    }
    assert!(placed > usize::from(u8::MAX), "{placed} objects");
    build_draw_lists(&mut s);
    let n = drawn(&s);
    if cfg!(feature = "wide") {
        assert_eq!(n, placed, "every object drawn");
        for row in 0..VIEW_H {
            for col in 0..VIEW_W {
                let i = row * VIEW_W + col;
                assert_eq!(
                    s.objects.draw.tiles(col, row),
                    [0x200 + i as u16],
                    "cell ({col}, {row})"
                );
            }
        }
    } else {
        assert_eq!((n, DRAW_NODES), (255, 256), "the original's pool");
    }
}

/// The view reaches [`HALF_W`] columns and [`HALF_H`] rows either side of the leader: the
/// corners are drawn in the corner cells, one tile past each side is not. Wide's view is wider
/// than tall (37 × 21, spec 13 Decision 1): an object [`HALF_H`] + 1 columns east is drawn
/// there and not in Original. Fails with [`VIEW_W`] and [`VIEW_H`] swapped.
#[test]
fn the_view_reaches_half_w_columns_and_half_h_rows_from_the_leader() {
    let mut s = world();
    let (lx, ly) = (s.map.centre.x as i16, s.map.centre.y as i16);
    let (hw, hh) = (HALF_W as i16, HALF_H as i16);
    let corners = [(-hw, -hh), (hw, -hh), (-hw, hh), (hw, hh)];
    for (k, &(dx, dy)) in corners.iter().enumerate() {
        put(&mut s, 30 + k as u16, (lx + dx) as u16, (ly + dy) as u16);
    }
    for (dx, dy) in [(-hw - 1, 0), (hw + 1, 0), (0, -hh - 1), (0, hh + 1)] {
        put(&mut s, 40, (lx + dx) as u16, (ly + dy) as u16);
    }
    build_draw_lists(&mut s);
    let d = &s.objects.draw;
    let (w, h) = (VIEW_W - 1, VIEW_H - 1);
    for (k, (col, row)) in [(0, 0), (w, 0), (0, h), (w, h)].into_iter().enumerate() {
        assert_eq!(d.tiles(col, row), [tile(30 + k as u16, 0)], "corner {k}");
    }
    assert_eq!(drawn(&s), 4, "nothing past the sides");
    let mut s = world();
    put(&mut s, 50, (lx + hh + 1) as u16, ly as u16);
    put(&mut s, 51, lx as u16, (ly + hh + 1) as u16);
    build_draw_lists(&mut s);
    if cfg!(feature = "wide") {
        let east = s.objects.draw.tiles(HALF_W + HALF_H + 1, HALF_H);
        assert_eq!(
            east,
            [tile(50, 0)],
            "east of the leader past the rows' half"
        );
        assert_eq!(drawn(&s), 1, "south of it past the rows' half: not drawn");
    } else {
        assert_eq!(drawn(&s), 0, "both past the half view");
    }
}

/// The superchunks of the loaded 2 × 2 block, which hold view cell (x, y) (every surface
/// coordinate wraps at 1024).
fn loaded_holds(s: &State, x: i32, y: i32) -> bool {
    let (x, y) = (x.rem_euclid(1024), y.rem_euclid(1024));
    let b = ((y >> 7) & 7) * 8 + ((x >> 7) & 7);
    s.objects.loaded_surface.contains(&(b as i16))
}

/// Every view cell round the leader at (x, y) lies in the loaded blocks.
fn view_loaded(s: &State, x: u16, y: u16) -> bool {
    let (hw, hh) = (HALF_W as i32, HALF_H as i32);
    (-hh..=hh).all(|dy| (-hw..=hw).all(|dx| loaded_holds(s, i32::from(x) + dx, i32::from(y) + dy)))
}

/// The region objects follow the party with the half view as margin on each axis: every view
/// cell lies inside the loaded superchunks for a leader one tile either side of each
/// superchunk edge, on a fresh load and walking across the whole world each way (twice round,
/// past the wrap at 1024).
#[test]
fn the_loaded_blocks_hold_the_whole_view_at_every_superchunk_edge() {
    let mut s = world();
    with_blocks(&mut s);
    for edge in (0..8u16).map(|k| k * 128) {
        for d in [-1i16, 0, 1] {
            for (x, y) in [(edge as i16 + d, 300), (300, edge as i16 + d)] {
                let (x, y) = ((x & 0x3ff) as u16, (y & 0x3ff) as u16);
                unload_region_objects(&mut s);
                load_region(&mut s, x, y, 0);
                assert!(view_loaded(&s, x, y), "fresh load at ({x}, {y})");
            }
        }
    }
    for axis in [0, 1] {
        for step in [1i32, -1] {
            unload_region_objects(&mut s);
            for i in 0..2048 {
                let a = (300 + step * i).rem_euclid(1024) as u16;
                let (x, y) = if axis == 0 { (a, 300) } else { (300, a) };
                load_region(&mut s, x, y, 0);
                assert!(view_loaded(&s, x, y), "walking to ({x}, {y})");
            }
        }
    }
    // The hysteresis stays the half view: the base moves only past it.
    unload_region_objects(&mut s);
    load_region(&mut s, 100, 100, 0);
    assert_eq!(s.objects.surface_base, 0);
    let east = 256 - HALF_W as u16;
    load_region(&mut s, east - 1, 100, 0);
    assert_eq!(
        s.objects.surface_base,
        0,
        "{} from the east edge",
        HALF_W + 1
    );
    load_region(&mut s, east, 100, 0);
    assert_eq!(s.objects.surface_base, 1, "{HALF_W} from the east edge");
    let south = 256 - HALF_H as u16;
    load_region(&mut s, east, south - 1, 0);
    assert_eq!(
        s.objects.surface_base,
        1,
        "{} from the south edge",
        HALF_H + 1
    );
    load_region(&mut s, east, south, 0);
    assert_eq!(s.objects.surface_base, 9, "{HALF_H} from the south edge");
}

#[test]
fn small_helpers() {
    assert_eq!(actor_frame(0x16a, 2), 19);
    assert_eq!(actor_frame(0x200, 5), 5);
    assert_eq!(actor_frame(0x100, 5), 0);
    let mut s = world();
    let before = s.rng;
    let v = vary(&mut s, 10);
    assert!((5..=15).contains(&v));
    let mut r = before;
    r.next_word();
    r.next_word();
    assert_eq!(s.rng, r, "two draws");
    s.objects.defs.monster_types = vec![9, 7, 0, 5];
    assert_eq!(monster_index(&s.objects, 7), 1);
    assert_eq!(monster_index(&s.objects, 5), -1);
}

#[test]
fn type_frame_word_splits_10_and_6_bits() {
    let mut o = Objects::default();
    o.type_frame[3] = 0x3ff | (0x2a << 10);
    assert_eq!(o.obj_type(ObjectId(3)), 0x3ff);
    assert_eq!(o.frame(ObjectId(3)), 0x2a);
    assert_eq!(o.obj_type(ObjectId(OBJECT_SLOTS as u16)), 0);
    assert_eq!(o.location(ObjectId(OBJECT_SLOTS as u16)), None);
    assert_eq!(o.next_of(NONE), NONE);
}

#[test]
fn taking_and_counting_by_type_at_any_depth() {
    let mut s = world();
    let actor = create_actor(&mut s, 0x100, 5, 5, 0);
    let bag = create_in(&mut s, 31, IN_INVENTORY, 0, actor);
    let deep = create_in(&mut s, 60, IN_CONTAINER, 0x0005, bag);
    let top = add_to_owner(&mut s, actor, 60, 0x0007);
    assert_eq!(count_of_kind(&mut s.objects, actor, 60), 12);
    // 9 of 12: the first stack in list order (7) goes, the next gives up 2.
    let last = take_from_owner(&mut s, actor, 60, 9);
    assert_eq!(s.objects.tf(top), 0);
    assert_eq!(last, deep);
    assert_eq!(s.objects.qq_of(deep), 0x0003);
    assert_eq!(count_of_kind(&mut s.objects, actor, 60), 3);
    assert_eq!(count_of_kind(&mut s.objects, bag, 60), 3);
}

/// Runs `f` on a watchdog thread and gives its result, or fails the test when it does not
/// return within 10 s: a regression to an endless walk fails instead of stalling the suite.
#[allow(clippy::disallowed_methods)] // test only: the watchdog thread
pub(super) fn returns<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or_else(|e| panic!("{what} did not return: {e}"))
}

/// Two objects "inside" each other (status 0x08, each the other's parent), off the map: a
/// chain 0x10c928 and its kin walk without a bound in the original.
fn cyclic_pair(s: &mut State) -> (Slot, Slot) {
    let a = put(s, 0x63, 7, 7);
    let b = put(s, 0x63, 8, 8);
    s.objects.obj_status[a as usize] = IN_CONTAINER;
    s.objects.obj_status[b as usize] = IN_CONTAINER;
    s.objects.set_parent(a, b);
    s.objects.set_parent(b, a);
    (a, b)
}

#[test]
fn a_cyclic_holder_chain_is_not_inside_anything_else() {
    let (inside, other_first) = returns("contained_in", || {
        let mut s = world();
        let (a, b) = cyclic_pair(&mut s);
        let c = put(&mut s, 0x63, 9, 9);
        (s.objects.contained_in(a, b), s.objects.contained_in(a, c))
    });
    assert!(inside, "a well-formed step of the chain still counts");
    assert!(!other_first, "an owner the chain never reaches");
}

#[test]
fn sorting_an_object_on_a_cyclic_chain_ends() {
    let r = returns("compare", || {
        let mut s = world();
        let (a, _) = cyclic_pair(&mut s);
        let c = put(&mut s, 0x63, 9, 9);
        s.objects.compare(a, c)
    });
    assert!((-1..=1).contains(&r.signum()));
}

#[test]
fn a_body_on_a_cyclic_chain_raises_nobody() {
    let (got, actor_at, flag, body_kind) = returns("actor_from_body", || {
        let mut s = world();
        let (body, _) = cyclic_pair(&mut s);
        s.objects.qq[body as usize] = 5 << 8;
        s.objects.type_frame[5] = 0x19a;
        s.objects.set_xyz(5, 40, 40, 0);
        let got = actor_from_body(&mut s, body);
        let at = (s.objects.x(5), s.objects.y(5));
        (got, at, s.objects.allow_type_4c, s.objects.kind(body))
    });
    assert_eq!(got, NONE);
    assert_eq!(actor_at, (40, 40), "the actor stays where it was");
    assert_eq!(flag, 0, "type 0x4c refused again");
    assert_eq!(body_kind, 0x63, "the body stays");
}
