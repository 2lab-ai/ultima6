//! Calls from the combat code into functions other modules own. Ported owners are called
//! directly (integration M2: objects 0x10e596, 0x12f8fe; the pointer merge: graphics 0x15265a,
//! 0x152800, 0x152876, 0x1521f4); the others are the narrowest signature the combat code needs,
//! a `todo_stub!` with its contract and an explicit fallback. Proposed owner in brackets.

use crate::engine::with_current_io;
use crate::objects::{self, Slot};
use crate::state::State;
use crate::todo_stub;

/// [graphics] 0x15265a(slot, tile, angle, cx, cy): build sprite `slot` from object tile `tile`
/// turned by `angle` (0..127 = a full turn) about (cx, cy); the slot's background flag is
/// cleared: [`crate::gfx::sprite::prepare`].
pub(crate) fn sprite_prepare(s: &mut State, slot: u16, tile: u16, angle: u16, cx: u16, cy: u16) {
    crate::gfx::sprite::prepare(&mut s.gfx, slot, tile, angle, cx as i16, cy as i16);
}

/// [graphics] 0x152800(slot, x, y): save the back-buffer block under a sprite centred on
/// (x, y) and mark the slot as holding it: [`crate::gfx::sprite::save`].
pub(crate) fn sprite_save(s: &mut State, slot: u16, x: u16, y: u16) {
    let gfx = &mut s.gfx;
    with_current_io(|io| io.draw_back(|back| crate::gfx::sprite::save(gfx, back, slot, x, y)));
}

/// [graphics] 0x152876(slot, x, y): a slot holding a saved background puts it back; any other
/// slot draws its sprite centred on (x, y) into the back buffer: [`crate::gfx::sprite::draw`].
pub(crate) fn sprite_draw(s: &mut State, slot: u16, x: u16, y: u16) {
    let gfx = &s.gfx;
    with_current_io(|io| io.draw_back(|back| crate::gfx::sprite::draw(gfx, back, slot, x, y)));
}

/// [objects] 0x10c6a6(actor): the remains of a dead actor: a map object of its type + frame
/// (status | 1, quality = the actor, temporary only for monsters) on its tile with everything
/// it carried moved inside; a pool of type 0x152 under it is moved to the top; the actor goes
/// to level 15. Returns the remains, -1 when no slot is free. Fallback: that behaviour, written
/// from 0x10c6a6's assembly with the ported object calls (0x10ad1a, 0x10bcee, 0x10ae52,
/// 0x10cda8, 0x10c9d2).
pub(crate) fn remains(s: &mut State, actor: i16) -> Slot {
    let a = actor as usize;
    if actor < 0 || s.objects.type_frame[a] == 0 {
        return -1;
    }
    let p = s.objects.location[a];
    let (x, y, z) = (p.x, p.y, u16::from(p.z));
    let under = objects::begin_at(&mut s.objects, &s.tables, x, y, z);
    s.objects.allow_type_4c = 1;
    let status = s.objects.obj_status[a] | 1;
    let tf = s.objects.type_frame[a];
    let body = objects::create_on_map(&mut s.objects, tf, status, (actor as u16) << 8, x, y, z);
    if body >= 0 {
        if actor < 0xe0 {
            s.objects.obj_status[body as usize] &= !0x20;
        }
        loop {
            let c = objects::first_child(&mut s.objects, actor);
            if c < 0 {
                break;
            }
            objects::move_into(s, c, body, 8);
            s.objects.obj_status[c as usize] |= 1;
        }
        if under >= 0 && s.objects.type_frame[under as usize] & 0x3ff == 0x152 {
            objects::move_to(s, under, x, y, z);
        }
        objects::move_to(s, actor, x, y, 0xf);
    }
    s.objects.allow_type_4c = 0;
    todo_stub!(
        "0x10c6a6 make an actor's remains (fallback: ported behaviour)",
        body
    )
}

/// The magic hunk's refusal line (PC-relative 0x1159f8), shown for a cast the port cannot run.
const CAST_REFUSED: u32 = 0x0011_59f8;

