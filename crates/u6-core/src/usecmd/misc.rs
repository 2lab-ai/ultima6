//! The rest of the Use handlers: the vortex cube 0x1303f4, wands 0x12d204, instruments
//! 0x12c8b8, the machine 0x12cfe4, the wishing well 0x12f0d2, shrines 0x12ed4a / 0x12eca6,
//! the fishing pole 0x12ef7a, the serpent figure 0x12ff50.

use super::defs::{self, MSG_DONE, MSG_FAILED, MSG_NO_EFFECT};
use super::dispatch::FMT_NL_LINE;
use super::{Ux, seams};
use crate::objects::{Slot, object_of_kind_at};
use crate::state::State;
use crate::todo_stub;
use crate::ui::printf::Arg;

const STR_WITH: u32 = 0x0012_c944;
const FMT_STR: u32 = 0x0012_c94a; // "%s"
const FMT_SP_STR: u32 = 0x0012_c94e; // " %s"
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
/// Original: A4-0x660e (8 pointers: the shrine mantras, by virtue)
const MANTRAS: i32 = -0x660e;
/// Original: A4-0x65ce (20 pointers: the words the wishing well grants food for)
const WISHES: i32 = -0x65ce;
/// Original: A4-0x779a (word: the song playing)
const SONG: i32 = -0x779a;

/// Original: 0x1303f4 (objects: use the vortex cube at the Codex: the ending)
///
/// The cube must hold the eight moonstones (one of each frame); on the surface the lens 0x18a
/// at (0x399, 0x353), the lens 0x18c at (0x39d, 0x353) and the Codex 0x3b at (0x39b, 0x353)
/// must be in place and the party within 4 tiles of the Codex: the Codex goes, "The Codex has
/// vanished!", a key, the ending (0x133d1e, 0x13513a). False otherwise ("No effect").
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
        let near = cx - 5 < 0x39c && 0x39a < cx + 5 && cy - 5 < 0x354 && 0x352 < cy + 5;
        if codex == -1 || !near {
            return false;
        }
        codex
    };
    crate::objects::remove(&mut ux.s(), codex);
    ux.print(STR_CODEX, &[]).await;
    ux.key().await;
    seams::ending(&mut ux.s());
    true
}

/// Original: 0x12d204 (objects: use a wand: "With" its spells, cast one, a charge goes)
///
/// The wand's first content (a charge 0x150) names the spell list by its quality (A4-0x71b4,
/// names at A4-0x70b8); none → "No effect". The spell is cast through magic 0x11a586 and one
/// charge of that quality is taken (0x10d16e).
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
        let name = defs::a4_pointed(ux.io, defs::SPELL_NAMES, usize::from(spell));
        let fmt = if i == 0 { FMT_STR } else { FMT_SP_STR };
        ux.print(fmt, &[Arg::Str(&name)]).await;
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

/// Original: 0x12c8b8 (objects: play an instrument: digit keys play notes of `instrument`)
///
/// Only with the music module loaded (A4-0x35cc): the song stops, the cursor goes off, each
/// digit key plays its note ('0' = 10) until another key; then the song stops again.
pub async fn instrument(ux: Ux<'_>, instrument: u8) {
    if !seams::music_loaded(&ux.s()) {
        return;
    }
    seams::play_song(&mut ux.s(), 0);
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
        seams::play_instrument(&mut ux.s(), instrument, note);
    }
    seams::play_song(&mut ux.s(), 0);
}

/// Original: 0x12cfe4 (objects: start the machine: its animation runs 16 frames, a sound)
///
/// The animation slot whose tile is the base tile of type 0xec steps its 16 frames (tile
/// substitution 0x152cd2, window 0x102c90, overlay 0x135120) and goes back to its first frame;
/// then the view refresh, the song stops, a sound plays, the screen effect 0x1623be(100), the
/// song comes back.
pub async fn machine(ux: Ux<'_>) {
    {
        let mut s = ux.s();
        let base = s.tables.basetile.base.get(0xec).copied();
        let anim = s.tables.anim.clone();
        let slot = anim
            .slots
            .iter()
            .take(usize::from(anim.count))
            .find(|a| Some(a.tile_to_animate) == base);
        if let Some(a) = slot {
            for i in 0..0x10 {
                crate::gfx::anim::substitute(&mut s.gfx, a.tile_to_animate, a.first_anim_frame + i);
                crate::gfx::render_window(&mut s, ux.io, true);
                crate::main_loop::anim_step(&mut s);
            }
            crate::gfx::anim::substitute(&mut s.gfx, a.tile_to_animate, a.first_anim_frame);
        }
        crate::gfx::refresh_view(&mut s, ux.io);
        let song = defs::a4_words(ux.io, SONG, 1).first().copied().unwrap_or(0) as i16;
        seams::play_song(&mut s, 0);
        seams::play_sound(&mut s);
        seams::screen_effect(ux.io);
        seams::play_song(&mut s, song);
    }
}

/// Original: 0x12f0d2 (objects: the wishing well)
///
/// "Make a wish?" and `Y`: "Wish for:" and a line; one of the 20 words at A4-0x65ce gives, 1 in
/// 5, "You got food" and food (0x81) if the leader can carry it, else "Failed"; another word
/// "No effect". Any other key prints a newline.
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
/// "Mantra:" and a line; it must be the mantra of this shrine (0xf2 + virtue). Then on each
/// tile around the shrine a moonstone (0x49) of the shrine's virtue frees it: the force field
/// 0x33 there goes and the eggs of wisps and daemons around are removed (0x12eca6); "Done"
/// with magic's flash 0x115b7c, else "No effect".
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
        let (sx, sy) = (s.objects.x(shrine), s.objects.y(shrine));
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
/// Water (flags1 bit 0) on a tile around the leader: unless one was caught this hour
/// (A4-0x7b5f) 1 in 5 catches a fish (0x109 into the inventory, "Got it!") if the leader can
/// carry it ("Got it, but can't carry it."), else "Didn't get a fish."; no water "You need to
/// stand next to water.".
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
                let (x, y) = ((mx + i).wrapping_sub(1), (my + j).wrapping_sub(1));
                let t = s
                    .map
                    .tile_at(usize::from(x), usize::from(y), z)
                    .unwrap_or(0);
                crate::actors::walk::flags1(&s, u16::from(t)) & 1 != 0
            };
            if !wet {
                continue;
            }
            let caught = ux
                .s()
                .actors
                .objlist_tail
                .get(HOURLY_B)
                .copied()
                .unwrap_or(0)
                == 0
                && ux.s().rng.range(0, 4) == 0;
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

/// Stub of 0x12ff50 (objects: the serpent figure summons silver serpents)
///
/// Stub contract: "Not now!" while a silver serpent (0x19d) is among the temporary actors;
/// else up to three eggs of serpents hatch around the party (RNG offsets, 0x11b1c0 checks,
/// 0x10bc4c / 0x10be02, 0x1319b2) and "Silver snakes are generated!" or "None generated!".
/// Fallback: nothing happens, no RNG draw. Not on the Use receipt paths.
pub async fn serpents(_ux: Ux<'_>) {
    todo_stub!("0x12ff50 summon silver serpents", ())
}
