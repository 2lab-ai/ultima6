//! The rest of the Use handlers: the vortex cube 0x1303f4, wands 0x12d204, instruments
//! 0x12c8b8, the machine 0x12cfe4, the wishing well 0x12f0d2, shrines 0x12ed4a / 0x12eca6,
//! the fishing pole 0x12ef7a, the serpent figure 0x12ff50.

use super::defs::{self, MSG_DONE, MSG_FAILED, MSG_NO_EFFECT};
use super::dispatch::{FMT_NL_LINE, STR_NOT_NOW};
use super::{Ux, seams};
use crate::geom::RULE_HALF;
use crate::objects::{IN_CONTAINER, Slot, object_of_kind_at};
use crate::state::State;
use crate::ui::printf::Arg;

const STR_WITH: u32 = 0x0012_c944;
const FMT_STR: u32 = 0x0012_c94a; // format: the string
const FMT_SP_STR: u32 = 0x0012_c94e; // format: a space, the string
const STR_CODEX: u32 = 0x0012_cf66;
const STR_MANTRA: u32 = 0x0012_cc02;
const STR_WRONG_MANTRA: u32 = 0x0012_cc0c;
const STR_CANT_CARRY_FISH: u32 = 0x0012_cc1c;
const STR_GOT_IT: u32 = 0x0012_cc3a;
const STR_NO_FISH: u32 = 0x0012_cc44;
const STR_NEED_WATER: u32 = 0x0012_cc5a;
const STR_WISH: u32 = 0x0012_ccae;
const STR_WISH_FOR: u32 = 0x0012_ccbc;
const STR_FOOD: u32 = 0x0012_ccc8;
const STR_CANT_CARRY: u32 = 0x0012_ccd8;
const STR_BANG: u32 = 0x0012_ccf4;
const FMT_S: u32 = 0x0012_ccf8;
const STR_NL: u32 = 0x0012_ccfe;
/// The serpent figure's lines (0x12ff50): no egg laid (at 0x1301c2), eggs laid (at 0x1301ce).
const STR_NONE_HATCHED: u32 = 0x0012_ceee;
const STR_HATCHED: u32 = 0x0012_cf00;
/// Port-only (spec 08, slice K2; 2026-10-25): the strings these handlers pass to `printf` as its
/// format, for the locale layer's catalogue ([`super::PRINTED`]): the wand's lead-in, the
/// Codex's line, the shrine's mantra prompt and wrong-mantra line, the fishing pole's four
/// lines, the wishing well's two prompts, its food line and its too-heavy line, and the serpent
/// figure's three lines (its refusal is the checks' too), each on a wrapping line of the
/// message window; and the wand's two syllable formats, the well's message format, the mark
/// that ends its food line and its newline, of no word of their own (the last two under three
/// bytes too), which the catalogue leaves out. The syllables a wand prints go through `%s`:
/// they stay English, as magic's do. The words typed at the shrine and the well are read
/// against tables the catalogue does not hold.
pub(super) const PRINTED: &[u32] = &[
    STR_WITH,
    FMT_STR,
    FMT_SP_STR,
    STR_CODEX,
    STR_MANTRA,
    STR_WRONG_MANTRA,
    STR_CANT_CARRY_FISH,
    STR_GOT_IT,
    STR_NO_FISH,
    STR_NEED_WATER,
    STR_WISH,
    STR_WISH_FOR,
    STR_FOOD,
    STR_CANT_CARRY,
    STR_BANG,
    FMT_S,
    STR_NL,
    STR_NOT_NOW,
    STR_NONE_HATCHED,
    STR_HATCHED,
];
/// Original: A4-0x660e (8 pointers: the shrine mantras, by virtue)
pub(super) const MANTRAS: i32 = -0x660e;
/// Original: A4-0x65ce (20 pointers: the words the wishing well grants food for)
const WISHES: i32 = -0x65ce;

