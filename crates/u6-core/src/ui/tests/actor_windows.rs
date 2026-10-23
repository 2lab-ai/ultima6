//! Spec 14: the side panel's three views in their slots below the UI (Wide), on synthetic data:
//! a redraw draws each view into its slot as a draw of that view alone would, and leaves the
//! state as a redraw of the current view alone; a press in a slot acts as the same press in the
//! panel showing that view. Original has no slot.

use super::*;

/// Original: no slot, no slot region; the frame is the UI's screen (`geom` checks its size).
#[cfg(not(feature = "wide"))]
#[test]
fn original_has_no_slot_and_no_slot_region() {
    assert!(crate::geom::SLOTS.is_empty());
    assert!(pointer::SLOT_REGIONS.is_empty());
    let s = world();
    for i in 0..8 {
        let r = pointer::region(&s.ui.pointer.tables, i);
        assert_eq!(r.is_some(), i < 4, "region {i}");
    }
}

/// Original: a redraw in the roster mode draws the roster alone; the inventory's cells, count and
/// context stay as they were.
#[cfg(not(feature = "wide"))]
#[test]
fn original_roster_redraw_leaves_the_inventory_alone() {
    let mut s = world();
    s.inventory.cells[0] = 7;
    s.ui.panel.inventory_first = 4;
    s.ui.panel.inventory_count = 9;
    let before = (s.inventory.clone(), s.ui.panel.inventory_first);
    let e = run(s, &[], 4, |st, io| async move {
        panels::redraw_panel(&st, &io).await;
    });
    let st = e.state();
    assert_eq!((st.inventory.clone(), st.ui.panel.inventory_first), before);
    assert_eq!(st.ui.panel.inventory_count, 9);
}

/// The synthetic world of both builds: tile = 4 × type, two members (actors 1 and 2), member 0
/// holding a two-handed weapon readied and, listed in this order, a ring, a bag with two pebbles
/// in it and fourteen pebbles (sixteen list entries); every tile record a whole opaque square
/// of its own colour; click regions of made-up rects but the side panel's, which is the panel
/// area; the roster shown, member 0's inventory drawn before (Wide: the inventory view's last
/// base, spec 14 Design 3).
fn world() -> State {
    use crate::objects::{IN_CONTAINER, IN_INVENTORY, READIED};
    use pointer::{PointerState, PointerTables};
    let mut s = state();
    s.tables.basetile = u6_formats::basetile::BaseTiles {
        base: (0..1024).map(|t| t * 4).collect(),
    };
    let mut weight = vec![0u8; 1024];
    for (t, w) in [(TWO_HANDED, 60), (RING, 1), (BAG, 5), (PEBBLE, 1)] {
        weight[usize::from(t)] = w;
    }
    s.tables.tileflags = u6_formats::tileflag::TileFlags {
        flags1: vec![0; 2048],
        flags2: vec![0; 2048],
        weight,
        flags3: vec![0; 2048],
    };
    s.objects.defs.type_set_670c = vec![BAG];
    for (r, t) in s.gfx.records.iter_mut().enumerate() {
        *t = u6_formats::gfx::Tile {
            masktype: 0,
            kind: u6_formats::gfx::TileKind::Map,
            first_row: 0,
            rows: 16,
            pixels: vec![3 + (r % 12) as u8; 256],
            transparent: vec![false; 256],
            unused_byte: None,
            anim_ref: None,
        };
    }
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    let p = &mut s.actors.party;
    p.size = 2;
    p.members[..2].copy_from_slice(&[1, 2]);
    p.names = vec!["Ab".into(), "Cd".into()];
    let a = &mut s.actors.attrs;
    for (i, ty, hp) in [(1usize, 0x178u16, 30u8), (2, 0x17a, 8)] {
        a.base_type_frame[i] = ty;
        a.hp[i] = hp;
        a.strength[i] = 20;
        a.status[i] = 0x80;
        a.combat_mode[i] = 2;
    }
    for o in [1usize, 2] {
        s.objects.type_frame[o] = s.actors.attrs.base_type_frame[o];
    }
    // Each object is linked right after its owner: made last, listed first.
    give(&mut s, TWO_HANDED, READIED, 0, 1);
    for _ in 0..14 {
        give(&mut s, PEBBLE, IN_INVENTORY, 0, 1);
    }
    let bag = give(&mut s, BAG, IN_INVENTORY, 0, 1);
    give(&mut s, PEBBLE, IN_CONTAINER, 0, bag);
    give(&mut s, PEBBLE, IN_CONTAINER, 0, bag);
    give(&mut s, RING, IN_INVENTORY, 0, 1);
    s.ui.pointer = PointerState::from_tables(PointerTables {
        regions: vec![
            [0, 0, 0, 0],
            [0, 0xb0, 0x9f, 0xbf],
            [0xa8, 6, 0x137, 0x67],
            [0, 0, 319, 199],
        ],
        shapes: Vec::new(),
        walk_cell: (-1, -1),
        shown: false,
    });
    s.ui.panel.mode = 0x91;
    s.ui.panel.member = 0;
    s.ui.panel.container = 1;
    s.ui.panel.dirty = true;
    #[cfg(feature = "wide")]
    {
        s.ui.panel.slots.base = Some(1);
    }
    s
}

/// An object of type `ty` with `status` and quality / quantity `qq` in `owner`, linked right
/// after it (listed first).
fn give(s: &mut State, ty: u16, status: u8, qq: u16, owner: i16) -> i16 {
    let o = crate::objects::create_in(s, ty, status, qq, owner);
    assert!(o > 0, "a slot for {ty:#x}");
    o
}

/// Synthetic object types: a two-handed weapon (tile 0x228), a ring (tile 0x258), a container
/// and a pebble that readies nowhere.
const TWO_HANDED: u16 = 0x8a;
const RING: u16 = 0x96;
const BAG: u16 = 0x30;
const PEBBLE: u16 = 0x41;

#[cfg(feature = "wide")]
mod wide {
    use std::cell::RefCell;
    use std::rc::Rc;

    use u6_render::{Framebuffer, Kind, Src};

    use super::super::*;
    use super::{BAG, PEBBLE, RING, TWO_HANDED, give, world};

    /// A synthetic food: one of the types Use eats (`usecmd/cases.rs`).
    const FOOD: u16 = 0x80;
    /// A slot press case of [`a_slot_press_acts_as_the_panel_press_in_that_view`]: its label,
    /// the state, the slot, the panel pixel pressed, and what the press does there.
    type Case = (
        &'static str,
        State,
        usize,
        (i16, i16),
        Box<dyn Fn(&State) -> bool>,
    );
    /// [`command_outcome`]: the selection, the objects' status, places, types and quantities,
    /// the inventory context.
    type Outcome = (i16, Vec<u8>, Vec<(u16, u16)>, Vec<u16>, Vec<u16>, (u16, u8));
    use crate::objects::{READIED, Slot};

