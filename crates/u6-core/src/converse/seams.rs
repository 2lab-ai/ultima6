//! Calls from the converse code into modules that are not ported yet (spec 03 §3 stub points).
//!
//! Each seam is the narrowest signature the converse code needs, a `todo_stub!` with an
//! explicit fallback and a one-line contract. The dispatcher re-points them at the owning module
//! when it lands. Owners: ui (WU-P4) = 0x108658, 0x108376, 0x107cba, 0x10790a, 0x105016,
//! 0x111c30, 0x1110c8, 0x1112cc, 0x133b0e and the panel/scroll globals; objects (WU-P3) =
//! 0x10eef8, 0x10ae52, 0x10c9d2, 0x128b0e, 0x128b98, 0x128ce8, 0x10d2d8, 0x10cf7c, 0x10d16e,
//! 0x10b18c, 0x10cda8, 0x10a4d6, 0x12f6ee and the per-object tables A4+0x250c / A4-0x1184;
//! actors (WU-P5) = 0x121fca, 0x121ff8, 0x122048,
//! 0x1220f0, 0x122144, 0x1279d4, 0x10368a, 0x11ba5e, 0x1240fa, 0x1221da and A4-0x7afb / -0x7afe;
//! map (WU-P2) = the 11×11 window caches, 0x109e74; gfx (WU-P1) = 0x1533e0, 0x102ef0; magic and
//! music stay stubs in milestone 1 (0x116526, 0x11785e, 0x110878, 0x110912).

use u6_formats::position::Position;

use super::party::ix;
use super::vars::{int_var, string_var, var_letter};
use super::{Cx, Msg, OutFallback};
use crate::engine::{Io, Key};
use crate::objects::ObjectId;
use crate::state::State;
use crate::todo_stub;

/// Objlist-block (A4-0x7b84) offsets the converse code reads: A4-0x7b26 (word, int var 0x15),
/// A4-0x7b0b (solo member, -1 = party), A4-0x7b04 (gender, non-zero = female).
const TAIL_VAR15: usize = 0x5e;
const TAIL_SOLO: usize = 0x79;
const TAIL_GENDER: usize = 0x80;

// ---- text output and input (ui) --------------------------------------------------------------

/// 0x108658 printf: prints `text` (already formatted) to the message scroll.
/// Fallback: every byte through [`put_char`]'s fallback (printf sends its plain characters to
/// 0x108376 too).
pub(crate) async fn print(cx: &Cx<'_>, text: &[u8]) {
    todo_stub!("0x108658 printf to the message scroll (converse seam)", {
        for &b in text {
            char_out(cx, b).await;
        }
    })
}

/// 0x108376: one character to the message scroll. It reads the converse tables: `$x` / `#x`
/// print string / word variable `x`. Fallback: a stand-in into `ui.messages` (one entry per
/// line) for what decides the text: `$` `#` substitution, `*` waits for a key, `&`..`&` only
/// while A4-0x7b26 is set, `/` and `\\` singular / plural letters after a `#` number, `@`
/// (keyword highlight) and `^` dropped, `<` `>` `` ` `` (font / flags) dropped.
pub(crate) async fn put_char(cx: &Cx<'_>, c: u8) {
    todo_stub!(
        "0x108376 message-scroll character output (converse seam)",
        {
            char_out(cx, c).await;
        }
    )
}

async fn char_out(cx: &Cx<'_>, c: u8) {
    if cx.st(|s| out_char(s, c)) {
        cx.io.next_key().await;
    }
}

