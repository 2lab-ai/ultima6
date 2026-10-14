//! The command loop 0x1041d8: one iteration = panel redraw (0x103f64), command key (0x10790a),
//! letter → command code with the cursor set up, the handler, end of turn (0x1213de, 0x10348c,
//! the prompt).
//!
//! Handlers: walk (0x80), pass (space, Esc, 0x8f), party mode `0`, solo mode `1`..`9`, Talk
//! (0x83 → 0x1127c4), the control codes (0x05 / 0x11 / 0x12d exit, 0x08 help, 0x16 version, 0x1a
//! sound), and since milestone 2 Attack (0x81 → combat 0x124caa), Begin / break off combat
//! (`B` → combat 0x121c64 / 0x121d78), Look / Get / Drop / Move (0x84..0x87 → objects 0x1298b8,
//! 0x12a7a4, 0x12a226, 0x12ae04), Use (0x88 → 0x1305e0), Save (Ctrl-S → 0x105716), Restore
//! (Ctrl-R → 0x1052c8) and the coordinate dump's overview (0xd5 → 0x13410a). The cut handlers
//! (Cast, Rest, the debug teleport) are stub points: the loop prints what the original prints
//! before calling them (the command's name), then the port's own line [`NOT_IMPLEMENTED`],
//! clears the command state (`end_command`) and returns [`NO_TURN`]: no movement points spent,
//! no world turn, the prompt again.

use std::cell::RefCell;

use super::{flush_prints, game_string, leader_name, pointed_string, say};
use crate::converse::Cx;
use crate::engine::Io;
use crate::image::A4;
use crate::objects::ObjectId;
use crate::state::State;
use crate::todo_stub;
use crate::ui::globals::NO_COMMAND;
use crate::ui::printf::{Arg, printf};

/// Image strings the loop pushes (PC-relative in hunk 1).
const FMT_PROMPT: u32 = 0x0010_1f3c;
const FMT_KEY_ECHO: u32 = 0x0010_1f44;
const FMT_NAME_DASH: u32 = 0x0010_1f48;
const MSG_RESTORE: u32 = 0x0010_1f4c;
const MSG_HELP: u32 = 0x0010_1f5c;
const MSG_ON: u32 = 0x0010_1f62;
const MSG_OFF: u32 = 0x0010_1f66;
const MSG_BANG: u32 = 0x0010_1f6a;
const FMT_COORDS: u32 = 0x0010_1f6e;
const FMT_REPROMPT: u32 = 0x0010_1fc0;
/// "Rest", the first line 0x127b1a prints (hunk 14).
const MSG_REST: u32 = 0x0012_72cc;
/// Data-hunk strings, as A4 - offset.
const MSG_WHAT: u32 = 0x7694;
const MSG_PARTY_MODE: u32 = 0x768d;
const MSG_PASS: u32 = 0x7641;
const MSG_ABOARD: u32 = 0x744d;
const MSG_EXIT: u32 = 0x765f;
const MSG_SAVE: u32 = 0x764d;
const MSG_SOLO: u32 = 0x766a;
const MSG_FAILS: u32 = 0x7681;
const MSG_NOT_ALL_HERE: u32 = 0x72aa;
const MSG_NOT_IN_COMBAT: u32 = 0x732d;
const MSG_NOT_SOLO: u32 = 0x7340;
const MSG_BEGIN_COMBAT: u32 = 0x7534;
const MSG_BREAK_COMBAT: u32 = 0x7525;
const MSG_VERSION: u32 = 0x723c;
const MSG_SOUND_ON: u32 = 0x727b;
const MSG_SOUND_OFF: u32 = 0x7271;
/// Pointer table of the command names, 4 bytes per command code (A4-0x7c8e + 4 code).
const COMMAND_NAMES: u32 = 0x7c8e;

/// Command codes.
const WALK: u16 = 0x80;
const ATTACK: u16 = 0x81;
const CAST: u16 = 0x82;
const TALK: u16 = 0x83;
const REST: u16 = 0x89;
const COMBAT: u16 = 0x8a;
const SAVE: u16 = 0xaa;
const EXIT: u16 = 0x12d;
/// The key read gives this when a cursor confirmed its target.
const TARGET_DONE: u16 = crate::ui::wait::REPEAT;
/// Port-only: what a cut handler returns instead of its code. No key or command code is 0xffff;
/// the loop reprints the prompt and runs no end of turn (no world turn, no volcano check).
pub(crate) const NO_TURN: u16 = 0xffff;
/// Port-only text (not the game's) a cut handler prints instead of acting.
pub(crate) const NOT_IMPLEMENTED: &[u8] = b"Not implemented in this version - nothing happened.\n";