/// Port-only (spec 08, slice K2; 2026-10-24): the strings these calls pass to `printf` as its
/// format (queued through [`super::present::say`]), for the locale layer's catalogue
/// ([`crate::locale::strings`]): the refusal line. Its address is magic's not-possible line as
/// well (`crate::magic::items`' `MSG_NOT_POSSIBLE`, printed through `printf`): the catalogue is
/// keyed by content, so that line shows the pack's form too.
pub(crate) const PRINTED: &[u32] = &[CAST_REFUSED];

/// [magic] 0x11a586(spell, 1) from a staff or wand: the spell its quality names, cast by the
/// attacker as party slot 16 (A4+0x47fc, leader index 16) at the selection with the text off;
/// the hit then reads the result byte A4+0x181e (asm 0x12467a..0x12476e).
///
/// Still a stub after the magic merge: magic's dispatch ([`crate::magic::dispatch`]) awaits (it
/// prints and flies) while the hit 0x124232 that casts here is synchronous (the Attack command
/// and the NPC worktypes), and magic's caster has no slot 16. Fallback: the cast is refused
/// before the staff's charge is taken: the refusal line, the prompt reprinted, nothing cast.
pub(crate) fn cast_from_item(s: &mut State) {
    super::present::say(s, CAST_REFUSED, vec![]);
    s.actors.work.prompt = 1;
    todo_stub!(
        "0x11a586 cast a spell from a staff or wand (fallback: refused before the charge)",
        ()
    )
}

/// [magic] 0x1165d2(attacker, target): weapon 0x5b puts the actor it hit to sleep (D0 = the
/// attacker, D1 = A4+0x185c, asm 0x122ec6..0x122ed8): [`crate::magic::sleep`]; its result
/// byte is dropped.
pub(crate) fn weapon_spell(s: &mut State, target: i16, attacker: i16) {
    crate::magic::sleep(s, attacker as u16, target as u16);
}

/// [objects] 0x12f8fe(actor): a rider dismounts before it lies down:
/// [`crate::objects::commands::dismount`].
pub(crate) fn dismount(s: &mut State, actor: i16) {
    crate::objects::commands::dismount(s, actor);
}

/// [sound] 0x110878(song): plays song `n` (7 = the combat song): [`crate::sound::play_song`].
pub(crate) fn play_song(s: &mut State, song: u16) {
    crate::sound::play_song(s, song);
}

/// [sound] 0x110912: the song of the area: [`crate::sound::play_area_song`].
pub(crate) fn area_song(s: &mut State) {
    crate::sound::play_area_song(s);
}

/// [objects] 0x10e596(word, cx, cy, mode): add a tile to the draw list of window cell (cx, cy):
/// [`crate::objects::add_tile`].
pub(crate) fn add_tile(s: &mut State, word: u16, cx: i16, cy: i16, mode: u16) {
    objects::add_tile(s, word, cx, cy, mode);
}

/// [sound] A4-0x779a: the song playing (`sound.current`).
pub(crate) fn current_song(s: &State) -> u16 {
    s.sound.current
}

/// [map] 0x109c12(x0, y0, 0x28, 2): flood the sight from window cell (x0, y0) into the cell
/// flags (bit 7 = seen). `map/view.rs` ports 0x109c12 for its modes 0 (light) and 1 (sight)
/// only; the overview's mode 2 is not ported. Fallback: every cell seen.
pub(crate) fn flood_sight(s: &mut State, _x0: i16, _y0: i16) {
    s.map.view_flags.iter_mut().for_each(|f| *f |= 0x80);
    todo_stub!(
        "0x109c12 flood the sight for the overview (fallback: all seen)",
        ()
    )
}

/// [graphics] 0x1521f4(x0, y0, x1, y1): fill the rectangle with the plot colour, into the back
/// buffer while A4-0x6179 is set: [`crate::gfx::blit::fill_rect`].
pub(crate) fn fill_rect(s: &mut State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let gfx = &s.gfx;
    let (x0, y0, x1, y1) = (x0 as u16, y0 as u16, x1 as u16, y1 as u16);
    with_current_io(|io| {
        let fill = |fb: &mut u6_render::Framebuffer| {
            crate::gfx::blit::fill_rect(gfx, fb, x0, y0, x1, y1);
        };
        if gfx.plot_back {
            io.draw_back(fill);
        } else {
            io.draw(fill);
        }
    });
}
