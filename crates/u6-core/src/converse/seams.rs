//! Calls from the converse code into other modules (spec 03 §3).
//!
//! Each seam is the narrowest signature the converse code needs; since the integration (WU-P7)
//! each forwards to its owner, and the targets outside milestone 1 keep a `todo_stub!` with an
//! explicit fallback and a one-line contract. Owners: ui (WU-P4) = 0x108658, 0x108376, 0x107cba, 0x10790a, 0x105016,
//! 0x111c30, 0x1110c8, 0x1112cc, 0x133b0e and the panel/scroll globals; objects (WU-P3) =
//! 0x10eef8, 0x10ae52, 0x10c9d2, 0x128b0e, 0x128b98, 0x128ce8, 0x10d2d8, 0x10cf7c, 0x10d16e,
//! 0x10b18c, 0x10cda8, 0x10a4d6, 0x12f6ee and the per-object tables A4+0x250c / A4-0x1184;
//! actors (WU-P5) = 0x121fca, 0x121ff8, 0x122048,
//! 0x1220f0, 0x122144, 0x1279d4, 0x10368a, 0x11ba5e, 0x1240fa, 0x1221da and A4-0x7afb / -0x7afe;
//! map (WU-P2) = the 11×11 window caches, 0x109e74; gfx (WU-P1) = 0x1533e0, 0x102ef0; magic and
//! music stay stubs in milestone 1 (0x116526, 0x11785e, 0x110878, 0x110912).

use super::Cx;
use super::party::ix;
use crate::engine::Io;
use crate::state::State;
use crate::todo_stub;

/// Objlist-block (A4-0x7b84) offsets the converse code reads: A4-0x7b26 (word, int var 0x15),
/// A4-0x7b0b (solo member, -1 = party), A4-0x7b04 (gender, non-zero = female).
const TAIL_VAR15: usize = 0x5e;
const TAIL_SOLO: usize = 0x79;
const TAIL_GENDER: usize = 0x80;

// ---- text output and input (ui) --------------------------------------------------------------

/// 0x108658 printf: prints `text` to the message scroll (the converse passes it as the format).
pub(crate) async fn print(cx: &Cx<'_>, text: &[u8]) {
    Box::pin(crate::ui::printf::printf(cx.state, cx.io, text, &[])).await;
}

/// 0x108376: one character to the message scroll through the escape layer (`$x` / `#x` read the
/// converse variables, `&` text shows only while A4-0x7b26 is set).
pub(crate) async fn put_char(cx: &Cx<'_>, c: u8) {
    Box::pin(crate::ui::printf::text_out(cx.state, cx.io, i16::from(c))).await;
}

/// 0x107cba(buf, max): line input with echo; returns the typed bytes (no NUL), empty after Esc.
pub(crate) async fn read_line(cx: &Cx<'_>, max: usize) -> Vec<u8> {
    let (n, line) = Box::pin(crate::ui::keyread::read_line(cx.state, cx.io, max as i16)).await;
    if n < 0 { Vec::new() } else { line }
}

/// 0x10790a: one key, as the game's key code (letters upper case, CR 0x0d, Esc 0x1b).
pub(crate) async fn read_key(cx: &Cx<'_>) -> u16 {
    Box::pin(crate::ui::wait::read_key(cx.state, cx.io)).await
}

/// Op 0xcb: A4-0x7a1c = 1 (the waiting cursor), 0x10790a, A4-0x7a1c = 5.
pub(crate) async fn wait_key(cx: &Cx<'_>) {
    cx.st(|s| s.ui.input.cursor_glyph = 1);
    Box::pin(crate::ui::wait::read_key(cx.state, cx.io)).await;
    cx.st(|s| s.ui.input.cursor_glyph = 5);
}

/// Op 0xc0: A4-0x7a32 = 1, A4-0x7a30 = 0x92 (inventory pick), 0x10790a, then the picked
/// (A4-0x2f04, A4-0x2f02).
pub(crate) async fn select_item(cx: &Cx<'_>) -> (i16, i16) {
    cx.st(|s| {
        s.ui.input.target_pending = 1;
        s.ui.panel.mode = 0x92;
    });
    Box::pin(crate::ui::wait::read_key(cx.state, cx.io)).await;
    cx.st(|s| (s.ui.input.selection, s.ui.input.target.0))
}

/// 0x128b98(buf, 0): decimal number in `text`; -1 empty, -2 escaped / not a number.
/// Fallback: mode 0 as the original (digits accumulate ×10 in a word; any other byte → -2;
/// empty → -1, the escape flag A4-0x7a44 taken as clear).
pub(crate) fn parse_number(text: &[u8]) -> i16 {
    todo_stub!("0x128b98 parse a number (converse seam)", {
        if text.is_empty() {
            return -1;
        }
        let mut v: i16 = 0;
        for &b in text {
            if !b.is_ascii_digit() {
                return -2;
            }
            v = v.wrapping_mul(10).wrapping_add(i16::from(b - b'0'));
        }
        v
    })
}