/// Objlist block bytes (A4-0x7b84 + n) the loop reads.
const TAIL_COMBAT: usize = 0x78;
pub(crate) const TAIL_SOLO: usize = 0x79;
const TAIL_SOLO_WORK: usize = 0x7a;

fn tail(s: &State, i: usize) -> u8 {
    s.actors.objlist_tail.get(i).copied().unwrap_or(0)
}

/// Replaces: 0x1051a6 (set the pointer sprite by the pointer position: the port has no pointer)
fn pointer_shape() {}

/// `printf` of the image string at `addr`.
async fn print_at(state: &RefCell<State>, io: &Io, addr: u32, args: &[Arg<'_>]) {
    let fmt = game_string(io, addr);
    printf(state, io, &fmt, args).await;
}

/// "Name-": the command's name from the table A4-0x7c8e, then a dash.
async fn print_command(state: &RefCell<State>, io: &Io, code: u16) {
    let name = pointed_string(io, A4.wrapping_sub(COMMAND_NAMES) + 4 * u32::from(code));
    print_at(state, io, FMT_NAME_DASH, &[Arg::Str(&name)]).await;
}

/// Original: 0x1041d8 (command loop + A-Z key dispatcher, end-of-turn)
///
/// The prompt (`%s:` newline `%>` with the leader's name), then per command: the side panel
/// (0x103f64), A4-0x7a2c = 1 while no target is pending, the key (0x10790a), the letter mapped
/// to its code ([`map_letter`]), the handler ([`dispatch`]); unless the code is 0x12d the end of
/// turn ([`end_of_turn`]). Code 0x12d (exit confirmed) ends the loop.
pub async fn command_loop(state: &RefCell<State>, io: &Io) {
    let name = leader_name(&state.borrow());
    print_at(state, io, FMT_PROMPT, &[Arg::Str(&name)]).await;
    loop {
        crate::ui::panels::redraw_panel(state, io).await;
        {
            let mut s = state.borrow_mut();
            if s.ui.input.target_pending == 0 {
                s.main.reading = 1;
            }
        }
        let key = crate::ui::wait::read_key(state, io).await;
        state.borrow_mut().main.reading = 0;
        let code = map_letter(state, io, key).await;
        let code = dispatch(state, io, code).await;
        // The frames and lines the synchronous combat and object code queued for this command
        // (none for most), then any lines still queued.
        crate::combat::play_effects(state, io).await;
        match code {
            EXIT => return,
            NO_TURN => reprompt(state, io).await,
            _ => end_of_turn(state, io, code).await,
        }
    }
}

/// The cross-hair a targeted command starts with: cell (5, 5), `range` (-1 = a direction picks
/// at once), cursor mode `mode`, not started by Tab; the panel cursor is restored first
/// (0x106932(0)).
fn start_cursor(s: &mut State, io: &Io, mode: u8, range: i8) {
    crate::ui::cursor::panel_cursor_in(s, io, 0);
    let input = &mut s.ui.input;
    input.cursor_by_tab = false;
    input.targeting = mode;
    input.cursor = (5, 5);
    input.range = range;
}

/// 0x104216..0x104496: command letters to codes, with the cursor each starts. Drop's inventory
/// panel is redrawn (0x103f64) before its panel cursor shows.
pub(crate) async fn map_letter(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    let (code, redraw) = letter_code(&mut state.borrow_mut(), io, key);
    if redraw {
        crate::ui::panels::redraw_panel(state, io).await;
    }
    if code == 0x86 {
        crate::ui::cursor::panel_cursor_in(&mut state.borrow_mut(), io, 1);
    }
    code
}

/// The synchronous part of [`map_letter`]: the code, and whether Drop asks for the inventory
/// panel of the leader first.
fn letter_code(s: &mut State, io: &Io, key: u16) -> (u16, bool) {
    let code = match key {
        0x13 => SAVE,
        0x41 => {
            start_cursor(s, io, 1, 7);
            let target = s.main.attack_target;
            if target >= 0 {
                let p = s
                    .objects
                    .location(ObjectId(target as u16))
                    .unwrap_or_default();
                let c = s.map.centre;
                let col = (p.x as i8).wrapping_sub(c.x as i8).wrapping_add(5);
                let row = (p.y as i8).wrapping_sub(c.y as i8).wrapping_add(5);
                let inside = (0..=10).contains(&col) && (0..=10).contains(&row);
                s.ui.input.cursor = if inside { (col, row) } else { (5, 5) };
            }
            ATTACK
        }
        0x42 => COMBAT,
        0x43 => {
            start_cursor(s, io, 3, 7);
            CAST
        }
        0x44 => {
            start_cursor(s, io, 2, 7);
            s.ui.input.panel_cursor = (3, 0);
            let (leader, size) = (s.actors.party.leader_index, s.actors.party.size);
            let p = &mut s.ui.panel;
            let redraw = leader != size && (p.mode != 0x92 || p.member != i16::from(leader));
            if redraw {
                p.mode = 0x92;
                p.member = i16::from(leader);
                p.dirty = true;
            }
            return (0x86, redraw);
        }
        0x47 => {
            start_cursor(s, io, 1, -1);
            0x85
        }
        0x4c => {
            start_cursor(s, io, 1, 7);
            0x84
        }
        0x4d => {
            start_cursor(s, io, 1, -1);
            0x87
        }
        0x52 => {
            start_cursor(s, io, 1, 7);
            REST
        }
        0x54 => {
            start_cursor(s, io, 1, 7);
            TALK
        }
        0x55 => {
            start_cursor(s, io, 1, -1);
            0x88
        }
        other => other,
    };
    (code, false)
}

/// The cleanup after a command (0x104d7c and its copies): no target pending, the panel cursor
/// restored, cursor mode off, no command waiting, the pointer sprite.
fn end_command(s: &mut State, io: &Io) {
    s.ui.input.target_pending = 0;
    crate::ui::cursor::panel_cursor_in(s, io, 0);
    s.ui.input.targeting = 0;
    s.ui.input.target_command = NO_COMMAND;
    pointer_shape();
}

/// Port-only: a cut handler's ending. [`NOT_IMPLEMENTED`] (on a new line after `after_name`, the
/// "Name-" or "Rest" the original printed first), the command state cleared; the caller returns
/// [`NO_TURN`].
async fn not_implemented(state: &RefCell<State>, io: &Io, after_name: bool) {
    if after_name {
        printf(state, io, b"\n", &[]).await;
    }
    printf(state, io, NOT_IMPLEMENTED, &[]).await;
    end_command(&mut state.borrow_mut(), io);
}

/// Reads keys until `Y` or `N` and echoes it (`%c` newline); true for `Y`.
async fn yes_no(state: &RefCell<State>, io: &Io) -> bool {
    let key = loop {
        let k = crate::ui::wait::read_key(state, io).await;
        if k == 0x59 || k == 0x4e {
            break k;
        }
    };
    print_at(state, io, FMT_KEY_ECHO, &[Arg::Word(key)]).await;
    key == 0x59
}

/// 0x104496..0x104d96: the handler of `code`; returns the code the end of turn sees (an exit
/// prompt answered `N` gives 0, `Y` 0x12d), or [`NO_TURN`] after a cut handler.
pub(crate) async fn dispatch(state: &RefCell<State>, io: &Io, code: u16) -> u16 {
    let size = u16::from(state.borrow().actors.party.size);
    if (0x30..=0x39).contains(&code) && code < size + 0x31 {
        if code == 0x30 {
            party_mode(state, io).await;
        } else {
            solo_mode(state, io, code).await;
        }
        return code;
    }
    match code {
        0x05 | 0x11 | EXIT => {
            say(state, io, MSG_EXIT, &[]).await;
            return if yes_no(state, io).await { EXIT } else { 0 };
        }
        0x08 => {
            print_at(state, io, MSG_HELP, &[]).await;
            // A4-0x7a50: the help mode is the keyword colour of the text layer.
            let on = {
                let mut s = state.borrow_mut();
                let t = &mut s.ui.text;
                t.keyword_colour = !t.keyword_colour;
                t.keyword_colour
            };
            print_at(state, io, if on { MSG_ON } else { MSG_OFF }, &[]).await;
            print_at(state, io, MSG_BANG, &[]).await;
        }
        0x12 => {
            print_at(state, io, MSG_RESTORE, &[]).await;
            if yes_no(state, io).await {
                crate::saveload::restore_game(state, io).await;
            }
        }
        0x16 => say(state, io, MSG_VERSION, &[]).await,
        0x1a => {
            let on = state.borrow().main.sound == 0;
            if on {
                state.borrow_mut().main.sound = 1;
                // Contract: start the area song. M1 has no sound: the flag only.
                todo_stub!("0x110912 pick and play the area song", ());
                say(state, io, MSG_SOUND_ON, &[]).await;
            } else {
                todo_stub!("0x110878 play song 0 (music off)", ());
                say(state, io, MSG_SOUND_OFF, &[]).await;
                state.borrow_mut().main.sound = 0;
            }
        }
        0x1b | 0x20 | NO_COMMAND => pass(state, io).await,
        WALK => {
            let dir = state.borrow().ui.input.direction;
            crate::actors::party::player_step(&mut state.borrow_mut(), dir as u16);
        }
        ATTACK => {
            // 0x124caa prints "Attack" itself, reads the target and spends the movement points.
            crate::combat::attack::attack(state, io).await;
            state.borrow_mut().ui.input.target_command = crate::combat::attack::AFTER_ATTACK;
        }
        CAST..=0x88 => return targeted(state, io, code).await,
        REST => {
            let aboard = {
                let s = state.borrow();
                s.actors.party.leader_index == s.actors.party.size
            };
            if !aboard {
                todo_stub!("0x110878 play song 0 (music off)", ());
            }
            print_at(state, io, MSG_REST, &[]).await;
            // Contract: the Rest command (0x127b1a: hours, guard, heal). Fallback: no rest, no
            // turn.
            todo_stub!("0x127b1a Rest command", ());
            if !aboard {
                todo_stub!("0x110912 pick and play the area song", ());
            }
            not_implemented(state, io, true).await;
            return NO_TURN;
        }
        COMBAT => return combat_toggle(state, io).await,
        SAVE => {
            if state.borrow().map.flag_7afb == 0 {
                say(state, io, MSG_SAVE, &[]).await;
                if yes_no(state, io).await {
                    crate::saveload::save_game(state, io).await;
                    end_command(&mut state.borrow_mut(), io);
                }
            }
        }
        0xd5 => {
            let args = {
                let s = state.borrow();
                let (cal, c) = (s.actors.calendar, s.map.centre);
                [cal.hour, cal.day, cal.minute]
                    .map(u16::from)
                    .to_vec()
                    .into_iter()
                    .chain([c.x, c.y, u16::from(c.z)])
                    .collect::<Vec<_>>()
            };
            let args: Vec<Arg<'_>> = args.into_iter().map(Arg::Word).collect();
            print_at(state, io, FMT_COORDS, &args).await;
            // 0x13410a: the map overview, shown until a key.
            crate::combat::peer::overview(state, io).await;
        }
        0xd6 => {
            // Contract: the debug teleport (number prompts 0x104140, then 0x1092fc). Fallback:
            // no teleport, no turn.
            todo_stub!("0x104140 debug teleport prompts", ());
            not_implemented(state, io, false).await;
            return NO_TURN;
        }
        0xd7 => {
            let mut s = state.borrow_mut();
            crate::actors::clock::tick(&mut s, 0);
            s.actors.work.force_teleport = 1;
            crate::actors::turn::go_to_schedules(&mut s);
            s.actors.work.force_teleport = 0;
        }
        0x119 => super::debug_hook(),
        _ => {
            say(state, io, MSG_WHAT, &[]).await;
            end_command(&mut state.borrow_mut(), io);
        }
    }
    code
}

/// Pass (0x104b62): "Pass!", the leader's movement points 0, the cursor off; a ship (0x19f) or
/// balloon (0x1a7) leader drifts one step with the wind (A4-0x7b7b ≥ 0); the party follows
/// (0x11c910 with D1 = 1).
async fn pass(state: &RefCell<State>, io: &Io) {
    say(state, io, MSG_PASS, &[]).await;
    let mut s = state.borrow_mut();
    let leader =
        usize::from(s.actors.party.members[usize::from(s.actors.party.leader_index).min(15)]);
    s.actors.movement_points[leader] = 0;
    crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
    s.ui.input.target_pending = 0;
    s.ui.input.targeting = 0;
    pointer_shape();
    let ty = s.objects.obj_type(ObjectId(leader as u16));
    let wind = s.actors.wind as i8;
    if (ty == 0x19f || ty == 0x1a7) && wind >= 0 {
        crate::actors::party::player_step(&mut s, wind as u16);
    }
    crate::actors::party::follow(&mut s, leader as u16, true);
}

/// `0` (0x1045ae..0x1046bc): party mode, when not aboard and out of combat, if every member is
/// within 8 tiles of the leader on both axes ("Not everyone is here." otherwise).
async fn party_mode(state: &RefCell<State>, io: &Io) {
    let (aboard, combat, all_here) = {
        let s = state.borrow();
        let p = &s.actors.party;
        let c = s.map.centre;
        let near = |m: &u8| {
            let at = s
                .objects
                .location(ObjectId(u16::from(*m)))
                .unwrap_or_default();
            (c.x as i16 - at.x as i16).abs() <= 8 && (c.y as i16 - at.y as i16).abs() <= 8
        };
        (
            p.leader_index == p.size,
            tail(&s, TAIL_COMBAT) != 0,
            p.members.iter().take(usize::from(p.size)).all(near),
        )
    };
    if aboard {
        say(state, io, MSG_ABOARD, &[]).await;
    } else if combat {
        say(state, io, MSG_NOT_IN_COMBAT, &[]).await;
    } else if !all_here {
        say(state, io, MSG_NOT_ALL_HERE, &[]).await;
    } else {
        say(state, io, MSG_PARTY_MODE, &[]).await;
        let mut s = state.borrow_mut();
        super::end_solo(&mut s, io);
        s.ui.input.target_pending = 0;
        s.ui.input.targeting = 0;
        s.ui.input.target_command = NO_COMMAND;
        pointer_shape();
    }
}

/// `1`..`9` (0x1046bc..0x104866): solo mode for member `code - '1'`, when it can act (actor status
/// without 0x16, movement flags without 0x10, object status without 0x04), not aboard, out of
/// combat. The old solo member (or leader) gets its saved worktype back, the new one's is saved,
/// followers (worktype 1) stop (0), the member leads (0x103fd4) with worktype 2.
async fn solo_mode(state: &RefCell<State>, io: &Io, code: u16) {
    let i = usize::from(code - 0x31);
    let (able, aboard, combat, name) = {
        let s = state.borrow();
        let m = usize::from(s.actors.party.members[i]);
        let able = s.actors.attrs.status[m] & 0x16 == 0
            && s.actors.attrs.movement_flags[m] & 0x10 == 0
            && s.objects.obj_status[m] & 0x04 == 0;
        let p = &s.actors.party;
        let name = p
            .names
            .get(i)
            .map_or_else(Vec::new, |n| n.as_bytes().to_vec());
        (
            able,
            p.leader_index == p.size,
            tail(&s, TAIL_COMBAT) != 0,
            name,
        )
    };
    if !able {
        say(state, io, MSG_FAILS, &[Arg::Str(&name)]).await;
    } else if aboard {
        say(state, io, MSG_ABOARD, &[]).await;
    } else if combat {
        say(state, io, MSG_NOT_IN_COMBAT, &[]).await;
    } else {
        {
            let mut s = state.borrow_mut();
            let members = s.actors.party.members;
            let solo = tail(&s, TAIL_SOLO) as i8;
            let saved = tail(&s, TAIL_SOLO_WORK);
            let back = if solo < 0 {
                members[usize::from(s.actors.party.leader_index).min(15)]
            } else {
                members[(solo as usize).min(15)]
            };
            s.actors.worktype[usize::from(back)] = saved;
            let w = s.actors.worktype[usize::from(members[i])];
            s.actors.objlist_tail[TAIL_SOLO_WORK] = w;
            for &m in members.iter().take(usize::from(s.actors.party.size)) {
                if s.actors.worktype[usize::from(m)] == 1 {
                    s.actors.worktype[usize::from(m)] = 0;
                }
            }
            super::switch_leader(&mut s, i as u8);
            let leader = members[usize::from(s.actors.party.leader_index).min(15)];
            s.actors.worktype[usize::from(leader)] = 2;
            let li = s.actors.party.leader_index;
            s.actors.objlist_tail[TAIL_SOLO] = li;
            if s.ui.panel.mode != 0x91 {
                s.ui.panel.dirty = true;
            }
        }
        say(state, io, MSG_SOLO, &[]).await;
        end_command(&mut state.borrow_mut(), io);
    }
}

/// Cast, Talk, Look, Get, Drop, Move, Use (0x10498e..0x104a96): Cast, Get, Drop and Move are
/// refused aboard ship; then the command waits for its target (A4-0x7a32 = 1, A4-0x7a2e = code),
/// "Name-" is printed and, except for Cast (0x11af10, which reads its own input), the next key
/// read must confirm a target (0x8e) or the loop says "what?"; a confirmed target goes to the
/// command's handler.
async fn targeted(state: &RefCell<State>, io: &Io, code: u16) -> u16 {
    if matches!(code, CAST | 0x85..=0x87) {
        let aboard = {
            let s = state.borrow();
            s.actors.party.leader_index == s.actors.party.size
        };
        if aboard {
            print_command(state, io, code).await;
            say(state, io, MSG_ABOARD, &[]).await;
            return code;
        }
    }
    {
        let mut s = state.borrow_mut();
        s.ui.input.target_pending = 1;
        s.ui.input.target_command = code;
        pointer_shape();
    }
    print_command(state, io, code).await;
    if code == CAST {
        // 0x11af10 reads its own input (the book, the syllables, the target).
        crate::magic::cast(state, io).await;
    } else if crate::ui::wait::read_key(state, io).await == TARGET_DONE {
        let command = state.borrow().ui.input.target_command;
        match command {
            TALK => {
                let (talker, target) = {
                    let s = state.borrow();
                    (s.actors.party.leader_index, s.ui.input.selection)
                };
                crate::converse::talk(&Cx::new(state, io), talker, target, true).await;
                todo_stub!("0x110912 pick and play the area song", ());
            }
            0x84 => crate::objects::commands::look(state, io).await,
            0x85 => crate::objects::commands::get(state, io).await,
            0x86 => crate::objects::commands::drop_object(state, io).await,
            0x87 => crate::objects::commands::push(state, io).await,
            0x88 => crate::usecmd::use_command(state, io).await,
            _ => {}
        }
    } else {
        say(state, io, MSG_WHAT, &[]).await;
    }
    end_command(&mut state.borrow_mut(), io);
    code
}

/// `B` (0x104b98..0x104c10): begin or break off combat, in party mode only: the line, then
/// combat mode on (0x121c64) or off (0x121d78).
async fn combat_toggle(state: &RefCell<State>, io: &Io) -> u16 {
    let (solo, combat, aboard) = {
        let s = state.borrow();
        let p = &s.actors.party;
        (
            tail(&s, TAIL_SOLO) as i8,
            tail(&s, TAIL_COMBAT) != 0,
            p.leader_index == p.size,
        )
    };
    if solo >= 0 {
        say(state, io, MSG_NOT_SOLO, &[]).await;
        return COMBAT;
    }
    if combat {
        say(state, io, MSG_BREAK_COMBAT, &[]).await;
        crate::combat::mode::break_off(&mut state.borrow_mut());
    } else if aboard {
        say(state, io, MSG_ABOARD, &[]).await;
    } else {
        say(state, io, MSG_BEGIN_COMBAT, &[]).await;
        crate::combat::mode::begin_combat(&mut state.borrow_mut());
    }
    COMBAT
}

/// 0x104d98..0x104df2: a pending save or restore (A4-0x637a 'R' / 'G'), the world turn
/// (0x1213de), the volcano check (0x10348c); after any command but a step the prompt is
/// reprinted (A4-0x7afd), which also prints it when a routine asked for it.
async fn end_of_turn(state: &RefCell<State>, io: &Io, code: u16) {
    let mode = state.borrow().objects.save_mode;
    if mode == 0x52 {
        crate::saveload::restore_game(state, io).await;
    } else if mode == 0x47 {
        crate::saveload::save_game(state, io).await;
    }
    crate::actors::turn::world_turn(state, io).await;
    flush_prints(state, io).await;
    super::end_of_turn_volcano(&mut state.borrow_mut());
    flush_prints(state, io).await;
    if code != WALK {
        state.borrow_mut().actors.work.prompt = 1;
    }
    if state.borrow().actors.work.prompt != 0 {
        reprompt(state, io).await;
    }
}

/// The prompt again (`\n%s:` newline `%>`, A4-0x7afd back to 0): after any command but a step,
/// and after a cut handler.
async fn reprompt(state: &RefCell<State>, io: &Io) {
    let name = leader_name(&state.borrow());
    print_at(state, io, FMT_REPROMPT, &[Arg::Str(&name)]).await;
    state.borrow_mut().actors.work.prompt = 0;
}