/// Original: 0x1303f4 (objects: use the vortex cube at the Codex: the ending)
///
/// The cube must hold the eight moonstones (one of each frame); on the surface the lens 0x18a
/// at (0x399, 0x353), the lens 0x18c at (0x39d, 0x353) and the Codex 0x3b at (0x39b, 0x353)
/// must be in place and the party within 5 tiles of the Codex ([`RULE_HALF`], the original's
/// window in both builds, spec 13 Decision 3): the Codex goes (0x10c1d6),
/// 0x133d1e, the vanishing line (0x12cf66), a key, the end sequence 0x13513a. False otherwise
/// (the caller prints the cube's refusal).
///
/// Port: the ending is not ported, so the cube refuses before the Codex goes (03 §3: an
/// unported step on a path that destroys something stops it first) and play goes on with the
/// Codex in place.
pub async fn vortex_cube(ux: Ux<'_>, cube: Slot) -> bool {
    let codex = {
        let mut s = ux.s();
        let mut seen = [false; 8];
        let mut c = crate::objects::first_child(&mut s.objects, cube);
        while c >= 0 {
            if let Some(f) = seen.get_mut(usize::from(s.objects.tf(c) >> 10)) {
                *f = true;
            }
            c = crate::objects::next_child(&mut s.objects);
        }
        if !seen.iter().all(|&f| f) {
            return false;
        }
        if object_of_kind_at(&mut s, 0x399, 0x353, 0, 0x18a) == -1
            || object_of_kind_at(&mut s, 0x39d, 0x353, 0, 0x18c) == -1
        {
            return false;
        }
        let codex = object_of_kind_at(&mut s, 0x39b, 0x353, 0, 0x3b);
        let c = s.map.centre;
        let (cx, cy) = (c.x as i16, c.y as i16);
        let r = RULE_HALF;
        let near = cx - r < 0x39c && 0x39a < cx + r && cy - r < 0x354 && 0x352 < cy + r;
        if codex == -1 || !near {
            return false;
        }
        codex
    };
    if !seams::ending_ported() {
        return false;
    }
    // Once the ending is ported: 0x133d1e after the removal, 0x13513a after the key.
    crate::objects::remove(&mut ux.s(), codex);
    ux.print(STR_CODEX, &[]).await;
    ux.key().await;
    true
}

/// Original: 0x12d204 (objects: use a wand: list its spells, cast one, a charge goes)
///
/// The wand's first content (a charge 0x150) names the spell by its quality: its incantation's
/// letters (A4-0x71b4) are printed as their syllables (A4-0x70b8; the game's magic words, not
/// the spell's name), after the line 0x12c944; none → the no-effect message. The spell is
/// cast through magic 0x11a586 and one charge of that quality is taken (0x10d16e).
pub async fn wand(ux: Ux<'_>, obj: Slot) {
    let charge = crate::objects::first_child(&mut ux.s().objects, obj);
    if charge == -1 {
        ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
        return;
    }
    let q = super::quality(&ux.s(), charge);
    ux.print(STR_WITH, &[]).await;
    let list = defs::a4_pointed_list(ux.io, defs::WAND_SPELLS, usize::from(q));
    for (i, &spell) in list.iter().enumerate() {
        let syllable = defs::a4_pointed(ux.io, defs::SPELL_NAMES, usize::from(spell));
        let fmt = if i == 0 { FMT_STR } else { FMT_SP_STR };
        ux.print(fmt, &[Arg::Str(&syllable)]).await;
    }
    {
        let mut s = ux.s();
        if s.ui.input.targeting != 0 {
            s.ui.input.range = 7;
        }
    }
    seams::cast_from_object(ux.state, ux.io, q).await;
    let mut s = ux.s();
    crate::objects::take_from_owner(&mut s, obj, 0x150, u16::from(q) << 8 | 1);
}