// ---- panels and scroll globals (ui) -----------------------------------------------------------

/// 0x1125b8: (A4-0x7a10)->flags |= 0x10 (the current window pages) before the conversation.
pub(crate) fn mark_scroll_talking(s: &mut State) {
    s.ui.text.win_mut().flags |= crate::ui::text::PAGED;
}

/// 0x1125b8: A4-0x7a52 = 1 after the conversation (the next poll clears the paged flag).
pub(crate) fn mark_7a52(s: &mut State) {
    s.ui.input.unpage = true;
}

/// (A4-0x7a30 panel mode, A4-0x7a3e panel member).
pub(crate) fn panel(s: &State) -> (u16, u16) {
    (s.ui.panel.mode, s.ui.panel.member as u16)
}

/// Restores A4-0x7a30 / A4-0x7a3e.
pub(crate) fn set_panel(s: &mut State, mode: u16, member: u16) {
    s.ui.panel.mode = mode;
    s.ui.panel.member = member as i16;
}

/// After talking with a non-member's inventory shown: A4-0x7a3e = 0, A4-0x722c = 0,
/// A4-0x7a30 = 0x91.
pub(crate) fn reset_panel(s: &mut State) {
    let p = &mut s.ui.panel;
    p.member = 0;
    p.roster_first = 0;
    p.mode = 0x91;
}

/// Op 0xbe: A4+0x1380 = `other`, A4+0x1382 = 0, A4-0x7a3e = `member` when `Some`, then the
/// inventory panel 0x111c30.
pub(crate) async fn inventory_panel(cx: &Cx<'_>, _npc: i16, member: Option<u16>, other: i16) {
    cx.st(|s| {
        let p = &mut s.ui.panel;
        p.open_container = other as u16;
        p.container_flag = false;
        if let Some(m) = member {
            p.member = m as i16;
        }
    });
    Box::pin(crate::ui::panels::inventory(cx.state, cx.io)).await;
}

/// 0x128ce8: portrait panel (portrait, name, worn objects; waits for a key outside talk).
/// Fallback: its first steps for a person with nothing readied: the panel cleared to the paper
/// colour (0x152176 + 0x1521f4 over (0xa8, 6)..(0x137, 0x67)), the sky bar (0x133b28), the
/// portrait through 0x133d3e at (0xd8, 0x18); no name line and no equipment.
pub(crate) async fn portrait_panel(cx: &Cx<'_>, npc: i16) {
    todo_stub!("0x128ce8 portrait panel (converse seam)", {
        let mut state = cx.state.borrow_mut();
        let paper = state.ui.tables.colours.paper;
        cx.io
            .draw(|fb| crate::ui::text::fill_rect(fb, 0xa8, 6, 0x137, 0x67, paper));
        crate::ui::panels::sky_bar(&mut state, cx.io);
        super::script::draw_portrait(&state, cx.io, npc, 0xd8, 0x18);
        state.ui.panel.dirty = true;
    })
}

/// 0x1533e0: draw the 56×64 portrait bitmap at (x, y) on the screen through the bitmap colour map
/// A4-0x3952 (gfx [`crate::gfx::blit::blit_bitmap`]).
pub(crate) fn blit_portrait(state: &State, io: &Io, pixels: &[u8], x: i16, y: i16) {
    let colours = &state.gfx.tables.bitmap_colours;
    io.draw(|fb| crate::gfx::blit::blit_bitmap(colours, fb, (56, 64), pixels, x as u16, y as u16));
}

/// 0x1110c8: object type in the 9-entry list at A4-0x71ca (counted, not weighed ×10).
/// Fallback: false.
pub(crate) fn counted_type(_s: &State, _ty: i16) -> bool {
    todo_stub!("0x1110c8 counted object type (converse seam)", false)
}

/// 0x1112cc(actor): the two carried-weight words it leaves at A4+0x1342 / A4+0x1344.
/// Fallback: (0, 0).
pub(crate) fn carried_weights(_s: &mut State, _actor: i16) -> (i16, i16) {
    todo_stub!("0x1112cc carried weight (converse seam)", (0, 0))
}

/// 0x133b0e: full redraw of the panels and the view (as the actors call it).
pub(crate) fn refresh_panels(s: &mut State) {
    crate::actors::seams::full_redraw(s);
}

// ---- objects ----------------------------------------------------------------------------------

