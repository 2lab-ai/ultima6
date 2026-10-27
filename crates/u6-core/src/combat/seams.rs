//! Calls from the combat code into functions other modules own, the owner in brackets. They
//! forward to the ported owner (objects 0x10e596, 0x12f8fe; graphics 0x15265a, 0x152800,
//! 0x152876, 0x1521f4; magic 0x1165d2 and a spellbook's or spell object's cast 0x11a586; sound
//! 0x110878, 0x110912 and the song word A4-0x779a; map 0x109c12's overview flood). The remains
//! 0x10c6a6 are ported here from the assembly with the ported object calls.

use std::cell::RefCell;

use crate::engine::{Io, with_current_io};
use crate::objects::{self, Slot};
use crate::state::State;

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

/// Original: 0x10c6a6 (objects: the remains of a dead actor)
///
/// A map object of its type + frame word (status | 1, quality = the actor, temporary only for
/// monsters) on its tile with everything it carried moved inside (0x10cda8 with 8, then status
/// bit 0) while A4-0x78da allows type 0x4c; a pool of type 0x152 first at the tile is moved to
/// the top; the actor goes to level 15. Returns the remains, -1 when no slot is free (asm
/// 0x10c6a6..0x10c80e). With nothing at the tile the original tests the type word of slot -1
/// (D4 = -1, asm 0x10c7c0..0x10c7d6: the last two location bytes of slot 0xbff) and on a 0x152
/// calls 0x10c9d2 with -1, which returns -1 at its first test (asm 0x10c9e2..0x10c9e8) and the
/// caller drops; the port skips the test.
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
    body
}

/// Original: 0x124232's call of 0x11a586 for a spellbook's or spell object's cast (asm
/// 0x1246e6..0x1246fe: D0 = the item's quality, D1 = 1, the selection chosen already)
///
/// [magic] the spell runs at the selection: [`crate::magic::dispatch`] with `chosen`
/// ([`super::attack::cast_item`] sets the caster, the selection and the text switch around it).
pub(crate) async fn cast_from_item(state: &RefCell<State>, io: &Io, spell: u8) {
    crate::magic::dispatch(state, io, u16::from(spell), true).await;
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
/// flags (bit 7 = seen), to every cell it reaches: [`crate::map::view::flood_overview`].
pub(crate) fn flood_sight(s: &mut State, x0: i16, y0: i16) {
    crate::map::view::flood_overview(s, x0, y0);
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