/// Original: 0x12c8b8 (objects: play an instrument: digit keys play notes)
///
/// Only with the module buffer allocated (A4-0x35cc): the song playing (A4-0x779a) is kept and
/// song 13, the instrument notes, loaded (0x110878(0xd): the music stops, the module is read even
/// with sound off and not started); the cursor goes off; each digit key ('0' = 10) is stored at
/// buffer +0x3d2 and the buffer played (0x100fa4) until another key; then the kept song again.
/// The instrument in D0 is stored and never read (asm 0x12c8c0).
pub async fn instrument(ux: Ux<'_>, _instrument: u8) {
    if !seams::music_loaded(&ux.s()) {
        return;
    }
    let kept = ux.s().sound.current as i16;
    seams::play_song(&mut ux.s(), crate::sound::SONG_INSTRUMENT as i16);
    ux.s().ui.input.targeting = 0;
    loop {
        let k = ux.key().await;
        let digit = defs::a4_byte(ux.io, defs::CHAR_CLASS, usize::from(k & 0xff)) & 4 != 0;
        if !digit {
            break;
        }
        let note = if k == 0x30 {
            10
        } else {
            (k as u8).wrapping_sub(0x30)
        };
        seams::play_instrument(&mut ux.s(), note);
    }
    seams::play_song(&mut ux.s(), kept);
}

/// Original: 0x12cfe4 (objects: start the machine: its animation runs 16 steps, a sound)
///
/// The animation slot whose tile is the base tile of type 0xec runs 16 steps, step k showing
/// its frame first + k / 4 (asm 0x12d018..0x12d024: tile substitution 0x152cd2, window
/// 0x102c90, overlay 0x135120), and goes back to its first frame; then the view refresh, the
/// song stops, the sound-effect module plays for the wait 0x1623be(100), the song comes back.
///
/// Port: each step's window is presented and held one vertical blank (the Amiga shows it while
/// the next one is drawn; the port publishes only at a wait), the state borrow released.
pub async fn machine(ux: Ux<'_>) {
    let slot = {
        let s = ux.s();
        let base = s.tables.basetile.base.get(0xec).copied();
        let anim = &s.tables.anim;
        anim.slots
            .iter()
            .take(usize::from(anim.count))
            .find(|a| Some(a.tile_to_animate) == base)
            .map(|a| (a.tile_to_animate, a.first_anim_frame))
    };
    if let Some((tile, first)) = slot {
        for step in 0..0x10u16 {
            {
                let mut s = ux.s();
                crate::gfx::anim::substitute(&mut s.gfx, tile, first.wrapping_add(step >> 2));
                crate::gfx::render_window(&mut s, ux.io, true);
                crate::main_loop::anim_step(&mut s);
            }
            ux.io.vblank().await;
        }
        crate::gfx::anim::substitute(&mut ux.s().gfx, tile, first);
    }
    // The song playing now (A4-0x779a), restored after the sound (asm 0x12d06e..0x12d08a).
    let song = {
        let mut s = ux.s();
        crate::gfx::refresh_view(&mut s, ux.io);
        let song = s.sound.current as i16;
        seams::play_song(&mut s, 0);
        seams::play_sound(&mut s);
        song
    };
    seams::wait_vblanks(ux.io, 100).await;
    seams::play_song(&mut ux.s(), song);
}