/// 0x10eef8(obj): the display name.
pub(crate) fn object_name(s: &State, obj: i16) -> Vec<u8> {
    crate::objects::display_name(s, obj).into_bytes()
}

/// 0x10ae52(actor): the first object directly inside or carried by the actor, or -1.
pub(crate) fn carried_object(s: &mut State, actor: i16) -> i16 {
    crate::objects::first_child(&mut s.objects, actor)
}

/// 0x10c9d2(obj, x, y, z): move an object to a map location.
pub(crate) fn move_object(s: &mut State, obj: i16, x: u16, y: u16, z: u8) {
    crate::objects::move_to(s, obj, x, y, u16::from(z));
}

/// 0x128b0e(obj) after a drop: the object may be taken again (status 0x01).
pub(crate) fn after_drop(s: &mut State, obj: i16) {
    if ix(obj).is_some() {
        crate::objects::mark_takeable(&mut s.objects, obj);
    }
}

/// The word at A4-0x1184 + 2 × `obj` (op 0xd6 walks it from an actor to a body object 0x153):
/// the object list link `objects.next`, -1 at the end.
pub(crate) fn object_link(s: &State, obj: i16) -> i16 {
    s.objects.next_of(obj)
}

/// The word at A4+0x250c + 2 × `obj` (its high byte names the NPC a shrine or statue talks
/// as): `objects.qq`, 0 for a slot past the table.
pub(crate) fn object_word_250c(s: &State, obj: i16) -> u16 {
    ix(obj).map_or(0, |i| s.objects.qq_of(i as i16))
}

/// 0x10d2d8(actor, type): how many objects of `type` the actor has.
pub(crate) fn object_count(s: &mut State, actor: i16, ty: i16) -> i16 {
    crate::objects::count_of_kind(&mut s.objects, actor, ty as u16)
}

/// Op 0xb9 → 0x10cf7c(actor, type, quality << 8 | quantity): give a new object.
pub(crate) fn add_object(s: &mut State, actor: i16, ty: i16, packed: u16) {
    crate::objects::add_to_owner(s, actor, ty as u16, packed);
}

/// Op 0xba → 0x10d16e(actor, type, quality << 8 | quantity): take objects away.
pub(crate) fn remove_object(s: &mut State, actor: i16, ty: i16, packed: u16) {
    crate::objects::take_from_owner(s, actor, ty as u16, packed);
}

/// 0x10b18c(actor, type, quality): the actor's object of that kind, or -1.
pub(crate) fn find_object(s: &mut State, actor: i16, ty: i16, quality: i16) -> i16 {
    crate::objects::find_child(&mut s.objects, actor, ty, quality)
}

/// 0x10cda8(obj, container, flags): move an object into an actor/container.
pub(crate) fn move_into(s: &mut State, obj: i16, dest: i16, flags: i16) {
    crate::objects::move_into(s, obj, dest, flags as u8);
}

/// Op 0x9c: 0x10a4d6(0x1af, movement flags & 7) then 0x12f6ee(actor, centre x, centre y, that
/// object, 0). Fallback: nothing.
pub(crate) fn mount(_s: &mut State, _actor: i16, _flags: u8) {
    todo_stub!("0x12f6ee put an actor on a horse (converse seam)", ())
}

// ---- actors, clock, combat leaves ---------------------------------------------------------------

/// Party leader / target cursor step of 0x114afa: 0x105016(A4-0x2f0a, A4-0x2f08), when ≥ 0
/// 0x11ba5e(leader, it) and 0x109e74. Fallback: nothing.
pub(crate) fn face_target(_s: &mut State) {
    todo_stub!("0x105016 face the talk target (converse seam)", ())
}

/// 0x1240fa(obj): the actor riding horse object `obj`. Fallback: `obj`.
pub(crate) fn rider(_s: &State, obj: i16) -> i16 {
    todo_stub!("0x1240fa rider of a horse (converse seam)", obj)
}

/// 0x121ff8(actor): effective dexterity (int var 10).
pub(crate) fn dexterity(s: &State, a: i16) -> u16 {
    crate::actors::leaves::dexterity(s, a as u16)
}

/// 0x122048(actor): effective intelligence (int var 18).
pub(crate) fn intelligence(s: &State, a: i16) -> u16 {
    crate::actors::leaves::intelligence(s, a as u16)
}

/// 0x121fca(actor): effective strength (int var 28).
pub(crate) fn strength(s: &State, a: i16) -> u16 {
    crate::actors::leaves::strength(s, a as u16)
}

/// 0x122144(actor): maximum hit points.
pub(crate) fn max_hp(s: &State, a: i16) -> i16 {
    crate::actors::leaves::max_hp(s, a as u16) as i16
}

