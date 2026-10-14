//! Calls from the combat code into functions other modules own. Ported owners are called
//! directly (integration M2: objects 0x10e596, 0x12f8fe); the others are the narrowest signature
//! the combat code needs, a `todo_stub!` with its contract and an explicit fallback. Proposed
//! owner in brackets.

use crate::engine::with_current_io;
use crate::objects::{self, Slot};
use crate::state::State;
use crate::todo_stub;

/// Sprite slots of the flight drawing (graphics hunk 26: the 16 prepared sprites at
/// A4-relative 0x155584 + 0x6900, their "background saved" bytes 0x1525c4 and the 8 saved
/// backgrounds 0x14df70). Port-only state of the fallbacks below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprites {
    /// Tile prepared in each slot (0x15265a), 0 = none.
    pub tiles: [u16; 16],
    /// 0x1525c4: the slot holds a saved background (0x152800 sets it, 0x15265a clears it).
    pub saved: [bool; 16],
    /// 0x14df70: 24 × 16 pixels under each of the first 8 slots.
    pub under: Vec<Vec<u8>>,
}

impl Default for Sprites {
    fn default() -> Self {
        Self {
            tiles: [0; 16],
            saved: [false; 16],
            under: vec![vec![0; 24 * 16]; 8],
        }
    }
}

/// [graphics] 0x15265a(slot, tile, angle, cx, cy): build sprite `slot` from object tile `tile`
/// turned by `angle` (0..127 = a full turn) about (cx, cy) in eighths of a pixel. Contract: the
/// slot's background flag is cleared. Fallback: the tile unturned.
pub(crate) fn sprite_prepare(s: &mut State, slot: u16, tile: u16, angle: u16, cx: u16, cy: u16) {
    let _ = (angle, cx, cy);
    let i = usize::from(slot & 0xf);
    s.combat.sprites.tiles[i] = tile;
    s.combat.sprites.saved[i] = false;
    todo_stub!(
        "0x15265a build a turned missile sprite (fallback: unturned tile)",
        ()
    )
}

/// The 24 × 16 back-buffer block 0x152800 / 0x152876 address for a sprite centred on (x, y):
/// byte column (x − 8) / 8, rows y − 8 .. y + 8.
fn block(x: u16, y: u16) -> (usize, usize) {
    (
        usize::from(x.wrapping_sub(8) >> 3) * 8,
        usize::from(y.wrapping_sub(8)),
    )
}

/// [graphics] 0x152800(slot, x, y): save the back-buffer block under a sprite centred on
/// (x, y) and mark the slot as holding it. Fallback: the same copy on the port's buffers.
pub(crate) fn sprite_save(s: &mut State, slot: u16, x: u16, y: u16) {
    let i = usize::from(slot & 7);
    let (bx, by) = block(x, y);
    let under = &mut s.combat.sprites.under[i];
    with_current_io(|io| {
        io.draw_back(|fb| {
            for r in 0..16 {
                for c in 0..24 {
                    under[r * 24 + c] = fb.get(bx + c, by + r).unwrap_or(0);
                }
            }
        })
    });
    s.combat.sprites.saved[usize::from(slot & 0xf)] = true;
    todo_stub!(
        "0x152800 save the background under a sprite (fallback: port copy)",
        ()
    )
}

/// [graphics] 0x152876(slot, x, y): a slot holding a saved background puts it back; any other
/// slot draws its sprite centred on (x, y) into the back buffer. Fallback: the background copy,
/// or the slot's tile blitted at (x − 8, y − 8).
pub(crate) fn sprite_draw(s: &mut State, slot: u16, x: u16, y: u16) {
    let i = usize::from(slot & 0xf);
    let saved = s.combat.sprites.saved[i];
    let tile = s.combat.sprites.tiles[i];
    let (bx, by) = block(x, y);
    let st = &*s;
    with_current_io(|io| {
        io.draw_back(|fb| {
            if saved {
                let under = &st.combat.sprites.under[i & 7];
                for r in 0..16 {
                    for c in 0..24 {
                        fb.set(bx + c, by + r, under[r * 24 + c]);
                    }
                }
            } else if tile != 0 {
                let (dx, dy) = (x.wrapping_sub(8), y.wrapping_sub(8));
                crate::gfx::blit::blit_back(&st.gfx, fb, tile, dx, dy);
            }
        })
    });
    todo_stub!(
        "0x152876 draw or restore a sprite (fallback: port blit)",
        ()
    )
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

/// [magic] 0x11a586(spell, 1): cast spell `spell` from a staff or wand held by `caster` (the
/// original makes the caster party slot 16, A4+0x47fc, with leader index 16; the target is the
/// selection). Returns the outcome byte the caller reads from A4+0x181e (0xfe charmed, 0
/// paralysed). Fallback: nothing is cast, outcome 1 (no message).
pub(crate) fn cast_from_item(_s: &mut State, _spell: u16, _caster: i16) -> u8 {
    todo_stub!(
        "0x11a586 cast a spell from a staff or wand (fallback: none)",
        1
    )
}

/// [magic] 0x1165d2(target, attacker): the effect of a hit with weapon 0x5b on an actor.
/// Fallback: none.
pub(crate) fn weapon_spell(_s: &mut State, _target: i16, _attacker: i16) {
    todo_stub!("0x1165d2 weapon 0x5b strikes an actor (fallback: none)", ())
}

/// [objects] 0x12f8fe(actor): a rider dismounts before it lies down:
/// [`crate::objects::commands::dismount`].
pub(crate) fn dismount(s: &mut State, actor: i16) {
    crate::objects::commands::dismount(s, actor);
}

/// [sound] 0x110878(song): plays song `n` (7 = the combat song). Fallback: silence.
pub(crate) fn play_song(_s: &mut State, _song: u16) {
    todo_stub!("0x110878 play a song (fallback: silence)", ())
}

/// [sound] 0x110912: the song of the area. Fallback: silence.
pub(crate) fn area_song(_s: &mut State) {
    todo_stub!(
        "0x110912 pick and play the area song (fallback: silence)",
        ()
    )
}

/// [objects] 0x10e596(word, cx, cy, mode): add a tile to the draw list of window cell (cx, cy):
/// [`crate::objects::add_tile`].
pub(crate) fn add_tile(s: &mut State, word: u16, cx: i16, cy: i16, mode: u16) {
    objects::add_tile(s, word, cx, cy, mode);
}

/// [sound] A4-0x779a: the song playing. Fallback: 7 (the combat song 0x121c64 starts), so
/// breaking off combat asks for the area song as the original does after a fight.
pub(crate) fn current_song(_s: &State) -> u16 {
    todo_stub!("0x110878 song playing (A4-0x779a; fallback: 7)", 7)
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

/// [graphics] 0x1521f4(x0, y0, x1, y1): fill the rectangle with the plot colour (into the back
/// buffer while A4-0x6179 is set). Fallback: the same fill pixel by pixel through 0x152188.
pub(crate) fn fill_rect(s: &mut State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let gfx = &s.gfx;
    with_current_io(|io| {
        let fill = |fb: &mut u6_render::Framebuffer| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    crate::gfx::blit::plot(gfx, fb, x as u16, y as u16);
                }
            }
        };
        if gfx.plot_back {
            io.draw_back(fill);
        } else {
            io.draw(fill);
        }
    });
    todo_stub!("0x1521f4 fill a rectangle (fallback: pixel fill)", ())
}