/// The synchronous part of the 0x108376 stand-in; `true` = wait for a key (`*`).
fn out_char(s: &mut State, c: u8) -> bool {
    let out = s.converse.out;
    if c != b'&' && out.gargish && word_7b26(s) == 0 {
        return false;
    }
    match c {
        b'#' | b'$' | b'%' | b'/' | b'@' | b'\\' | b'^' => {
            s.converse.out.prefix = c;
            return false;
        }
        b'&' => {
            s.converse.out.gargish = !out.gargish;
            return false;
        }
        b'*' => return true,
        b'<' | b'>' | b'`' => return false,
        _ => {}
    }
    let alpha = c.is_ascii_alphabetic();
    let text = match out.prefix {
        b'#' => {
            let v = int_var(s, var_letter(c));
            s.converse.out = OutFallback {
                prefix: 0,
                plural: v != 1,
                ..out
            };
            (v as i16).to_string().into_bytes()
        }
        b'$' => {
            s.converse.out.prefix = 0;
            string_var(s, var_letter(c))
        }
        b'/' | b'\\' => {
            if !alpha {
                s.converse.out.prefix = 0;
            }
            let keep = if out.prefix == b'/' {
                !out.plural
            } else {
                out.plural
            };
            if alpha && !keep {
                return false;
            }
            vec![c]
        }
        b'@' => {
            let ends = s
                .converse
                .text
                .as_ref()
                .map(|t| t.get(Msg::WordEnds).contains(&c));
            if ends.unwrap_or(false) {
                s.converse.out.prefix = 0;
            }
            vec![c]
        }
        b'^' => {
            s.converse.out.prefix = 0;
            vec![c]
        }
        _ => vec![c],
    };
    for b in text {
        push_char(s, b);
    }
    false
}

fn push_char(s: &mut State, b: u8) {
    let lines = &mut s.ui.messages;
    if lines.is_empty() {
        lines.push(String::new());
    }
    if b == b'\n' {
        lines.push(String::new());
    } else if let Some(last) = lines.last_mut() {
        last.push(char::from(b));
    }
}

/// 0x107cba(buf, max): line input; returns the typed bytes (no NUL). Fallback: Enter ends,
/// Escape clears and ends, Backspace deletes, printable 0x20..=0x7e is taken while shorter than
/// `max`; taken characters are echoed.
pub(crate) async fn read_line(cx: &Cx<'_>, max: usize) -> Vec<u8> {
    todo_stub!("0x107cba line input (converse seam)", {
        let mut line = Vec::new();
        loop {
            match cx.io.next_key().await {
                Key::Enter => break,
                Key::Escape => {
                    line.clear();
                    break;
                }
                Key::Backspace => {
                    line.pop();
                }
                k => {
                    let c = key_byte(k);
                    if line.len() < max && (0x20..=0x7e).contains(&c) {
                        line.push(c);
                        cx.st(|s| push_char(s, c));
                    }
                }
            }
        }
        line
    })
}

/// 0x10790a: one key, as the game's key code (letters upper case, CR 0x0d, Esc 0x1b).
/// Fallback: that mapping of the next queued key.
pub(crate) async fn read_key(cx: &Cx<'_>) -> u16 {
    todo_stub!("0x10790a read a key (converse seam)", {
        u16::from(key_byte(cx.io.next_key().await).to_ascii_uppercase())
    })
}

/// Op 0xcb: A4-0x7a1c = 1, 0x10790a, A4-0x7a1c = 5. Fallback: waits for one key.
pub(crate) async fn wait_key(cx: &Cx<'_>) {
    todo_stub!(
        "0x10790a wait for a key, A4-0x7a1c mode 1 (converse seam)",
        {
            cx.io.next_key().await;
        }
    )
}

/// Op 0xc0: A4-0x7a32 = 1, A4-0x7a30 = 0x92 (inventory pick), 0x10790a, then the picked
/// (A4-0x2f04, A4-0x2f02). Fallback: waits for one key, nothing picked (-1, -1).
pub(crate) async fn select_item(cx: &Cx<'_>) -> (i16, i16) {
    todo_stub!("0x10790a inventory item selection (converse seam)", {
        cx.io.next_key().await;
        (-1, -1)
    })
}