    /// The side panel area's top left and size, in frame pixels.
    const PANEL: (usize, usize) = (0xa8, 6);
    const SIZE: (usize, usize) = (144, 98);
    /// Spec 14 Design 1's slot origins, copied from the spec: roster, stats, inventory.
    const SPEC_SLOTS: [(usize, usize); 3] = [(8, 206), (160, 206), (8, 310)];
    /// The views of the slots, in the spec's order.
    const SPEC_VIEWS: [u16; 3] = [0x91, 0x90, 0x92];

    /// What a run leaves: the state, the screen buffer with its source plane, and the codes the
    /// game recorded.
    struct Run {
        state: State,
        screen: Framebuffer,
        codes: Vec<u16>,
    }

    /// Runs `game` on `s` (source plane on, `events` queued on the first tick) to its end.
    fn play<G, F>(s: State, events: &[InputEvent], game: G) -> Run
    where
        G: FnOnce(Rc<RefCell<State>>, Rc<Io>, Rc<RefCell<Vec<u16>>>) -> F + 'static,
        F: Future<Output = ()> + 'static,
    {
        let screen = Rc::new(RefCell::new(None));
        let codes = Rc::new(RefCell::new(Vec::new()));
        let (out, got) = (Rc::clone(&screen), Rc::clone(&codes));
        let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
            game(Rc::clone(&st), Rc::clone(&io), got).await;
            *out.borrow_mut() = Some(io.draw(|fb| fb.clone()));
        });
        e.enable_src_plane();
        e.tick(events, 16);
        for _ in 0..400 {
            if e.finished() {
                break;
            }
            e.tick(&[], 16);
        }
        assert!(e.finished(), "the game ended");
        let state = e.state().clone();
        let screen = screen.borrow_mut().take().expect("the screen");
        let codes = codes.borrow().clone();
        Run {
            state,
            screen,
            codes,
        }
    }

    /// A redraw of `s` (Wide: the three views).
    fn redraw(s: State) -> Run {
        play(s, &[], |st, io, _| async move {
            panels::redraw_panel(&st, &io).await;
        })
    }

    /// A redraw of `s` with view `view` drawn alone, as 0x103f64 draws it in Original.
    fn alone(mut s: State, view: u16) -> Run {
        s.ui.panel.mode = view;
        s.ui.panel.dirty = true;
        play(s, &[], |st, io, _| async move {
            panels::redraw_alone(&st, &io).await;
        })
    }

    /// The pixels and sources of the slot-sized rect at `at` of `fb`.
    fn rect(fb: &Framebuffer, at: (usize, usize)) -> Vec<(Option<u8>, Option<Src>)> {
        (0..SIZE.1)
            .flat_map(|dy| {
                (0..SIZE.0).map(move |dx| {
                    (
                        fb.get(at.0 + dx, at.1 + dy),
                        fb.src_at(at.0 + dx, at.1 + dy),
                    )
                })
            })
            .collect()
    }

    /// The pixels of rect `a` of `fa` that differ from rect `b` of `fb` (colour or source).
    fn differing(
        fa: &Framebuffer,
        a: (usize, usize),
        fb: &Framebuffer,
        b: (usize, usize),
    ) -> usize {
        rect(fa, a)
            .iter()
            .zip(rect(fb, b))
            .filter(|(x, y)| **x != *y)
            .count()
    }

    /// The frame pixel of panel pixel (x, y) in slot `slot` (the spec's origins).
    fn in_slot(slot: usize, (x, y): (i16, i16)) -> (i16, i16) {
        let (sx, sy) = SPEC_SLOTS[slot];
        (
            x - PANEL.0 as i16 + sx as i16,
            y - PANEL.1 as i16 + sy as i16,
        )
    }

    /// A press and release at (x, y), then the key `a`.
    fn click_then_key(x: i16, y: i16) -> Vec<InputEvent> {
        let at = |buttons| InputEvent::Pointer { x, y, buttons };
        vec![at(1), at(0), InputEvent::Key(Key::Char('a'))]
    }

    /// A redraw (the slots filled, as the command loop's before its key read), then the key read
    /// of `events`; the codes the key read gave.
    fn press(s: State, events: &[InputEvent]) -> Run {
        play(s, events, |st, io, codes| async move {
            panels::redraw_panel(&st, &io).await;
            let k = crate::ui::wait::read_key(&st, &io).await;
            codes.borrow_mut().push(k);
        })
    }

    /// The list cell of `obj` as the inventory view of `s` draws it.
    fn cell_of(s: &State, obj: Slot) -> (i16, i16) {
        let r = alone(s.clone(), 0x92);
        let k = r.state.inventory.cells[..12]
            .iter()
            .position(|&c| c == obj)
            .expect("listed") as i16;
        ((k & 3) * 16 + 0xf8 + 8, (k >> 2) * 16 + 0x20 + 8)
    }

    /// Member 0's object of type `ty` (the first).
    fn object(s: &State, ty: u16) -> Slot {
        (0x100..s.objects.type_frame.len() as Slot)
            .find(|&o| s.objects.kind(o) == ty && s.objects.place(o) != 0)
            .expect("the object")
    }

    #[test]
    fn each_slot_holds_its_view_and_the_panel_the_current_one() {
        for mode in SPEC_VIEWS {
            let mut pre = world();
            pre.ui.panel.mode = mode;
            let wide = redraw(pre.clone());
            for (slot, &view) in SPEC_VIEWS.iter().enumerate() {
                let r = alone(pre.clone(), view);
                let off = differing(&wide.screen, SPEC_SLOTS[slot], &r.screen, PANEL);
                assert_eq!(
                    off, 0,
                    "mode {mode:#x}: slot {slot} as view {view:#x} alone"
                );
            }
            let r = alone(pre.clone(), mode);
            let off = differing(&wide.screen, PANEL, &r.screen, PANEL);
            assert_eq!(off, 0, "mode {mode:#x}: the panel area shows it");
            // The roster's sky bar (its rows above the first member's figure) is drawn whatever
            // the mode.
            let (sx, sy) = SPEC_SLOTS[0];
            let sky = (sx + 8..sx + 0x88).any(|x| {
                (sy..sy + 16).all(|y| {
                    wide.screen
                        .src_at(x, y)
                        .is_some_and(|s| s.kind == Kind::Tile)
                })
            });
            assert!(sky, "mode {mode:#x}: the roster's sky bar");
            assert!(!wide.state.ui.panel.dirty);
            assert_eq!(wide.state.ui.panel.mode, mode);
        }
        // The views differ, so each comparison above names one.
        let pre = world();
        let rs: Vec<Run> = SPEC_VIEWS.iter().map(|&v| alone(pre.clone(), v)).collect();
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            assert_ne!(
                differing(&rs[i].screen, PANEL, &rs[j].screen, PANEL),
                0,
                "views {i} and {j}"
            );
        }
    }

    /// Spec 14 rev 3: the text in the slots is on the HD font's 8-pixel grid. With the HD font
    /// mode on, the compositor lists in each slot the cells it lists in the panel area of that
    /// view drawn alone (`u6_render::hd` `glyph_cell`), moved by the slot's offset.
    #[test]
    fn the_slots_text_shows_in_the_hd_font() {
        use u6_render::hd::{Compositor, NoImages, Scale};
        let listed = |fb: &Framebuffer, at: (usize, usize)| -> Vec<(usize, usize, u16)> {
            let mut c = Compositor::new();
            c.set_font_mode(true);
            c.compose(fb, &mut NoImages, Scale::X2);
            c.glyphs()
                .iter()
                .map(|g| (usize::from(g.x), usize::from(g.y), g.code))
                .filter(|&(x, y, _)| {
                    (at.0..at.0 + SIZE.0).contains(&x) && (at.1..at.1 + SIZE.1).contains(&y)
                })
                .map(|(x, y, code)| (x - at.0, y - at.1, code))
                .collect()
        };
        for mode in SPEC_VIEWS {
            let mut pre = world();
            pre.ui.panel.mode = mode;
            let wide = redraw(pre.clone());
            for (slot, &view) in SPEC_VIEWS.iter().enumerate() {
                let want = listed(&alone(pre.clone(), view).screen, PANEL);
                assert!(want.len() > 3, "view {view:#x} lists its text");
                let got = listed(&wide.screen, SPEC_SLOTS[slot]);
                assert_eq!(got, want, "mode {mode:#x}: slot {slot}");
            }
        }
    }

    /// Spec 14 rev 3 Design 6: while a conversation runs, its portrait stays in the panel area:
    /// a panel code from a press in the panel area (a member's row, 0x90: the key read marks
    /// the panel dirty, Design 4), a press in a slot and a redraw draw nothing there or in the
    /// slots, and the panel stays dirty; the redraw after the conversation draws the panel.
    #[test]
    fn a_conversation_keeps_its_portrait_in_the_panel_area() {
        let portrait = |fb: &Framebuffer| {
            (PANEL.1..PANEL.1 + SIZE.1)
                .all(|y| (PANEL.0..PANEL.0 + SIZE.0).all(|x| fb.get(x, y) == Some(9)))
        };
        let row = (0xd0, 0x1c + 4);
        let slot = in_slot(2, (0xf4, 0x4c));
        let mut events = click_then_key(row.0, row.1);
        events.truncate(2);
        events.extend(click_then_key(slot.0, slot.1));
        let before = Rc::new(RefCell::new(None));
        let during = Rc::new(RefCell::new(None));
        let (b, d) = (Rc::clone(&before), Rc::clone(&during));
        let r = play(world(), &events, move |st, io, codes| async move {
            panels::redraw_panel(&st, &io).await;
            {
                let mut s = st.borrow_mut();
                s.converse.active = true;
                s.ui.panel.dirty = true;
            }
            io.draw(|fb| {
                for y in PANEL.1..PANEL.1 + SIZE.1 {
                    for x in PANEL.0..PANEL.0 + SIZE.0 {
                        fb.set(x, y, 9);
                    }
                }
            });
            *b.borrow_mut() = Some(io.draw(|fb| fb.clone()));
            let k = crate::ui::wait::read_key(&st, &io).await;
            codes.borrow_mut().push(k);
            panels::redraw_panel(&st, &io).await;
            let (dirty, mode) = {
                let s = st.borrow();
                (s.ui.panel.dirty, s.ui.panel.mode)
            };
            *d.borrow_mut() = Some((io.draw(|fb| fb.clone()), dirty, mode));
            st.borrow_mut().converse.active = false;
            panels::redraw_panel(&st, &io).await;
        });
        assert_eq!(r.codes, [0x41], "the key after the presses");
        let before = before.borrow_mut().take().expect("before");
        let (during, dirty, mode) = during.borrow_mut().take().expect("during");
        assert!(portrait(&during), "the portrait stays");
        assert_eq!(
            (dirty, mode),
            (true, 0x90),
            "the member's row taken, the panel dirty"
        );
        for (i, &at) in SPEC_SLOTS.iter().enumerate() {
            assert_eq!(differing(&during, at, &before, at), 0, "slot {i} untouched");
        }
        assert!(
            !portrait(&r.screen),
            "the panel drawn after the conversation"
        );
        assert!(!r.state.ui.panel.dirty);
    }

    /// A bag of member 0's open (container) on the second page, kept by a redraw, then `moved`
    /// with it: the container and page the next redraw leaves.
    fn after_the_bag(moved: impl FnOnce(&mut State, Slot)) -> (u16, u8) {
        let mut pre = world();
        let bag = object(&pre, BAG);
        pre.ui.panel.container = bag as u16;
        pre.ui.panel.inventory_first = 4;
        for _ in 0..12 {
            give(&mut pre, PEBBLE, crate::objects::IN_CONTAINER, 0, bag);
        }
        let kept = redraw(pre).state;
        assert_eq!(
            (kept.ui.panel.container, kept.ui.panel.inventory_first),
            (bag as u16, 4),
            "kept before"
        );
        let mut s = kept;
        moved(&mut s, bag);
        s.ui.panel.dirty = true;
        let r = redraw(s).state;
        (r.ui.panel.container, r.ui.panel.inventory_first)
    }

    /// Spec 14 Design 3: the kept bag taken by the other member: member 0's actor, the first row.
    #[test]
    fn a_bag_taken_by_another_member_starts_the_context_again() {
        use crate::objects::{IN_INVENTORY, move_into};
        let got = after_the_bag(|s, bag| {
            move_into(s, bag, 2, IN_INVENTORY);
            assert_eq!(
                (s.objects.parent(bag), s.objects.place(bag)),
                (2, IN_INVENTORY)
            );
        });
        assert_eq!(got, (1, 0));
    }

    /// Spec 14 Design 3: the kept bag left on the map: member 0's actor, the first row.
    #[test]
    fn a_bag_left_on_the_map_starts_the_context_again() {
        let got = after_the_bag(|s, bag| {
            crate::objects::move_to(s, bag, 3, 4, 0);
            assert_eq!(s.objects.place(bag), 0, "on the map");
        });
        assert_eq!(got, (1, 0));
    }

    /// Spec 14 Design 3: a Restore (0x1052c8) with a bag open and a later page: the slots'
    /// record is cleared, and the next redraw shows member 0's actor from the first row (the old
    /// object slots name other objects after the reload).
    #[test]
    fn a_restore_starts_the_inventory_context_again() {
        let mut pre = world();
        let bag = object(&pre, BAG);
        pre.ui.panel.container = bag as u16;
        pre.ui.panel.inventory_first = 4;
        let objlist = crate::saveload::objlist::encode(&pre);
        pre.objects.blocks = u6_formats::objblk::ObjBlockId::all()
            .map(|id| {
                (
                    id,
                    u6_formats::objblk::ObjBlock {
                        objects: Vec::new(),
                    },
                )
            })
            .collect();
        let mut files = std::collections::BTreeMap::new();
        files.insert("schedule".to_string(), vec![0; 514]);
        files.insert("basetile".to_string(), vec![0; 0x800]);
        files.insert("DATA1/objlist".to_string(), objlist);
        let got = Rc::new(RefCell::new(None));
        let out = Rc::clone(&got);
        let mut e = Engine::with_game(pre, Files::new(files), move |st, io| async move {
            crate::saveload::restore(&mut st.borrow_mut(), &io).expect("the synthetic files");
            let cleared = st.borrow().ui.panel.slots == Default::default();
            panels::redraw_panel(&st, &io).await;
            let s = st.borrow();
            *out.borrow_mut() = Some((
                cleared,
                s.actors.party.members[0],
                s.ui.panel.container,
                s.ui.panel.inventory_first,
            ));
        });
        for _ in 0..40 {
            if e.finished() {
                break;
            }
            e.tick(&[], 16);
        }
        let (cleared, actor, container, first) = got.borrow().expect("restored");
        assert!(cleared, "the slots' record cleared by the restore");
        assert_eq!(actor, 1, "member 0 is actor 1 again");
        assert_eq!(
            (container, first),
            (1, 0),
            "member 0's actor, the first row"
        );
    }

    /// Spec 14 rev 3 Design 6, a real Talk (`converse::talk`, 0x1127c4) with a made-up script and
    /// made-up messages: the portrait panel (0x128ce8) is drawn with the conversation flag up
    /// and the roster mode back; while the conversation reads its input (a press on a member's
    /// row in the panel area, a press in the party slot, then the line that ends it), the panel
    /// area keeps the portrait panel and no slot changes; after it, the next redraw (the command
    /// loop's) draws the roster there again.
    #[test]
    fn a_real_talk_keeps_its_portrait_until_it_ends() {
        use crate::converse::text::{ConverseText, MESSAGES, Msg};
        use crate::converse::{Cx, talk};
        let mut pre = world();
        pre.converse.text = Some(ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
            let t: &[u8] = match m {
                Msg::Delimiters => b" ",
                Msg::Bye => b"bye",
                Msg::Newline => b"\n",
                Msg::TwoNewlines => b"\n\n",
                Msg::YouSay => b"\n>",
                Msg::ConverseFile => b"talkfile.?",
                Msg::PortraitFile => b"facefile.?",
                Msg::WordEnds => b" ,.",
                _ => b"-",
            };
            (m, t.to_vec())
        })));
        pre.converse.script = vec![0; crate::converse::SCRIPT_LEN];
        let cells = crate::geom::VIEW_W * crate::geom::VIEW_H;
        pre.objects.draw.base.resize(cells, 0);
        pre.objects.draw.cell.resize(cells, 0);
        pre.actors.objlist_tail[0x79] = 0xff;
        pre.actors.attrs.status[3] = 0;
        pre.objects.type_frame[3] = 0x178;
        // A made-up talk file: a library of u32 offsets, item 3 the made-up script of actor 3.
        let script = u6_formats::lzw::encode(b"\xff\x03Ef\xf1look\xf2hi").expect("packed");
        let mut lib = Vec::new();
        for i in 0..8u32 {
            let off: u32 = if i == 3 { 8 * 4 } else { 0 };
            lib.extend_from_slice(&off.to_le_bytes());
        }
        lib.extend(script);
        let mut files = std::collections::BTreeMap::new();
        files.insert("talkfile.a".to_string(), lib);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let mut e = Engine::with_game(pre, Files::new(files), move |st, io| async move {
            panels::redraw_panel(&st, &io).await;
            talk(&Cx::new(&st, &io), 0, 3, false).await;
            log.borrow_mut()
                .push((io.draw(|fb| fb.clone()), st.borrow().clone()));
            st.borrow_mut().ui.panel.dirty = true;
            panels::redraw_panel(&st, &io).await;
            log.borrow_mut()
                .push((io.draw(|fb| fb.clone()), st.borrow().clone()));
        });
        e.enable_src_plane();
        e.tick(&[], 16);
        let mut frames = Vec::new();
        let mut during = |e: &mut Engine, events: &[InputEvent]| {
            e.tick(events, 16);
            for _ in 0..20 {
                e.tick(&[], 16);
            }
            let s = e.state();
            assert!(s.converse.active, "the conversation still runs");
            frames.push((e.frame().clone(), s.ui.panel.mode, s.ui.panel.dirty));
        };
        during(&mut e, &[]);
        let row = (0xd0, 0x1c + 4);
        let slot = in_slot(0, row);
        let keys = |t: &str| {
            t.chars()
                .map(|c| InputEvent::Key(Key::Char(c)))
                .collect::<Vec<_>>()
        };
        during(&mut e, &click(row.0, row.1));
        during(&mut e, &click(slot.0, slot.1));
        let mut bye = keys("bye");
        bye.push(InputEvent::Key(Key::Enter));
        e.tick(&bye, 16);
        for _ in 0..200 {
            if e.finished() {
                break;
            }
            e.tick(&[], 16);
        }
        assert!(e.finished(), "the conversation ended");
        let (first, mode, dirty) = &frames[0];
        assert_eq!(
            (*mode, *dirty),
            (0x91, true),
            "the portrait drawn, the mode back, dirty"
        );
        let roster = alone(world(), 0x91);
        assert_ne!(
            differing(first, PANEL, &roster.screen, PANEL),
            0,
            "the portrait panel"
        );
        for (i, (f, _, _)) in frames.iter().enumerate().skip(1) {
            assert_eq!(
                differing(f, PANEL, first, PANEL),
                0,
                "input {i}: the portrait kept"
            );
            for (k, &at) in SPEC_SLOTS.iter().enumerate() {
                assert_eq!(differing(f, at, first, at), 0, "input {i}: slot {k} kept");
            }
        }
        let seen = seen.borrow();
        let (after, s) = &seen[1];
        assert!(!s.converse.active && !s.ui.panel.dirty);
        let want = alone(s.clone(), s.ui.panel.mode);
        assert_eq!(
            differing(after, PANEL, &want.screen, PANEL),
            0,
            "the panel drawn after it"
        );
    }

    /// Spec 14 Design 2: the redraw's state equals a redraw of the current view alone, field by
    /// field, but for the slots' own record; with a line pending in the message scroll, the
    /// party underground (the sky bar's wind), a two-handed weapon readied (the both-hands flag)
    /// and a scrolled list (the inventory context, kept).
    #[test]
    fn a_redraw_leaves_the_state_of_a_redraw_of_the_current_view_alone() {
        for mode in SPEC_VIEWS {
            let mut pre = world();
            pre.ui.panel.mode = mode;
            pre.ui.panel.inventory_first = 4;
            pre.map.centre.z = 1;
            pre.actors.wind = 3;
            let pending = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
                printf(&st, &io, b"xy", &[]).await;
                (st, io)
            };
            let a = play(pre.clone(), &[], move |st, io, _| async move {
                let (st, io) = pending(st, io).await;
                panels::redraw_panel(&st, &io).await;
            });
            let b = play(pre.clone(), &[], move |st, io, _| async move {
                let (st, io) = pending(st, io).await;
                panels::redraw_alone(&st, &io).await;
            });
            let (mut a, mut b) = (a.state, b.state);
            assert_eq!(
                (a.ui.panel.container, a.ui.panel.inventory_first),
                (1, 4),
                "mode {mode:#x}: the context kept"
            );
            for s in [&mut a, &mut b] {
                s.ui.panel.slots = Default::default();
            }
            (b.ui.panel.container, b.ui.panel.inventory_first) =
                (a.ui.panel.container, a.ui.panel.inventory_first);
            let fields = [
                ("tables", a.tables == b.tables),
                ("objects", a.objects == b.objects),
                ("actors", a.actors == b.actors),
                ("map", a.map == b.map),
                ("ui.text", a.ui.text == b.ui.text),
                ("ui.panel", a.ui.panel == b.ui.panel),
                ("ui.input", a.ui.input == b.ui.input),
                ("ui.pointer", a.ui.pointer == b.ui.pointer),
                ("ui.messages", a.ui.messages == b.ui.messages),
                ("ui", a.ui == b.ui),
                ("converse", a.converse == b.converse),
                ("gfx", a.gfx == b.gfx),
                ("main", a.main == b.main),
                ("magic", a.magic == b.magic),
                ("inventory", a.inventory == b.inventory),
                ("sound", a.sound == b.sound),
                ("rng", a.rng == b.rng),
                ("combat", a.combat == b.combat),
                ("objcmds", a.objcmds == b.objcmds),
                ("saveload", a.saveload == b.saveload),
                ("speech", a.speech == b.speech),
                ("locale", a.locale == b.locale),
            ];
            for (field, same) in fields {
                assert!(same, "mode {mode:#x}: {field}");
            }
            assert!(a == b, "mode {mode:#x}: the whole state");
            if mode == 0x91 {
                assert_eq!(a.actors.wind, 0xff, "the roster's sky bar ran underground");
            }
        }
    }

    /// Scroll the backpack past twelve from its slot while the roster is shown (0x96), then any
    /// dirty redraw: the slot shows the second page, `inventory_first` is 4, and the roster is
    /// still the panel's view.
    #[test]
    fn a_page_scrolled_in_the_slot_stays_through_every_redraw() {
        let pre = world();
        let (x, y) = in_slot(2, (0xf4, 0x4c));
        let r = play(
            pre.clone(),
            &click_then_key(x, y),
            |st, io, codes| async move {
                panels::redraw_panel(&st, &io).await;
                let k = crate::ui::wait::read_key(&st, &io).await;
                codes.borrow_mut().push(k);
                st.borrow_mut().ui.panel.dirty = true;
                panels::redraw_panel(&st, &io).await;
            },
        );
        let s = &r.state;
        assert_eq!(r.codes, [0x41], "the key after the click");
        assert_eq!(s.ui.panel.inventory_first, 4, "the second page");
        assert_eq!(s.ui.panel.mode, 0x91, "the roster still shown");
        assert_eq!(
            s.ui.input.pointer,
            (x, y),
            "the pointer where the press was"
        );
        let mut page = pre.clone();
        page.ui.panel.inventory_first = 4;
        let want = alone(page, 0x92);
        assert_eq!(differing(&r.screen, SPEC_SLOTS[2], &want.screen, PANEL), 0);
        let first = alone(pre, 0x92);
        assert_ne!(differing(&r.screen, SPEC_SLOTS[2], &first.screen, PANEL), 0);
    }

    /// Spec 14 Design 3: the page stays while the member shown does, and starts again (the
    /// member's actor, the first page) when it changes: here the next member, from the attributes
    /// slot's icon (0x99) while the roster is shown.
    #[test]
    fn the_inventory_context_starts_again_when_the_member_changes() {
        let mut pre = world();
        pre.ui.panel.inventory_first = 4;
        let kept = redraw(pre.clone()).state;
        assert_eq!(
            (kept.ui.panel.container, kept.ui.panel.inventory_first),
            (1, 4),
            "kept"
        );
        let (x, y) = in_slot(1, (0xe8, 0x5c));
        let r = press(kept, &click_then_key(x, y));
        let s = &r.state;
        assert_eq!((s.ui.panel.member, s.ui.panel.mode), (1, 0x91));
        assert_eq!(
            (s.ui.panel.container, s.ui.panel.inventory_first),
            (2, 0),
            "the next member's actor, from the first row"
        );
    }

    /// Ready (0xa7) and unready (0xa8) in the key read while the roster is the panel's view: the
    /// codes come from presses in the backpack slot (no key gives them in the roster mode,
    /// `ui/click.rs` 0x10619c); the backpack slot shows the change before the next key is read.
    #[test]
    fn ready_and_unready_with_the_roster_shown_reach_the_slot_in_the_same_turn() {
        let pre = world();
        let ring = object(&pre, RING);
        let weapon = object(&pre, TWO_HANDED);
        for (obj, at, readied) in [
            (ring, cell_of(&pre, ring), true),
            (weapon, (0xb8, 0x30), false),
        ] {
            let (x, y) = in_slot(2, at);
            let r = press(pre.clone(), &click_then_key(x, y));
            let s = &r.state;
            assert_eq!(r.codes, [0x41]);
            assert_eq!(s.objects.place(obj) == READIED, readied, "{obj}");
            assert_eq!(s.ui.panel.mode, 0x91);
            let want = alone(s.clone(), 0x92);
            assert_eq!(
                differing(&r.screen, SPEC_SLOTS[2], &want.screen, PANEL),
                0,
                "{obj}: the slot redrawn before the key"
            );
            let before = alone(pre.clone(), 0x92);
            assert_ne!(
                differing(&r.screen, SPEC_SLOTS[2], &before.screen, PANEL),
                0
            );
        }
    }

    /// What a press leaves that a test compares: the selection, every object's status, the
    /// member shown, the page, the container, the members' combat modes and the key read's
    /// codes.
    fn outcome(r: &Run) -> (i16, Vec<u8>, i16, u8, u16, Vec<u8>, Vec<u16>) {
        let s = &r.state;
        (
            s.ui.input.selection,
            s.objects.obj_status.clone(),
            s.ui.panel.member,
            s.ui.panel.inventory_first,
            s.ui.panel.container,
            s.actors.attrs.combat_mode[1..3].to_vec(),
            r.codes.clone(),
        )
    }

    /// Spec 14 Design 5: a press in a slot selects the same object and does the same as the same
    /// press in the panel showing that view: in the backpack slot a list cell (ready), an
    /// equipment cell (unready), the down arrow, the container figure, each bottom icon and the
    /// combat-mode icon; in the party slot a member's row; in the attributes slot an icon. The
    /// panel's view then is the press's code's, or the one shown before; the pointer is back
    /// where the press was.
    #[test]
    fn a_slot_press_acts_as_the_panel_press_in_that_view() {
        let pre = world();
        let ring = object(&pre, RING);
        let mut second = pre.clone();
        second.ui.panel.member = 1;
        let weapon = object(&pre, TWO_HANDED);
        type Done = Box<dyn Fn(&State) -> bool>;
        let member = |m: i16| -> Done { Box::new(move |s: &State| s.ui.panel.member == m) };
        let mode = |m: u16, n: i16| -> Done {
            Box::new(move |s: &State| (s.ui.panel.mode, s.ui.panel.member) == (m, n))
        };
        let cases: [Case; 11] = [
            (
                "list cell",
                pre.clone(),
                2,
                cell_of(&pre, ring),
                Box::new(move |s: &State| s.objects.place(ring) == READIED),
            ),
            (
                "equipment cell",
                pre.clone(),
                2,
                (0xb8, 0x30),
                Box::new(move |s: &State| s.objects.place(weapon) == crate::objects::IN_INVENTORY),
            ),
            (
                "down arrow",
                pre.clone(),
                2,
                (0xf4, 0x4c),
                Box::new(|s: &State| s.ui.panel.inventory_first == 4),
            ),
            (
                "container figure",
                pre.clone(),
                2,
                (0x118, 0x18),
                mode(0x91, 0),
            ),
            (
                "previous member",
                second.clone(),
                2,
                (0xb8, 0x5c),
                member(0),
            ),
            ("roster icon", pre.clone(), 2, (0xc8, 0x5c), mode(0x91, 0)),
            ("stats icon", pre.clone(), 2, (0xd8, 0x5c), mode(0x90, 0)),
            ("next member", pre.clone(), 2, (0xe8, 0x5c), member(1)),
            (
                "combat mode",
                second.clone(),
                2,
                (0xf8, 0x5c),
                Box::new(|s: &State| s.actors.attrs.combat_mode[2] == 3),
            ),
            (
                "member row",
                pre.clone(),
                0,
                (0xd0, 0x1c + 16 + 4),
                mode(0x90, 1),
            ),
            (
                "next member from stats",
                pre.clone(),
                1,
                (0xe8, 0x5c),
                member(1),
            ),
        ];
        for (label, s, slot, at, done) in cases {
            let view = SPEC_VIEWS[slot];
            let shown = if view == 0x91 { 0x92 } else { 0x91 };
            let mut panel = s.clone();
            panel.ui.panel.mode = view;
            assert!(!done(&panel), "{label}: not done before");
            let by_panel = press(panel, &click_then_key(at.0, at.1));
            assert!(done(&by_panel.state), "{label}: the panel's press did it");
            let mut other = s.clone();
            other.ui.panel.mode = shown;
            let (x, y) = in_slot(slot, at);
            let by_slot = press(other, &click_then_key(x, y));
            assert_eq!(outcome(&by_slot), outcome(&by_panel), "{label}");
            let (after_panel, after_slot) =
                (by_panel.state.ui.panel.mode, by_slot.state.ui.panel.mode);
            let want = if after_panel == view {
                shown
            } else {
                after_panel
            };
            assert_eq!(after_slot, want, "{label}: the panel's view after");
            assert_eq!(
                by_slot.state.ui.input.pointer,
                (x, y),
                "{label}: the pointer"
            );
            assert!(
                by_slot.state.ui.panel.slots.press.is_none(),
                "{label}: ended"
            );
            assert!(!by_slot.state.ui.panel.dirty, "{label}: redrawn");
        }
    }

    /// Spec 14 Design 4: a panel code the key read handles in place (Ready, 0xa7, from a press
    /// in the panel area in the inventory mode, drawn there by 0x111be0) marks the panel dirty,
    /// so the slots show it before the next key: the backpack's and the party's.
    #[test]
    fn a_panel_code_handled_in_place_reaches_the_slots() {
        let mut pre = world();
        pre.ui.panel.mode = 0x92;
        let ring = object(&pre, RING);
        let (x, y) = cell_of(&pre, ring);
        let r = press(pre.clone(), &click_then_key(x, y));
        let s = &r.state;
        assert_eq!(r.codes, [0x41]);
        assert_eq!(s.objects.place(ring), READIED);
        assert!(!s.ui.panel.dirty);
        for (slot, &view) in SPEC_VIEWS.iter().enumerate() {
            let want = alone(s.clone(), view);
            let off = differing(&r.screen, SPEC_SLOTS[slot], &want.screen, PANEL);
            assert_eq!(off, 0, "slot {slot}");
        }
        let before = alone(pre, 0x92);
        assert_ne!(
            differing(&r.screen, SPEC_SLOTS[2], &before.screen, PANEL),
            0
        );
    }

    /// Spec 14 Design 4: each command the command loop completes marks the panel dirty, so the
    /// next redraw (at the loop's top) fills the slots, here after a pass with the panel left
    /// clean: before it nothing is in the slots, after it the three views are.
    #[test]
    fn a_completed_command_redraws_the_slots() {
        let mut s = world();
        s.ui.panel.dirty = false;
        s.actors.worktype[1] = 2;
        s.actors.movement_points[1] = 20;
        s.actors.attrs.dexterity[1] = 10;
        let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
            crate::main_loop::commands::command_loop(&st, &io).await;
        });
        e.enable_src_plane();
        e.tick(&[], 16);
        let blank = |e: &Engine| {
            let (sx, sy) = SPEC_SLOTS[0];
            (sy..sy + SIZE.1).all(|y| (sx..sx + SIZE.0).all(|x| e.frame().get(x, y) == Some(0)))
        };
        assert!(blank(&e), "no redraw before the command");
        e.tick(&[InputEvent::Key(Key::Char(' '))], 16);
        let mut waiting = false;
        for _ in 0..200 {
            e.tick(&[], 16);
            if e.state().main.reading == 1 && e.io().pending_events() == 0 {
                waiting = true;
                break;
            }
        }
        assert!(waiting, "the loop waits for the next key");
        e.tick(&[], 16);
        assert!(!blank(&e), "the slots filled after the command");
        let st = e.state().clone();
        for (slot, &view) in SPEC_VIEWS.iter().enumerate() {
            let want = alone(st.clone(), view);
            let off = differing(e.frame(), SPEC_SLOTS[slot], &want.screen, PANEL);
            assert_eq!(off, 0, "slot {slot}");
        }
    }

    /// [`world`] for the command loop: member 0 leads with movement points, the keys `u` and `d`
    /// on raw keys of their own, and Use's list of usable types (A4-0x66dc,
    /// `objects/commands/keys.rs` `in_type_list`) holding [`FOOD`] alone, in a made-up image.
    fn with_commands(mut s: State) -> State {
        s.ui.tables.keymap[0x22] = [0x75, 0x55, 0, 0];
        s.ui.tables.keymap[0x23] = [0x64, 0x44, 0, 0];
        s.actors.worktype[1] = 2;
        s.actors.movement_points[1] = 20;
        s.actors.attrs.dexterity[1] = 10;
        let list = crate::image::A4.wrapping_sub(0x66dc);
        s.objcmds.data =
            crate::objects::commands::GameData::with_bytes(&[(list, &FOOD.to_be_bytes())]);
        s.objects.defs.stackable = vec![FOOD];
        s
    }

    /// The command loop on `s`, driven: each step's events queued on one tick, then ticks until
    /// its condition holds with the queue empty (`label` names a step that never gets there).
    /// The engine, and the state at the end of each step.
    fn command_steps(s: State, steps: Vec<Step>) -> (Engine, Vec<State>) {
        let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
            crate::main_loop::commands::command_loop(&st, &io).await;
        });
        e.enable_src_plane();
        e.tick(&[], 16);
        let mut states = Vec::new();
        for (label, events, until) in steps {
            e.tick(&events, 16);
            let mut there = false;
            for _ in 0..400 {
                e.tick(&[], 16);
                if e.io().pending_events() == 0 && until(&e.state()) {
                    there = true;
                    break;
                }
            }
            assert!(there, "{label}");
            states.push(e.state().clone());
        }
        e.tick(&[], 16);
        (e, states)
    }

    /// One step of [`command_steps`]: its label, its events, the condition it ends at.
    type Step = (&'static str, Vec<InputEvent>, fn(&State) -> bool);

    /// The command waits for its target (A4-0x7a32 set, the key read inside the command).
    fn waits_for_a_target(s: &State) -> bool {
        s.ui.input.target_pending == 1 && s.main.reading == 0
    }

    /// The loop is back at its key read, no target pending.
    fn at_the_prompt(s: &State) -> bool {
        s.ui.input.target_pending == 0 && s.main.reading == 1
    }

    /// A press and release at (x, y).
    fn click(x: i16, y: i16) -> Vec<InputEvent> {
        let at = |buttons| InputEvent::Pointer { x, y, buttons };
        vec![at(1), at(0)]
    }

    /// What a command leaves that the slot and the panel must agree on: the selection, every
    /// object's status, parent or place, type and quality / quantity, and the inventory context.
    fn command_outcome(s: &State) -> Outcome {
        let o = &s.objects;
        (
            s.ui.input.selection,
            o.obj_status.clone(),
            o.location.iter().map(|p| (p.x, p.y)).collect(),
            o.type_frame.clone(),
            o.qq.clone(),
            (s.ui.panel.container, s.ui.panel.inventory_first),
        )
    }

    /// Spec 14 Design 5, the real Use: `u`, then the target pending (0x88 waits), a press on a
    /// food of member 0's (a stack of three) in the panel showing the inventory (0x92), or in
    /// the backpack slot while the roster is shown: the press confirms it (`panel_click`'s 0x8e
    /// from `target_from_pointer`), Use eats one (`usecmd/food.rs` `eat`), the command ends and
    /// the loop is back at its prompt. Both select the food and leave the same objects and
    /// inventory context; the slot's way leaves the roster shown, and its slot shows the stack.
    #[test]
    fn use_from_the_slot_eats_what_use_from_the_panel_eats() {
        let mut pre = with_commands(world());
        let food = give(&mut pre, FOOD, crate::objects::IN_INVENTORY, 3, 1);
        let at = cell_of(&pre, food);
        let run = |mode: u16, (x, y): (i16, i16)| {
            let mut s = pre.clone();
            s.ui.panel.mode = mode;
            command_steps(
                s,
                vec![
                    (
                        "Use asks for its target",
                        vec![InputEvent::Key(Key::Char('u'))],
                        waits_for_a_target,
                    ),
                    ("Use ran", click(x, y), at_the_prompt),
                ],
            )
            .0
        };
        let by_panel = run(0x92, at);
        let by_slot = run(0x91, in_slot(2, at));
        for (label, e) in [("panel", &by_panel), ("slot", &by_slot)] {
            let s = e.state();
            assert_eq!(s.ui.input.selection, food, "{label}: the food selected");
            assert_eq!(s.objects.qq_of(food) & 0xff, 2, "{label}: one eaten");
            assert_eq!(
                s.objects.place(food),
                crate::objects::IN_INVENTORY,
                "{label}: still carried"
            );
        }
        assert_eq!(
            command_outcome(&by_slot.state()),
            command_outcome(&by_panel.state()),
            "the same state"
        );
        assert_eq!(
            (
                by_panel.state().ui.panel.mode,
                by_slot.state().ui.panel.mode
            ),
            (0x92, 0x91)
        );
        let want = alone(by_slot.state().clone(), 0x92);
        assert_eq!(
            differing(by_slot.frame(), SPEC_SLOTS[2], &want.screen, PANEL),
            0,
            "the slot shows it"
        );
    }

    /// Spec 14 Design 5, the real Drop: `d` (the leader's inventory shown, 0x92, the panel
    /// cursor up), a press on a pebble in the panel area or in the backpack slot, then Return
    /// for the location (the cross-hair on the leader's cell). Both drop that pebble, at the
    /// leader's cell, and leave the same objects and inventory context.
    #[test]
    fn drop_from_the_slot_drops_what_drop_from_the_panel_drops() {
        let pre = with_commands(world());
        let cells = alone(pre.clone(), 0x92).state.inventory.cells;
        let pebble = *cells[..12]
            .iter()
            .find(|&&c| c > 0 && pre.objects.kind(c) == PEBBLE)
            .expect("a pebble on the first page");
        let at = cell_of(&pre, pebble);
        let run = |(x, y): (i16, i16)| {
            command_steps(
                pre.clone(),
                vec![
                    (
                        "Drop asks for its target",
                        vec![InputEvent::Key(Key::Char('d'))],
                        waits_for_a_target,
                    ),
                    ("Drop asks where", click(x, y), |s: &State| {
                        s.ui.input.target_pending == 1 && s.ui.input.targeting == 1
                    }),
                    ("Drop ran", vec![InputEvent::Key(Key::Enter)], at_the_prompt),
                ],
            )
        };
        let (by_panel, _) = run(at);
        let (by_slot, _) = run(in_slot(2, at));
        let (leader, ring) = (pre.objects.location[1], object(&pre, RING));
        for (label, e) in [("panel", &by_panel), ("slot", &by_slot)] {
            let s = e.state();
            // The pressed pebble is the one dropped (`drop_object` takes the selection as it
            // starts; the panel cursor's restore after the count, 0x106932(0), then selects what
            // the cursor is on, the first cell's ring, which stays carried).
            assert_eq!(s.objects.place(pebble), 0, "{label}: the pebble on the map");
            let at = s.objects.location[pebble as usize];
            assert_eq!(
                (at.x, at.y),
                (leader.x, leader.y),
                "{label}: at the leader's cell"
            );
            assert_ne!(s.objects.place(ring), 0, "{label}: the ring still carried");
            let left = (0x100..s.objects.obj_status.len() as Slot)
                .filter(|&o| o != pebble && pre.objects.place(o) != s.objects.place(o))
                .count();
            assert_eq!(left, 0, "{label}: nothing else moved");
            assert_eq!(s.ui.panel.mode, 0x92, "{label}: Drop shows the inventory");
        }
        assert_eq!(
            command_outcome(&by_slot.state()),
            command_outcome(&by_panel.state()),
            "the same state"
        );
    }

    /// Spec 14 Design 3, with the real Use: the bag opened from the backpack slot (0xa0) while the
    /// roster is shown, then `u` and a press on a food in the bag (a stack of three), in the slot:
    /// one is eaten, the bag stays open on the slot, which shows it.
    #[test]
    fn a_bag_opened_in_the_slot_stays_open_through_use() {
        let mut pre = with_commands(world());
        let bag = object(&pre, BAG);
        let food = give(&mut pre, FOOD, crate::objects::IN_CONTAINER, 3, bag);
        let bag_at = in_slot(2, cell_of(&pre, bag));
        let mut open = pre.clone();
        open.ui.panel.container = bag as u16;
        let food_at = in_slot(2, cell_of(&open, food));
        let (e, _) = command_steps(
            pre.clone(),
            vec![
                ("the bag opened", click(bag_at.0, bag_at.1), |s: &State| {
                    s.ui.panel.container != 1 && s.main.reading == 1
                }),
                (
                    "Use asks for its target",
                    vec![InputEvent::Key(Key::Char('u'))],
                    waits_for_a_target,
                ),
                ("Use ran", click(food_at.0, food_at.1), at_the_prompt),
            ],
        );
        let s = e.state().clone();
        assert_eq!(s.ui.input.selection, food, "the food selected");
        assert_eq!(s.objects.qq_of(food) & 0xff, 2, "one eaten");
        assert_eq!(s.ui.panel.container, bag as u16, "the bag still open");
        assert_eq!(s.ui.panel.mode, 0x91, "the roster still shown");
        let want = alone(s.clone(), 0x92);
        let listed: Vec<i16> = want.state.inventory.cells[..12]
            .iter()
            .copied()
            .filter(|&c| c != 0)
            .collect();
        assert_eq!(listed.len(), 3, "the bag's food and two pebbles");
        assert_eq!(listed[0], food);
        assert!(listed[1..].iter().all(|&c| s.objects.kind(c) == PEBBLE));
        assert_eq!(
            differing(e.frame(), SPEC_SLOTS[2], &want.screen, PANEL),
            0,
            "the slot shows the bag"
        );
    }

    /// The combat-mode icon gives no code: its line is drawn at once, and the slot shows it
    /// before the next key.
    #[test]
    fn a_press_with_no_code_redraws_the_slots() {
        let mut pre = world();
        pre.ui.panel.member = 1;
        let (x, y) = in_slot(2, (0xf8, 0x5c));
        let r = press(pre.clone(), &click_then_key(x, y));
        assert_eq!(r.state.actors.attrs.combat_mode[2], 3, "the mode stepped");
        let want = alone(r.state.clone(), 0x92);
        assert_eq!(differing(&r.screen, SPEC_SLOTS[2], &want.screen, PANEL), 0);
        let before = alone(pre, 0x92);
        assert_ne!(
            differing(&r.screen, SPEC_SLOTS[2], &before.screen, PANEL),
            0
        );
    }

    /// Spec 14 Design 6: while the panel shows none of the three views (the spellbook, another
    /// actor's panel) or a conversation runs, a press in a slot gives nothing and changes
    /// nothing; nor does a held button.
    #[test]
    fn a_slot_press_in_another_mode_or_held_is_ignored() {
        let (x, y) = in_slot(2, (0xf4, 0x4c));
        for (mode, pressed, talk) in [
            (0xa1, true, false),
            (0x9e, true, false),
            (0x91, true, true),
            (0x91, false, false),
        ] {
            let mut s = world();
            s.converse.active = talk;
            s.ui.panel.mode = mode;
            s.ui.panel.dirty = false;
            s.ui.pointer.pressed = pressed;
            s.ui.pointer.buttons = 1;
            s.ui.input.pointer = (x, y);
            let before = s.clone();
            let r = play(s, &[], |st, io, codes| async move {
                let code = click::click(&st, &io).await;
                codes.borrow_mut().push(code);
            });
            assert_eq!(r.codes, [0], "mode {mode:#x}");
            let a = &r.state;
            assert!(a.ui.panel == before.ui.panel, "mode {mode:#x}: the panel");
            assert!(a.ui.input == before.ui.input, "mode {mode:#x}: the input");
            assert!(a.inventory == before.inventory, "mode {mode:#x}: the cells");
            assert!(a.objects == before.objects && a.actors == before.actors);
        }
    }

    /// Spec 14 Design 4: the panel cursor moved by key in the inventory view (along its bottom
    /// row of icons) shows in its slot.
    #[test]
    fn the_panel_cursor_shows_in_the_slot() {
        let mut pre = world();
        pre.ui.panel.mode = 0x92;
        pre.ui.input.targeting = 2;
        pre.ui.input.panel_cursor = (1, 3);
        let r = play(pre, &[], |st, io, _| async move {
            panels::redraw_panel(&st, &io).await;
            cursor::panel_cursor(&st, &io, 0);
            st.borrow_mut().ui.input.panel_cursor = (3, 3);
            cursor::panel_cursor(&st, &io, 1);
        });
        assert_eq!(differing(&r.screen, SPEC_SLOTS[2], &r.screen, PANEL), 0);
        // The cursor's tile at the fourth icon (x 0xe0, y 0x58 in the panel), in the slot.
        let cursor = r.state.gfx.record_of[0x16d];
        let (sx, sy) = SPEC_SLOTS[2];
        let at = (0xe0 - PANEL.0 + sx, 0x58 - PANEL.1 + sy);
        assert_eq!(r.screen.src_at(at.0, at.1).map(|s| s.id), Some(cursor));
    }
}
