//! Unit tests on synthetic object sets.

use super::*;
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
    // It moves one column east at 251 tiles into it, back west below 5 into the next one.
    load_region(&mut s, 250, 100, 0);
    assert_eq!(s.objects.surface_base, 0);
    load_region(&mut s, 251, 100, 0);
    assert_eq!(s.objects.surface_base, 1);
    let mut have = s.objects.loaded_surface;
    have.sort();
    assert_eq!(have, [1, 2, 9, 10]);
    assert_eq!(
        (s.objects.tmp_flag[0], s.objects.tmp_flag[8]),
        (1, 1),
        "written back"
    );
    load_region(&mut s, 133, 100, 0);
    assert_eq!(s.objects.surface_base, 1);
    load_region(&mut s, 132, 100, 0);
    assert_eq!(s.objects.surface_base, 0);
    load_region(&mut s, 251, 100, 0);
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
    assert_eq!(name_prefix(&s, loaf), ("3#".to_string(), 3));
    assert_eq!(display_name(&s, NONE), "<none>");
    s.actors.attrs.status = vec![0; 256];
    s.actors.attrs.status[3] = 0x80;
    s.actors.party.members[0] = 3;
    s.actors.party.size = 1;
    s.actors.party.names = vec!["Iolo".into()];
    assert_eq!(display_name(&s, 3), "Iolo");
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
    // Cell (5, 5): the plain object, the top tile (0x10) behind it; the wide object's west
    // half (mode 1, not a top tile) goes in front.
    assert_eq!(d.tiles(5, 5), [tile(20, 0) - 1, tile(10, 0), tile(21, 0)]);
    assert_eq!(d.tiles(6, 5), [tile(20, 0)]);
    // Hidden cells draw nothing.
    s.objects.draw.base[5 * 11 + 5] = 0xff;
    build_draw_lists(&mut s);
    assert!(s.objects.draw.tiles(5, 5).is_empty());
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