fn key_byte(k: Key) -> u8 {
    match k {
        Key::Enter => 0x0d,
        Key::Escape => 0x1b,
        Key::Space => b' ',
        Key::Backspace => 0x08,
        Key::Char(c) if c.is_ascii() => c as u8,
        _ => 0,
    }
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

/// 0x1125b8: (A4-0x7a10)->flags |= 0x10 before the conversation. Fallback: nothing.
pub(crate) fn mark_scroll_talking(_s: &mut State) {
    todo_stub!("0x1125b8 A4-0x7a10 scroll flag 0x10 (converse seam)", ())
}

/// 0x1125b8: A4-0x7a52 = 1 after the conversation. Fallback: nothing.
pub(crate) fn mark_7a52(_s: &mut State) {
    todo_stub!("0x1125b8 A4-0x7a52 = 1 (converse seam)", ())
}

/// (A4-0x7a30 panel mode, A4-0x7a3e panel member). Fallback: (0, 0).
pub(crate) fn panel(_s: &State) -> (u16, u16) {
    todo_stub!(
        "0x114afa read panel mode/member A4-0x7a30/-0x7a3e (converse seam)",
        (0, 0)
    )
}

/// Restores A4-0x7a30 / A4-0x7a3e. Fallback: nothing.
pub(crate) fn set_panel(_s: &mut State, _mode: u16, _member: u16) {
    todo_stub!("0x114afa write panel mode/member (converse seam)", ())
}

/// After talking with a non-member's inventory shown: A4-0x7a3e = 0, A4-0x722c = 0,
/// A4-0x7a30 = 0x91. Fallback: nothing.
pub(crate) fn reset_panel(_s: &mut State) {
    todo_stub!("0x114afa reset panel after talk (converse seam)", ())
}

/// Op 0xbe: A4+0x1380 = `other`, A4+0x1382 = 0, A4-0x7a3e = `member` when `Some`, then the
/// inventory panel 0x111c30(npc). Fallback: nothing.
pub(crate) async fn inventory_panel(_cx: &Cx<'_>, _npc: i16, _member: Option<u16>, _other: i16) {
    todo_stub!("0x111c30 inventory panel (converse seam)", ())
}

/// 0x128ce8: portrait panel (portrait, name, worn objects; waits for a key outside talk).
/// Fallback: the portrait only, through 0x133d3e at (0xd8, 0x18) as 0x128ce8 places it.
pub(crate) async fn portrait_panel(cx: &Cx<'_>, npc: i16) {
    todo_stub!("0x128ce8 portrait panel (converse seam)", {
        let state = cx.state.borrow();
        super::script::draw_portrait(&state, cx.io, npc, 0xd8, 0x18);
    })
}

/// 0x1533e0: draw a 56×64 bitmap at (x, y). Fallback: nothing.
pub(crate) fn blit_portrait(_io: &Io, _pixels: &[u8], _x: i16, _y: i16) {
    todo_stub!("0x1533e0 bitmap blit (converse seam)", ())
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

/// 0x133b0e: redraw the side panels. Fallback: nothing.
pub(crate) fn refresh_panels(_s: &mut State) {
    todo_stub!("0x133b0e refresh panels (converse seam)", ())
}

// ---- objects ----------------------------------------------------------------------------------

/// 0x10eef8(obj): the display name. Fallback: the look-table text of the type's base tile.
pub(crate) fn object_name(s: &State, obj: i16) -> Vec<u8> {
    todo_stub!("0x10eef8 object display name (converse seam)", {
        let ty = u16::try_from(obj).map_or(0, |o| s.objects.obj_type(ObjectId(o)));
        let tile = s
            .tables
            .basetile
            .base
            .get(usize::from(ty))
            .copied()
            .unwrap_or(0);
        s.tables
            .look
            .description(tile, false)
            .map(|t| t.chars().map(|c| c as u32 as u8).collect())
            .unwrap_or_default()
    })
}

/// 0x10ae52(actor): next object the actor carries along (≥ 0), or -1. Fallback: -1.
pub(crate) fn carried_object(_s: &State, _actor: i16) -> i16 {
    todo_stub!("0x10ae52 object held by an actor (converse seam)", -1)
}

/// 0x10c9d2(obj, x, y, z): move an object to a map location. Fallback: rewrites the location.
pub(crate) fn move_object(s: &mut State, obj: i16, x: u16, y: u16, z: u8) {
    todo_stub!("0x10c9d2 move an object (converse seam)", {
        if let Some(p) = ix(obj).and_then(|i| s.objects.location.get_mut(i)) {
            *p = Position { x, y, z };
        }
    })
}

/// 0x128b0e(obj) after a drop. Fallback: nothing.
pub(crate) fn after_drop(_s: &mut State, _obj: i16) {
    todo_stub!("0x128b0e after dropping an object (converse seam)", ())
}

/// The word at A4-0x1184 + 2 × `obj` (op 0xd6 walks it from an actor to a body object 0x153).
/// Fallback: -1 (end of the chain).
pub(crate) fn object_link(_s: &State, _obj: i16) -> i16 {
    todo_stub!("0x11404c object link table A4-0x1184 (converse seam)", -1)
}

/// The word at A4+0x250c + 2 × `obj` (its high byte names the NPC a shrine or statue talks
/// as). Actor slots: objlist 0x0600; other objects: the object tables. Fallback: the actor
/// word, 0 for other objects.
pub(crate) fn object_word_250c(s: &State, obj: i16) -> u16 {
    todo_stub!("0x114afa object word table A4+0x250c (converse seam)", {
        ix(obj)
            .and_then(|i| s.actors.attrs.unknown_0600.get(i))
            .copied()
            .unwrap_or(0)
    })
}

/// 0x10d2d8(actor, type): how many objects of `type` the actor has. Fallback: 0.
pub(crate) fn object_count(_s: &State, _actor: i16, _ty: i16) -> i16 {
    todo_stub!("0x10d2d8 count carried objects (converse seam)", 0)
}

/// Op 0xb9 → 0x10cf7c(actor, type, quality << 8 | quantity): give a new object. Fallback:
/// nothing.
pub(crate) fn add_object(_s: &mut State, _actor: i16, _ty: i16, _packed: u16) {
    todo_stub!("0x10cf7c give a new object (converse seam)", ())
}

/// Op 0xba → 0x10d16e(actor, type, quality << 8 | quantity): take objects away. Fallback:
/// nothing.
pub(crate) fn remove_object(_s: &mut State, _actor: i16, _ty: i16, _packed: u16) {
    todo_stub!("0x10d16e remove objects (converse seam)", ())
}

/// 0x10b18c(actor, type, quality): the actor's object of that kind, or -1. Fallback: -1.
pub(crate) fn find_object(_s: &State, _actor: i16, _ty: i16, _quality: i16) -> i16 {
    todo_stub!("0x10b18c find a carried object (converse seam)", -1)
}

/// 0x10cda8(obj, container, flags): move an object into an actor/container. Fallback: nothing.
pub(crate) fn move_into(_s: &mut State, _obj: i16, _dest: i16, _flags: i16) {
    todo_stub!("0x10cda8 move into a container (converse seam)", ())
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

fn stat(s: &State, a: i16, base: u8) -> u16 {
    let flags = ix(a)
        .and_then(|i| s.objects.actor_flags.get(i))
        .copied()
        .unwrap_or(0);
    let mut v = i16::from(base);
    if flags & 0x40 != 0 {
        v -= 3;
        if v < 1 {
            v = 1;
        }
    }
    v as u16
}

fn attr(v: &[u8], a: i16) -> u8 {
    ix(a).and_then(|i| v.get(i)).copied().unwrap_or(0)
}

/// 0x121ff8(actor): effective dexterity (int var 10). Fallback: dexterity, −3 (min 1) with
/// actor flag 0x40, 1 when status bit 2 is set (the A4-0x7b64 test taken as clear).
pub(crate) fn dexterity(s: &State, a: i16) -> u16 {
    todo_stub!("0x121ff8 effective dexterity (converse seam)", {
        let v = stat(s, a, attr(&s.actors.attrs.dexterity, a));
        if super::party::status(s, a) & 4 != 0 {
            1
        } else {
            v
        }
    })
}

/// 0x122048(actor): effective intelligence (int var 18). Fallback: −3 (min 1) with flag 0x40.
pub(crate) fn intelligence(s: &State, a: i16) -> u16 {
    todo_stub!("0x122048 effective intelligence (converse seam)", {
        stat(s, a, attr(&s.actors.attrs.intelligence, a))
    })
}

/// 0x121fca(actor): effective strength (int var 28). Fallback: −3 (min 1) with flag 0x40.
pub(crate) fn strength(s: &State, a: i16) -> u16 {
    todo_stub!("0x121fca effective strength (converse seam)", {
        stat(s, a, attr(&s.actors.attrs.strength, a))
    })
}

/// 0x122144(actor): maximum hit points. Fallback: level × 30 in 1..=255.
pub(crate) fn max_hp(s: &State, a: i16) -> i16 {
    todo_stub!("0x122144 maximum hit points (converse seam)", {
        (u16::from(attr(&s.actors.attrs.level, a)) * 30).clamp(1, 0xff) as i16
    })
}

/// 0x1220f0(actor): the level byte. Fallback: that byte.
pub(crate) fn level(s: &State, a: i16) -> i16 {
    todo_stub!("0x1220f0 actor level (converse seam)", {
        i16::from(attr(&s.actors.attrs.level, a))
    })
}

/// 0x1221da(actor) after worktype 0x91. Fallback: nothing.
pub(crate) fn worktype_91(_s: &mut State, _actor: i16) {
    todo_stub!("0x1221da worktype 0x91 follow-up (converse seam)", ())
}

/// 0x1279d4(member index, 0): heal a party member. Fallback: nothing.
pub(crate) async fn heal_member(_cx: &Cx<'_>, _index: i16) {
    todo_stub!("0x1279d4 heal a party member (converse seam)", ())
}

/// 0x10368a(minutes): advance the clock. Fallback: adds the minutes with carry into the hour
/// (mod 24) so that op 0x9e's wait for 5 o'clock ends.
pub(crate) async fn clock_tick(cx: &Cx<'_>, minutes: u8) {
    todo_stub!("0x10368a advance the clock (converse seam)", {
        cx.st(|s| {
            let c = &mut s.actors.calendar;
            let m = u16::from(c.minute) + u16::from(minutes);
            c.minute = (m % 60) as u8;
            c.hour = ((u16::from(c.hour) + m / 60) % 24) as u8;
        })
    })
}

/// Op 0x9e around the clock loop: A4-0x6a9e = 1 / 0, then A4-0x6578 = 1 and 0x109e74.
/// Fallback: nothing.
pub(crate) fn resting(_s: &mut State, _on: bool) {
    todo_stub!("0x109e74 rest flags and view flags (converse seam)", ())
}

/// 0x102ef0: refresh the view (one presented frame). Fallback: one VBlank.
pub(crate) async fn refresh_view(cx: &Cx<'_>) {
    todo_stub!(
        "0x102ef0 refresh the view (converse seam)",
        cx.io.vblank().await
    )
}

/// A4-0x7afb: non-zero refuses conversations ("No response"). Fallback: clear.
pub(crate) fn talk_refused(_s: &State) -> bool {
    todo_stub!(
        "0x114afa A4-0x7afb talk refusal flag (converse seam)",
        false
    )
}

/// A4-0x7afe += 1 after a conversation (byte; read by the clock, panels and main loop).
/// Fallback: nothing.
pub(crate) fn bump_7afe(_s: &mut State) {
    todo_stub!("0x114afa A4-0x7afe += 1 (converse seam)", ())
}

/// The map window caches 0x112bf2 reads at (col, row) of the 11×11 window: the byte table at
/// A4-0x15f0 and the word table at A4-0x1476. Fallback: (0, 0), a visible cell.
pub(crate) fn view_cell(_s: &State, _col: i16, _row: i16) -> (u8, i16) {
    todo_stub!(
        "0x112bf2 view window cell caches A4-0x15f0/-0x1476 (converse seam)",
        (0, 0)
    )
}

// ---- magic and music (milestone-1 stubs) ---------------------------------------------------

/// 0x116526(actor, type, quality): object id or -1. Fallback: -1 (not carried).
pub(crate) fn has_object(_s: &State, _actor: i16, _ty: i16, _quality: i16) -> i16 {
    todo_stub!("0x116526 actor carries an object (converse seam)", -1)
}

/// 0x11785e(obj): resurrect the body object. Fallback: nothing.
pub(crate) fn resurrect(_s: &mut State, _obj: i16) {
    todo_stub!("0x11785e resurrect (converse seam)", ())
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
