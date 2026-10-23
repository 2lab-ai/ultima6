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
/// in it and fourteen pebbles (sixteen list entries); every tile record a whole opaque square of its own colour; click
/// regions of made-up rects but the side panel's, which is the panel area; the roster shown.
fn world() -> State {
    use crate::objects::{IN_CONTAINER, IN_INVENTORY, READIED, create_in};
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
    let give = |s: &mut State, ty: u16, status: u8, owner: i16| {
        let o = create_in(s, ty, status, 0, owner);
        assert!(o > 0, "a slot for {ty:#x}");
        o
    };
    // Each object is linked right after its owner: made last, listed first.
    give(&mut s, TWO_HANDED, READIED, 1);
    for _ in 0..14 {
        give(&mut s, PEBBLE, IN_INVENTORY, 1);
    }
    let bag = give(&mut s, BAG, IN_INVENTORY, 1);
    give(&mut s, PEBBLE, IN_CONTAINER, bag);
    give(&mut s, PEBBLE, IN_CONTAINER, bag);
    give(&mut s, RING, IN_INVENTORY, 1);
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
    s
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
    use super::{BAG, PEBBLE, RING, TWO_HANDED, world};
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

    /// Open the bag from the backpack slot (0xa0) while the roster is shown, then what a Use
    /// leaves: the panel dirty (its refresh, `usecmd/seams.rs` `redraw_inventory`, and the
    /// command loop's, Design 4) and redrawn. The bag stays open in the slot.
    #[test]
    fn a_bag_opened_in_the_slot_stays_open_through_use() {
        let pre = world();
        let bag = object(&pre, BAG);
        let (x, y) = in_slot(2, cell_of(&pre, bag));
        let r = play(pre.clone(), &click_then_key(x, y), |st, io, _| async move {
            panels::redraw_panel(&st, &io).await;
            crate::ui::wait::read_key(&st, &io).await;
            for _ in 0..2 {
                st.borrow_mut().ui.panel.dirty = true;
                panels::redraw_panel(&st, &io).await;
            }
        });
        let s = &r.state;
        assert_eq!(s.ui.panel.container, bag as u16, "the bag open");
        assert_eq!(s.ui.panel.mode, 0x91);
        let mut open = pre.clone();
        open.ui.panel.container = bag as u16;
        let want = alone(open, 0x92);
        assert_eq!(differing(&r.screen, SPEC_SLOTS[2], &want.screen, PANEL), 0);
        assert!(
            want.state.inventory.cells[..12]
                .iter()
                .filter(|&&c| c != 0)
                .all(|&c| s.objects.kind(c) == PEBBLE),
            "the bag's list"
        );
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
        let cases: [(&str, State, usize, (i16, i16)); 11] = [
            ("list cell", pre.clone(), 2, cell_of(&pre, ring)),
            ("equipment cell", pre.clone(), 2, (0xb8, 0x30)),
            ("down arrow", pre.clone(), 2, (0xf4, 0x4c)),
            ("container figure", pre.clone(), 2, (0x118, 0x18)),
            ("previous member", second.clone(), 2, (0xb8, 0x5c)),
            ("roster icon", pre.clone(), 2, (0xc8, 0x5c)),
            ("stats icon", pre.clone(), 2, (0xd8, 0x5c)),
            ("next member", pre.clone(), 2, (0xe8, 0x5c)),
            ("combat mode", second.clone(), 2, (0xf8, 0x5c)),
            ("member row", pre.clone(), 0, (0xd0, 0x1c + 16 + 4)),
            ("next member from stats", pre.clone(), 1, (0xe8, 0x5c)),
        ];
        for (label, s, slot, at) in cases {
            let view = SPEC_VIEWS[slot];
            let shown = if view == 0x91 { 0x92 } else { 0x91 };
            let mut panel = s.clone();
            panel.ui.panel.mode = view;
            let by_panel = press(panel, &click_then_key(at.0, at.1));
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
