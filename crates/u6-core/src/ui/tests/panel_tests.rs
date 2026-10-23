//! The roster and stats panels on synthetic party data.

use super::*;

/// Three members: actors 1..=3, the first a hero-type figure (object type 0x19a).
fn party_state() -> State {
    let mut s = state();
    let p = &mut s.actors.party;
    p.size = 3;
    p.members[..3].copy_from_slice(&[1, 2, 3]);
    p.names = vec!["Ann".into(), "Bo".into(), "Cy".into()];
    let a = &mut s.actors.attrs;
    for (i, hp) in [(1usize, 9u8), (2, 30), (3, 41)] {
        a.hp[i] = hp;
    }
    a.strength[1] = 20;
    a.dexterity[1] = 15;
    a.intelligence[1] = 12;
    a.magic[1] = 5;
    a.level[1] = 3;
    a.exp[1] = 450;
    s.objects.type_frame[1] = 0x19a;
    s
}

fn panel(s: State) -> Engine {
    run(s, &[], 4, |st, io| async move {
        panels::redraw_panel(&st, &io).await;
        io.present();
    })
}

/// Status-window row `row` (screen row row + 1), columns 22..39.
fn status_row(e: &Engine, row: usize, fg: u8) -> String {
    row_text(e.frame(), 22..39, row + 1, fg)
}

#[test]
fn stats_panel_lays_out_attributes_and_alert_health() {
    let mut s = party_state();
    s.ui.panel.mode = 0x90;
    let e = panel(s);
    assert_eq!(status_row(&e, 0, INK).trim(), "Ann");
    assert_eq!(
        cell(e.frame(), 22 + 7, 1, INK),
        b'A',
        "centred on 17 columns"
    );
    let lines: Vec<String> = (2..=11)
        .map(|r| status_row(&e, r, INK)[10..].trim_end().to_string())
        .collect();
    assert_eq!(lines[..3], ["a20", "b15", "c12"]);
    // max magic = 2 × INT for type 0x19a, max hp = level × 30 (the seams' fallbacks).
    assert_eq!(lines[4..7], ["mm", " 5/24", "hh"]);
    assert_eq!(
        status_row(&e, 9, ALERT)[10..13].to_string(),
        "  9",
        "below 10: alert ink"
    );
    assert_eq!(lines[7].trim_start(), "/90");
    assert_eq!(lines[8..], ["l/x", "3/450"]);
    let st = e.state();
    assert!(!st.ui.panel.dirty);
    assert_eq!(st.ui.text.current, WindowId::Messages, "back to the scroll");
    assert!(st.ui.messages.is_empty(), "panel text is not scroll text");
    // The panel area is cleared to the paper colour.
    assert_eq!(e.frame().get(0xa8, 6), Some(PAPER));
    assert_eq!(e.frame().get(0xa7, 6), Some(0));
}

#[test]
fn roster_lists_members_with_hit_points_and_names() {
    let e = panel(party_state());
    for (k, (hp, name)) in [("  9", "Ann"), (" 30", "Bo"), (" 41", "Cy")]
        .into_iter()
        .enumerate()
    {
        let ink = if k == 0 { ALERT } else { INK };
        assert_eq!(status_row(&e, 2 + 2 * k, ink)[14..17].to_string(), hp);
        assert_eq!(status_row(&e, 3 + 2 * k, INK)[3..].trim_end(), name);
    }
    // No scroll arrows for three members from the top.
    assert_eq!(cell(e.frame(), 0x15, 3, INK), 0);
    assert_eq!(cell(e.frame(), 0x15, 12, INK), 0);
}

#[test]
fn roster_arrows_when_the_list_scrolls() {
    let mut s = party_state();
    s.actors.party.size = 7;
    s.actors.party.members[..7].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
    s.ui.panel.roster_first = 1;
    let e = panel(s);
    assert_eq!(cell(e.frame(), 0x15, 3, INK), 0x18);
    assert_eq!(cell(e.frame(), 0x15, 12, INK), 0x19);
}

/// Spec 07 HD0: the side panel's clear (the stats panel here), the combat-mode line's and the
/// command underline's, over a paper of the panel's paper colour but for two pixels of another
/// (in the panel's top rows and under the first icon's underline): each pixel filled names the
/// paper where the paper's colour there is the colour filled, and is classic elsewhere (the other
/// colour's pixels, the underline in the command colour).
#[test]
fn the_panel_and_underline_fills_name_the_paper_where_it_is_their_colour() {
    use u6_formats::bitmap::Bitmap;
    use u6_render::{BITMAP_PAPER, Kind, Src};
    let mut s = party_state();
    s.ui.panel.mode = 0x90;
    s.actors.attrs.combat_mode[1] = 2;
    let mut pixels = vec![PAPER; 320 * 200];
    pixels[7 * 320 + 0xa9] = ALERT;
    pixels[0xc0 * 320 + 10] = ALERT;
    let bmp = Bitmap {
        width: 320,
        height: 200,
        pixels,
    };
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    s.ui.paper = Some(text::Paper::drawn(&identity, &bmp));
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        panels::redraw_panel(&st, &io).await;
        panels::combat_mode_line(&st, &io).await;
        let mut s = st.borrow_mut();
        cursor::select_command(&mut s, &io, 0x81);
        cursor::select_command(&mut s, &io, 0x82);
        drop(s);
        io.present();
    });
    e.enable_src_plane();
    for _ in 0..4 {
        e.tick(&[], 16);
    }
    let fb = e.frame();
    let at = |x: usize, y: usize| (fb.get(x, y).unwrap(), fb.src_at(x, y).unwrap());
    let paper = |x: usize, y: usize| Src {
        ver: 0,
        id: BITMAP_PAPER,
        u: x as u16,
        v: y as u16,
        kind: Kind::Bitmap,
        flags: 0,
    };
    // The panel's clear, above its text rows.
    assert_eq!(at(0xa8, 6), (PAPER, paper(0xa8, 6)), "the panel's clear");
    assert_eq!(at(0x137, 7), (PAPER, paper(0x137, 7)));
    assert_eq!(
        at(0xa9, 7),
        (PAPER, Src::CLASSIC),
        "over the paper's other colour"
    );
    // The combat-mode line's clear, right of its name.
    assert_eq!(
        at(0x137, 0x67),
        (PAPER, paper(0x137, 0x67)),
        "the mode line's clear"
    );
    // The first icon's underline painted over in the paper colour; the second's in the command
    // colour.
    for x in 8..24 {
        let want = if x == 10 {
            Src::CLASSIC
        } else {
            paper(x, 0xc0)
        };
        assert_eq!(at(x, 0xc0), (PAPER, want), "the underline cleared at {x}");
    }
    for x in 24..40 {
        assert_eq!(at(x, 0xc0), (5, Src::CLASSIC), "the underline at {x}");
    }
    assert_eq!(
        kind_at(fb, 0, 0xc0),
        Kind::Classic,
        "no fill there: the plane's own"
    );
}

fn kind_at(fb: &Framebuffer, x: usize, y: usize) -> u6_render::Kind {
    fb.src_at(x, y).unwrap().kind
}