/// Original: 0x12f0d2 (objects: the wishing well)
///
/// The wish prompt (0x12ccae) and `Y`: the second prompt (0x12ccbc) and a line; one of the 20
/// words at A4-0x65ce gives, 1 in 5, the food line (0x12ccc8) and food (0x81) if the leader can
/// carry it (else the line 0x12ccd8), else the failed message; another word the no-effect
/// message. Any other key prints a newline.
pub async fn wish(ux: Ux<'_>) {
    let m = {
        let s = ux.s();
        crate::objects::party_member(&s, usize::from(s.actors.party.leader_index))
    };
    ux.print(STR_WISH, &[]).await;
    if ux.key().await != 0x59 {
        ux.print(STR_NL, &[]).await;
    } else {
        ux.print(STR_WISH_FOR, &[]).await;
        let line = seams::read_line(ux.state, ux.io, 0x12).await;
        let hit = (0..0x14).any(|i| matches(&defs::a4_pointed(ux.io, WISHES, i), &line));
        if !hit {
            ux.say(FMT_S, MSG_NO_EFFECT).await;
        } else if ux.s().rng.range(0, 4) == 0 {
            ux.print(STR_FOOD, &[]).await;
            let heavy = {
                let mut s = ux.s();
                let w = seams::carried_weight(&mut s, Slot::from(m));
                let st = i16::from(s.actors.attrs.strength[usize::from(m)]);
                w.wrapping_add(2) > st.wrapping_mul(0x14)
            };
            if heavy {
                ux.print(STR_CANT_CARRY, &[]).await;
            } else {
                crate::objects::add_to_owner(&mut ux.s(), Slot::from(m), 0x81, 1);
                ux.print(STR_BANG, &[]).await;
            }
        } else {
            ux.say(FMT_S, MSG_FAILED).await;
        }
    }
    let mut s = ux.s();
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

/// The comparison 0x101724 makes (a C string compare; the typed line against a table word).
/// Case is ignored [likely]: the line editor keeps what was typed.
fn matches(word: &[u8], line: &[u8]) -> bool {
    !word.is_empty() && word.eq_ignore_ascii_case(line)
}

/// Original: 0x12ed4a (objects: chant the mantra at a shrine)
///
/// The mantra prompt (0x12cc02) and a line; it must be the mantra of this shrine (0xf2 +
/// virtue; else the line 0x12cc0c). Then on each tile around the party leader (A4+0x47ec
/// [A4-0x7b7a], asm 0x12edca..0x12ee5a; the shrine is used for its type only) a moonstone
/// (0x49) of the shrine's virtue frees it: the force field 0x33 there goes and the eggs of
/// wisps and daemons around are removed (0x12eca6); the done message with magic's flash
/// 0x115b7c, else the no-effect message.
pub async fn mantra(ux: Ux<'_>, shrine: Slot) {
    let virtue = ux.s().objects.kind(shrine).wrapping_sub(0xf2);
    ux.print(STR_MANTRA, &[]).await;
    let line = seams::read_line(ux.state, ux.io, 4).await;
    ux.put(b'\n').await;
    let found =
        (0..8u16).find(|&i| matches(&defs::a4_pointed(ux.io, MANTRAS, usize::from(i)), &line));
    if found != Some(virtue) {
        ux.print(STR_WRONG_MANTRA, &[]).await;
        return;
    }
    let freed = {
        let mut s = ux.s();
        let l = Slot::from(super::leader(&s));
        let (sx, sy) = (s.objects.x(l), s.objects.y(l));
        let z = u16::from(s.map.centre.z);
        let mut freed = false;
        for i in 0..3u16 {
            for j in 0..3u16 {
                if (i, j) == (1, 1) {
                    continue;
                }
                let (x, y) = ((sx + i).wrapping_sub(1), (sy + j).wrapping_sub(1));
                let stone = object_of_kind_at(&mut s, x, y, z, 0x49);
                if stone < 0 || s.objects.tf(stone) >> 10 != virtue {
                    continue;
                }
                let field = object_of_kind_at(&mut s, x, y, z, 0x33);
                if field >= 0 {
                    crate::objects::remove(&mut s, field);
                }
                let (mx, my) = (s.objects.x(stone), s.objects.y(stone));
                clear_eggs(&mut s, mx as i16, my as i16);
                freed = true;
                break;
            }
        }
        freed
    };
    if freed {
        seams::shrine_flash(ux.state, ux.io).await;
        ux.say(FMT_NL_LINE, MSG_DONE).await;
    } else {
        ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
    }
}

/// Original: 0x12eca6 (objects: remove the wisp / daemon eggs within 20 tiles of (x, y))
///
/// Each egg (0x14f) whose first content is a wisp (0x16a) or daemon (0x16b) loses its contents
/// and goes.
pub fn clear_eggs(s: &mut State, x: i16, y: i16) {
    let mut c = crate::objects::begin_area(s, x - 0x14, y - 0x14, x + 0x14, y + 0x14);
    while c >= 0 {
        if s.objects.kind(c) == 0x14f {
            let mut k = crate::objects::first_child(&mut s.objects, c);
            if matches!(s.objects.kind(k), 0x16a | 0x16b) {
                while k >= 0 {
                    crate::objects::remove(s, k);
                    k = crate::objects::first_child(&mut s.objects, c);
                }
                crate::objects::remove(s, c);
            }
        }
        c = crate::objects::next_in_area(&mut s.objects);
    }
}

/// Original: 0x12ef7a (objects: fish with a fishing pole)
///
/// Water (flags1 bit 0) on a tile the scan reads: unless one was caught this hour (A4-0x7b5f)
/// 1 in 5 catches a fish (0x109 into the inventory, the line 0x12cc3a) if the leader can carry
/// it (else 0x12cc1c), else the line 0x12cc44; no water the line 0x12cc5a.
///
/// The scan runs a 3 × 3 loop skipping (1, 1), but both coordinates take the outer counter
/// (asm 0x12efc4..0x12efd0 add D7 to x and to y): only (x−1, y−1), the leader's own tile and
/// (x+1, y+1) are read. Kept as the original reads them.
pub async fn fish(ux: Ux<'_>) {
    use crate::actors::tail::HOURLY_B;
    let (m, mx, my) = {
        let s = ux.s();
        let m = crate::objects::party_member(&s, usize::from(s.actors.party.leader_index));
        let l = Slot::from(m);
        (
            m,
            crate::objects::member_x(&s, l),
            crate::objects::member_y(&s, l),
        )
    };
    let mut water = false;
    'out: for i in 0..3u16 {
        for j in 0..3u16 {
            if (i, j) == (1, 1) {
                continue;
            }
            let wet = {
                let s = ux.s();
                let z = usize::from(s.map.centre.z);
                // The outer counter for y too (0x12efca..0x12efd0).
                let (x, y) = ((mx + i).wrapping_sub(1), (my + i).wrapping_sub(1));
                let t = s
                    .map
                    .tile_at(usize::from(x), usize::from(y), z)
                    .unwrap_or(0);
                crate::actors::walk::flags1(&s, u16::from(t)) & 1 != 0
            };
            if !wet {
                continue;
            }
            // The hourly byte first; the draw 0x151f06(0, 4) only when it is 0 (asm 0x12eff4,
            // 0x12f000).
            let caught = {
                let mut s = ux.s();
                s.actors.objlist_tail.get(HOURLY_B).copied().unwrap_or(0) == 0
                    && s.rng.range(0, 4) == 0
            };
            if caught {
                let heavy = {
                    let mut s = ux.s();
                    let w = seams::carried_weight(&mut s, Slot::from(m));
                    let st = i16::from(s.actors.attrs.strength[usize::from(m)]);
                    w.wrapping_add(5) > st.wrapping_mul(0x14)
                };
                if heavy {
                    ux.print(STR_CANT_CARRY_FISH, &[]).await;
                } else {
                    ux.print(STR_GOT_IT, &[]).await;
                    let mut s = ux.s();
                    crate::objects::create_in(&mut s, 0x109, 0x10, 1, Slot::from(m));
                    s.actors.objlist_tail[HOURLY_B] = 1;
                }
            } else {
                ux.print(STR_NO_FISH, &[]).await;
            }
            water = true;
            break 'out;
        }
    }
    if !water {
        ux.print(STR_NEED_WATER, &[]).await;
    }
    let mut s = ux.s();
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

/// Original: 0x12ff50 (objects: the serpent figure lays the eggs of silver serpents)
///
/// A silver serpent (0x19d) in one of actor slots 0xe0..0x100 (asm 0x12ff5c..0x12ff90): the
/// line 0x12cee2 and nothing else. Else a probe 0x19d is made at (0, 0) on the party's level
/// (0x10bc4c at 0x12ffd0) and up to three eggs laid (0x12ffde..0x1301ae). Each takes at most
/// 8 tries of a side 0x151f06(0, 3) and an offset d 0x151f06(0, 10) round the centre: (x − 6,
/// y + d − 6), (x + d − 6, y − 7), (x + 7, y + d − 6) or (x + d − 6, y + 6) (0x130020..0x13007a).
/// The first spot where the probe may stand (0x11b1c0) at (x, y), (x − 1, y), (x, y + 1),
/// (x − 1, y + 1), (x + 1, y) and (x, y − 1) takes an egg 0x14f of quantity 100 and quality 4
/// (0x10bc4c at 0x130172) holding a 0x19d of quality 8 and quantity 0x151f06(4, 8) (0x10be02
/// at 0x13019a), and the egg hatches (0x1319b2). The probe goes (0x10c1d6 at 0x1301b8); the
/// line 0x12cf00 when an egg was laid, else 0x12ceee.
///
/// With the object pool full (0x10bc4c: -1) the original passes the -1 on; the port does not.
/// No probe: the port finds no spot, where the original tests slot -1 (0x11b1c0 reads the word
/// before the type table, 0x137986, the location bytes 0x135585..0x135587 and the actor byte
/// A4+0x3d0b), so its spots, draws and line depend on those bytes; nor does the port clear
/// A4-0x6a96, which each 0x11b1c0 call clears first (0x11b1d2). No egg: the port draws its
/// serpent's quantity and counts it, as the original does (the add at 0x1301aa is
/// unconditional), but makes no contents (0x10be02 finds the pool as full: 0x10a79a) and
/// hatches nothing; the original's 0x1319b2(-1), unless A4-0x7afb is set or the hour test on
/// the byte A4+0x250a fails, draws 0x151f06(1, 100) once while bit 0x40 of the byte before the
/// status table (A4+0x190b) is clear, walks what 0x10ae52 finds in slot -1 should that draw not
/// exceed the byte A4+0x250b, and sets 0x42 in A4+0x190b. The count, and so the line, match
/// when the probe was made.
pub async fn serpents(ux: Ux<'_>) {
    let line = {
        let mut s = ux.s();
        let out = (0xe0..0x100).any(|a: Slot| {
            let tf = s.objects.tf(a);
            tf != 0 && tf & 0x3ff == 0x19d
        });
        if out {
            STR_NOT_NOW
        } else {
            let c = s.map.centre;
            let z = u16::from(c.z);
            let new = |s: &mut State, x: u16, y: u16, tf: u16, qq: u16| {
                let [b0, b1, b2] = crate::objects::pack_xyz(x, y, z).to_packed();
                let ([t0, t1], [q0, q1]) = (tf.to_be_bytes(), qq.to_be_bytes());
                crate::objects::create_from_record(s, [0, b0, b1, b2, t0, t1, q0, q1])
            };
            let probe = new(&mut s, 0, 0, 0x19d, 1);
            let mut laid = 0;
            for _ in 0..3 {
                let mut spot = None;
                for _ in 0..8 {
                    let side = s.rng.range(0, 3);
                    let d = s.rng.range(0, 10);
                    let at = |dx: i16, dy: i16| {
                        (c.x.wrapping_add_signed(dx), c.y.wrapping_add_signed(dy))
                    };
                    let d = d as i16;
                    let (x, y) = match side {
                        0 => at(-6, d - 6),
                        1 => at(d - 6, -7),
                        2 => at(7, d - 6),
                        _ => at(d - 6, 6),
                    };
                    let around = [(0, 0), (-1, 0), (0, 1), (-1, 1), (1, 0), (0, -1)]
                        .map(|(dx, dy)| (x.wrapping_add_signed(dx), y.wrapping_add_signed(dy)));
                    if probe >= 0
                        && around.into_iter().all(|(x, y)| {
                            crate::actors::walk::can_move_to(&mut s, probe as u16, x, y)
                        })
                    {
                        spot = Some((x, y));
                        break;
                    }
                }
                let Some((x, y)) = spot else {
                    continue;
                };
                let egg = new(&mut s, x, y, 0x14f, 0x0464);
                let n = s.rng.range(4, 8);
                if egg >= 0 {
                    crate::objects::create_in(&mut s, 0x19d, IN_CONTAINER, 0x0800 | n, egg);
                    crate::objects::hatch_egg(&mut s, egg);
                }
                laid += 1;
            }
            crate::objects::remove(&mut s, probe);
            if laid == 0 {
                STR_NONE_HATCHED
            } else {
                STR_HATCHED
            }
        }
    };
    ux.print(line, &[]).await;
}