/// 0x1220f0(actor): the level byte.
pub(crate) fn level(s: &State, a: i16) -> i16 {
    i16::from(crate::actors::leaves::level(s, a as u16))
}

/// 0x1221da(actor) after worktype 0x91. Fallback: nothing.
pub(crate) fn worktype_91(_s: &mut State, _actor: i16) {
    todo_stub!("0x1221da worktype 0x91 follow-up (converse seam)", ())
}

/// 0x1279d4(member index, 0): heal a party member (no announcement); its lines print at once.
pub(crate) async fn heal_member(cx: &Cx<'_>, index: i16) {
    cx.st(|s| crate::actors::party::heal(s, index as usize, false));
    Box::pin(crate::main_loop::flush_prints(cx.state, cx.io)).await;
}

/// 0x10368a(minutes): advance the clock; its lines print at once.
pub(crate) async fn clock_tick(cx: &Cx<'_>, minutes: u8) {
    cx.st(|s| crate::actors::clock::tick(s, i16::from(minutes)));
    Box::pin(crate::main_loop::flush_prints(cx.state, cx.io)).await;
}

/// Op 0x9e around the clock loop: A4-0x6a9e = 1 / 0, then A4-0x6578 = 1 and 0x109e74.
pub(crate) fn resting(s: &mut State, on: bool) {
    s.actors.work.force_teleport = u16::from(on);
    if !on {
        s.gfx.dissolve = 1;
        crate::map::view::update_view(s);
    }
}

/// 0x102ef0: refresh the view (one presented frame), then the next display generation.
pub(crate) async fn refresh_view(cx: &Cx<'_>) {
    cx.st(|s| crate::gfx::refresh_view(s, cx.io));
    cx.io.vblank().await;
}

/// A4-0x7afb: non-zero refuses conversations ("No response"; `map.flag_7afb`).
pub(crate) fn talk_refused(s: &State) -> bool {
    s.map.flag_7afb != 0
}

/// A4-0x7afe += 1 after a conversation (the panel is redrawn: `ui.panel.dirty`).
pub(crate) fn bump_7afe(s: &mut State) {
    s.ui.panel.dirty = true;
}

/// The map window caches 0x112bf2 reads at (col, row) of the 11×11 window: the byte table at
/// A4-0x15f0 (first draw-list node) and the word table at A4-0x1476 (tile shown), built by
/// 0x109e74 / 0x10e81e (`objects.draw`).
pub(crate) fn view_cell(s: &State, col: i16, row: i16) -> (u8, i16) {
    let d = &s.objects.draw;
    usize::try_from(row * 11 + col)
        .ok()
        .filter(|&i| i < d.cell.len())
        .map_or((0, 0xff), |i| (d.cell[i], d.base[i] as i16))
}

// ---- magic and music (milestone-1 stubs) ---------------------------------------------------

/// 0x116526(actor, type, quality): object id or -1 (magic).
pub(crate) fn has_object(s: &State, actor: i16, ty: i16, quality: i16) -> i16 {
    crate::magic::carried_object(&s.objects, actor, ty, quality)
}

/// 0x110878(song): play / 0 = stop. Fallback: nothing.
pub(crate) fn play_song(_s: &mut State, _song: u16) {
    todo_stub!("0x110878 play song (converse seam)", ())
}

/// 0x110912: pick the area's song. Fallback: nothing.
pub(crate) fn pick_song(_s: &mut State) {
    todo_stub!("0x110912 pick the area song (converse seam)", ())
}

// ---- objlist-block bytes (actors keep the block; the converse reads these directly) ---------

/// A4-0x7b0b: solo member index, -1 in party mode.
pub(crate) fn solo_member(s: &State) -> i8 {
    s.actors
        .objlist_tail
        .get(TAIL_SOLO)
        .map_or(-1, |&b| b as i8)
}

/// A4-0x7b04: the avatar is female.
pub(crate) fn female(s: &State) -> bool {
    s.actors
        .objlist_tail
        .get(TAIL_GENDER)
        .is_some_and(|&b| b != 0)
}

/// A4-0x7b26 (big-endian word, int var 0x15).
pub(crate) fn word_7b26(s: &State) -> u16 {
    let t = &s.actors.objlist_tail;
    match (t.get(TAIL_VAR15), t.get(TAIL_VAR15 + 1)) {
        (Some(&h), Some(&l)) => u16::from_be_bytes([h, l]),
        _ => 0,
    }
}

/// Writes A4-0x7b26 back.
pub(crate) fn set_word_7b26(s: &mut State, v: u16) {
    if let Some(w) = s.actors.objlist_tail.get_mut(TAIL_VAR15..TAIL_VAR15 + 2) {
        w.copy_from_slice(&v.to_be_bytes());
    }
}
