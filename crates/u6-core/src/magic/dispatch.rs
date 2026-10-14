//! The spell dispatcher 0x11a586: one case per spell number (a jump table of 0x81 entries at
//! 0x11a5ac), each calling the routine of its spell kind with the missile tile, the range,
//! the kind of target and whether the target is chosen already.

use std::cell::RefCell;

use super::effects::{Target, on_target};
use super::{detect, spells};
use crate::engine::Io;
use crate::main_loop::game_string;
use crate::state::State;
use crate::ui::printf::printf;

/// The newline 0x11a586 prints first.
const MSG_NEWLINE: u32 = 0x0011_58ba;
/// Timers A4-0x7b72 + n the dispatcher sets.
const LIGHT: usize = 0;
const INFRAVISION: usize = 1;
const NEGATE: usize = 13;
const TIME_STOP: usize = 14;
const ECLIPSE: usize = 15;

/// The spells that flash the caster and the window and set a timer (or reread the clock).
async fn timed(state: &RefCell<State>, io: &Io, spell: u16, timer: usize, value: u8, tick: bool) {
    let caster = {
        let s = state.borrow();
        super::cast::caster(&s)
    };
    spells::flash_actor(state, io, caster, 0).await;
    spells::flash_window(state, io, spell / 16 + 1).await;
    let mut s = state.borrow_mut();
    s.actors.timers[timer] = value;
    if tick {
        crate::actors::clock::tick(&mut s, 0);
    }
}

/// Original: 0x11a586 (magic: run spell `spell`; `chosen` = its target is chosen already)
///
/// A newline, the spell's case, then the view flags (0x109e74) and the view refresh (0x102ef0).
/// Spell numbers without a spell (circle × 16 + 10..15, 0x7a..0x7f) do nothing.
pub async fn dispatch(state: &RefCell<State>, io: &Io, spell: u16, chosen: bool) {
    let nl = game_string(io, MSG_NEWLINE);
    printf(state, io, &nl, &[]).await;
    let pre = i16::from(chosen);
    let any = |tile| Target {
        tile,
        range: 8,
        kind: -1,
        chosen,
    };
    let actor = |tile| Target {
        tile,
        range: 8,
        kind: 1,
        chosen,
    };
    match spell {
        0x00 => spells::create_food(state, io, spell).await,
        0x01 | 0x02 | 0x16 | 0x17 | 0x18 | 0x24 => {
            detect::detect(state, io, spell, 8, -1, chosen).await
        }
        0x32 => detect::detect(state, io, spell, 8, 1, chosen).await,
        0x03 | 0x05 | 0x06 | 0x20 | 0x28 | 0x34 | 0x45 | 0x50 | 0x51 => {
            on_target(state, io, spell, actor(0x17f)).await
        }
        0x04 => on_target(state, io, spell, any(0x18b)).await,
        0x08 => on_target(state, io, spell, any(0x18d)).await,
        0x19 | 0x21 | 0x30 | 0x58 => on_target(state, io, spell, any(0x17f)).await,
        0x11 => on_target(state, io, spell, actor(0x236)).await,
        0x12 => on_target(state, io, spell, actor(0x17c)).await,
        0x14 => on_target(state, io, spell, actor(0x17d)).await,
        0x22 => on_target(state, io, spell, actor(0x17e)).await,
        0x43 => on_target(state, io, spell, actor(0x18b)).await,
        0x44 | 0x60 => on_target(state, io, spell, actor(0x188)).await,
        0x65 => on_target(state, io, spell, actor(0x18c)).await,
        0x07 => {
            spells::flash_window(state, io, spell / 16 + 1).await;
            crate::map::travel::resurrect_party(&mut state.borrow_mut());
        }
        0x09 => timed(state, io, spell, LIGHT, 100, true).await,
        0x10 => timed(state, io, spell, INFRAVISION, 0x14, false).await,
        0x23 => timed(state, io, spell, LIGHT, 0xff, true).await,
        0x56 => timed(state, io, spell, NEGATE, 0x14, false).await,
        0x72 => timed(state, io, spell, ECLIPSE, 0x14, false).await,
        0x78 => timed(state, io, spell, TIME_STOP, 10, false).await,
        0x13 => spells::reappear(state, io, pre).await,
        0x15 => spells::telekinesis(state, io, pre).await,
        0x25 | 0x36 | 0x55 | 0x66 => spells::area(state, io, spell, 0x18b, 0x17f, pre).await,
        0x26 => spells::area(state, io, spell, 0x18b, 0x17d, pre).await,
        0x41 => spells::area(state, io, spell, 0x189, 0x17e, pre).await,
        0x67 | 0x73 => spells::area(state, io, spell, 0x188, 0x18a, pre).await,
        0x74 => spells::area(state, io, spell, 0x188, 0x18c, pre).await,
        0x27 => spells::peer(state, io, spell).await,
        0x29 | 0x47 | 0x52 | 0x63 => spells::all_foes(state, io, spell).await,
        0x31 | 0x77 => {
            let caster = super::cast::caster(&state.borrow());
            spells::summon(state, io, spell, caster).await
        }
        0x33 => spells::field(state, io, spell, 0x17e, pre).await,
        0x37 => spells::field(state, io, spell, 0x17c, pre).await,
        0x38 => spells::field(state, io, spell, 0x17d, pre).await,
        0x40 => spells::field(state, io, spell, 0x17f, pre).await,
        0x35 => spells::locate(state, io, spell).await,
        0x39 => spells::wind_change(state, io).await,
        0x42 => spells::insect_swarm(state, io).await,
        0x46 => spells::pickpocket(state, io).await,
        0x48 => spells::seance(state, io, pre).await,
        0x49 => spells::xray(state, io).await,
        0x53 => spells::wind(state, io, spell, 0x18d, pre).await,
        0x57 => spells::wind(state, io, spell, 0x17c, pre).await,
        0x62 => spells::wind(state, io, spell, 0x188, pre).await,
        0x71 => spells::wind(state, io, spell, 0x18c, pre).await,
        0x54 => spells::hail_storm(state, io, pre).await,
        0x59 => spells::web(state, io).await,
        0x61 => spells::enchant(state, io).await,
        0x64 => spells::gate_travel(state, io).await,
        0x68 => spells::wing_strike(state, io).await,
        0x69 => {
            let c = state.borrow().map.centre;
            spells::wizard_eye(state, io, c.x as i16, c.y as i16, 1).await
        }
        0x70 => {
            spells::flash_window(state, io, spell / 16 + 1).await;
            spells::armageddon(state, io).await
        }
        0x75 => spells::resurrect_spell(state, io, pre).await,
        0x76 => spells::slime(state, io).await,
        0x79 => spells::tremor(state, io).await,
        0x80 => spells::spell_0x80(state, io).await,
        _ => {}
    }
    {
        let mut s = state.borrow_mut();
        crate::map::view::update_view(&mut s);
    }
    crate::gfx::view::refresh_view(&mut state.borrow_mut(), io);
}
