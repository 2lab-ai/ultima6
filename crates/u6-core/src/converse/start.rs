//! Talk 0x1127c4, the conversation set-up 0x1125b8 and the conversation loop 0x114afa.

use super::eval::stricmp;
use super::party::{actor_name, count_present, ix, obj_type, on_screen, status};
use super::run::run_text;
use super::script::{ScriptDst, load_script};
use super::seams;
use super::vars::{self, SLOT_GENDER, SLOT_NPC_NAME, SLOT_PLAYER, SLOT_TIME, SLOT_Z, StrRef};
use super::{ConverseText, Cx, INPUT_MAX, Msg, STRING_BUFFERS};
use crate::speech::{self, Kind};
use crate::state::State;

/// Shrine and statue object types: they talk as the NPC named by their object word.
const SHRINE: u16 = 0x189;
const STATUES: [u16; 3] = [0x18d, 0x18e, 0x18f];
/// Horse types whose rider talks (0x1127c4).
const HORSES: [u16; 2] = [0x1ae, 0x1af];
/// Object types that may talk from slots ≥ 0xe0 (0x114afa; 0x133e5e knows the first three).
const TALKING_OBJECTS: [u16; 4] = [0x17e, 0x175, 0x16b, 0x16a];
/// Worktypes that never answer (0x114afa).
const SILENT_WORKTYPES: [u8; 4] = [0x9b, 0x13, 0x07, 0x12];
/// Word variable mirrored from the actors' quest-flag copy A4-0x2fd2 (0x105350 writes it at
/// load; scripts read and write it here).
const VAR_QUEST: usize = 0x1a;

fn byte(v: &[u8], a: i16) -> u8 {
    ix(a).and_then(|i| v.get(i)).copied().unwrap_or(0)
}

/// Original: 0x1127c4 (Talk command: target, shrine/statue cases, start talk)
///
/// `talker` = the party index that talks (the main loop passes A4-0x7b7a), `target` = the object
/// picked. A horse's rider talks instead of the horse. Anything that is neither an actor slot
/// (0..256) nor a shrine/statue prints the line 0x1127ba. With `print_name` the target's name
/// follows the prompt: the word 0x1127aa (shrine) or 0x1127b2 (statue), the actor's name once met
/// (talk flag bit 0), else its display name. Then 0x1125b8.
pub async fn talk(cx: &Cx<'_>, talker: u8, target: i16, print_name: bool) {
    let ty = cx.st(|s| obj_type(s, target));
    let target = if HORSES.contains(&ty) {
        cx.st(|s| seams::rider(s, target))
    } else {
        target
    };
    let special = ty == SHRINE || STATUES.contains(&ty);
    if !(0..0x100).contains(&target) && !special {
        load_text(cx);
        let m = cx.msg(Msg::Nothing);
        seams::print(cx, &m).await;
        return;
    }
    if print_name {
        load_text(cx);
        let name = if ty == SHRINE {
            cx.msg(Msg::Shrine)
        } else if STATUES.contains(&ty) {
            cx.msg(Msg::Statue)
        } else if cx.st(|s| byte(&s.actors.attrs.talk_flags, target)) & 1 != 0 {
            let r = actor_name(cx, target).await;
            cx.st(|s| vars::resolve(s, r))
        } else {
            cx.st(|s| seams::object_name(s, target))
        };
        seams::print(cx, &name).await;
    }
    begin(cx, talker, target).await;
}

/// Reads the message table out of `game` on first use (a missing or unknown `game` leaves it
/// empty: the conversation then runs without the engine's own messages).
fn load_text(cx: &Cx<'_>) {
    if cx.cv(|c| c.text.is_none()) {
        let text = cx
            .io
            .files()
            .get("game")
            .and_then(|g| ConverseText::from_game(g).ok());
        cx.cv(|c| c.text = Some(text.unwrap_or_default()));
    }
}

/// Original: 0x1125b8 (conversation set-up: variables, then 0x114afa)
///
/// String slots: `$G` (0x10) the string at 0x112758 or 0x112760 by the hero's gender, `$N`
/// (0x17) the NPC-name buffer, `$P` (0x19) the talker's party name, `$T` (0x1d) the string at
/// 0x112790 (hour < 12), 0x112798 (< 18) or 0x1127a2, `$0`..`$9` the ten buffers. Word variables: 10 dexterity, 13 day,
/// 14 experience, 16 female, 17 hour, 18 intelligence, 20 karma, 21 A4-0x7b26, 22 month,
/// 24 party size − 1, 25 hit points, 28 strength (of the talker), 32 the target's worktype,
/// 34 year; 0..9 cleared. Music stops around the conversation; afterwards variable 21 is
/// written back to A4-0x7b26.
pub async fn begin(cx: &Cx<'_>, talker: u8, target: i16) {
    load_text(cx);
    let member = cx.st(|s| {
        set_vars(s, talker, target);
        seams::mark_scroll_talking(s);
        seams::play_song(s, 0);
        s.actors
            .party
            .members
            .get(usize::from(talker))
            .copied()
            .unwrap_or(0)
    });
    converse(cx, i16::from(member), target).await;
    cx.st(|s| {
        seams::pick_song(s);
        seams::mark_7a52(s);
        let v = vars::int_var(s, 0x15);
        seams::set_word_7b26(s, v);
        s.actors.quest_flag_copy = vars::int_var(s, VAR_QUEST as u8);
    });
}

fn set_vars(s: &mut State, talker: u8, target: i16) {
    let female = seams::female(s);
    let gender = if female { Msg::Milady } else { Msg::Milord };
    vars::set_slot(s, SLOT_GENDER, StrRef::Text(gender));
    vars::set_slot(s, SLOT_NPC_NAME, StrRef::NpcName);
    vars::set_slot(s, SLOT_PLAYER, StrRef::PartyName(talker));
    let hour = s.actors.calendar.hour;
    let time = match hour {
        0..12 => Msg::Morning,
        12..18 => Msg::Afternoon,
        _ => Msg::Evening,
    };
    vars::set_slot(s, SLOT_TIME, StrRef::Text(time));
    let m = i16::from(
        s.actors
            .party
            .members
            .get(usize::from(talker))
            .copied()
            .unwrap_or(0),
    );
    let exp = ix(m)
        .and_then(|i| s.actors.attrs.exp.get(i))
        .copied()
        .unwrap_or(0);
    let c = s.actors.calendar;
    let words: [(usize, u16); 15] = [
        (10, seams::dexterity(s, m)),
        (13, u16::from(c.day)),
        (14, exp),
        (16, u16::from(female)),
        (17, u16::from(hour)),
        (18, seams::intelligence(s, m)),
        (20, u16::from(s.actors.karma)),
        (21, seams::word_7b26(s)),
        (22, u16::from(c.month)),
        (24, u16::from(s.actors.party.size).wrapping_sub(1)),
        (25, u16::from(byte(&s.actors.attrs.hp, m))),
        (28, seams::strength(s, m)),
        (32, u16::from(byte(&s.actors.worktype, target))),
        (34, c.year),
        (VAR_QUEST, s.actors.quest_flag_copy),
    ];
    for (i, v) in words {
        vars::set_int(s, i, v);
    }
    for i in 0..STRING_BUFFERS {
        vars::set_slot(s, i, StrRef::Buffer(i as u8));
        vars::set_int(s, i, 0);
    }
}

/// Original: 0x114afa (start conversation: checks, load script, greeting, input loop)
///
/// `talker` = the actor who talks, `target` = the object talked to. Refusals, in order: a party
/// member off screen (line 0x112a0c), solo mode for another member than the first (0x112a1e),
/// a dead NPC 0 / ≥ 0xe0 reached by spell (0x112a32), an actor that cannot answer (dead without
/// the spell, asleep / paralysed (status bits 2, 1), silent worktypes, alignment 0x20 / 0x60:
/// 0x112a4a), A4-0x7afb set (0x112a5a), talking to oneself (0x112a68), and no script
/// (0x112a80). Then: NPC name from the script, the portrait panel and the line 0x112a96 + look
/// text (not by spell), the greeting after `f2`, and the input loop until `b6` (bye) or a
/// malformed / ending op. After each input the look word (0x112afa) replays the look text;
/// anything else runs the text with its keyword blocks.
pub async fn converse(cx: &Cx<'_>, talker: i16, target: i16) {
    load_text(cx);
    let refusal = cx.st(|s| refusal(s, talker, target));
    if let Some(m) = refusal {
        let m = cx.msg(m);
        seams::print(cx, &m).await;
        return;
    }
    let npc = cx.st(|s| {
        seams::face_target(s);
        s.converse.npc
    });
    let ty = cx.st(|s| obj_type(s, target));
    let loaded = (npc != 0 && (npc < 0xe0 || TALKING_OBJECTS.contains(&ty)))
        && cx.st(|s| {
            let files = cx.io.files();
            load_script(s, files, npc, ScriptDst::Main) != 0
        });
    if !loaded {
        let m = cx.msg(Msg::FunnyNoResponse);
        seams::print(cx, &m).await;
        return;
    }
    let (mode, member) = cx.st(|s| {
        count_present(s);
        let c = &mut s.converse;
        c.pc = 0;
        c.fault = false;
        let name: Vec<u8> = c
            .script
            .iter()
            .skip(2)
            .take_while(|&&b| b != 0xf1)
            .copied()
            .collect();
        c.npc_name = name;
        c.active = true;
        seams::panel(s)
    });
    if !cx.cv(|c| c.by_spell) {
        if ty != SHRINE {
            seams::portrait_panel(cx, target).await;
        }
        let m = cx.msg(Msg::YouSeeFirst);
        seams::print(cx, &m).await;
        cx.cv(|c| {
            while c.next() != 0xf1 && !c.fault {}
            c.look_pc = c.pos();
        });
        run_text(cx, npc, Kind::Look).await;
    }
    let m = cx.msg(Msg::Newline);
    seams::print(cx, &m).await;
    let greet = cx.cv(|c| {
        while c.next() != 0xf2 && !c.fault {}
        let ask = c.next() == 0xf7;
        c.advance(-1);
        !ask
    });
    if greet {
        run_text(cx, npc, Kind::Speech).await;
    }
    input_loop(cx, npc).await;
    cx.st(|s| {
        speech::end(s);
        s.converse.active = false;
        s.converse.bye = false;
        let (_, now) = seams::panel(s);
        if now == u16::from(s.actors.party.size) {
            seams::reset_panel(s);
        } else {
            seams::set_panel(s, mode, member);
        }
        seams::bump_7afe(s);
    });
}

/// The refusal message of 0x114afa's checks, or `None`; sets the talker / NPC words.
fn refusal(s: &mut State, talker: i16, target: i16) -> Option<Msg> {
    s.converse.talker = talker;
    s.converse.npc = target;
    if status(s, target) & 0x80 != 0 && !on_screen(s, target) {
        return Some(Msg::NotOnScreen);
    }
    let solo = seams::solo_member(s);
    if solo != -1 && solo != 0 {
        return Some(Msg::NotInSolo);
    }
    let ty = obj_type(s, target);
    if ty == SHRINE || STATUES.contains(&ty) {
        s.converse.npc = i16::from((seams::object_word_250c(s, target) >> 8) as u8);
    }
    let npc = s.converse.npc;
    let st = status(s, npc);
    let by_spell = s.converse.by_spell;
    if (npc == 0 || npc >= 0xe0) && st & 0x10 != 0 && by_spell {
        return Some(Msg::DeepMoan);
    }
    if target == npc {
        let wt = byte(&s.actors.worktype, npc);
        let silent = (st & 0x10 != 0 && !by_spell)
            || st & 0x06 != 0
            || SILENT_WORKTYPES.contains(&wt)
            || st & 0x60 == 0x20
            || st & 0x60 == 0x60;
        if silent {
            return Some(Msg::NoResponseNpc);
        }
    }
    if seams::talk_refused(s) {
        return Some(Msg::NoResponse);
    }
    if s.converse.talker == s.converse.npc {
        return Some(Msg::TalkingToYourself);
    }
    None
}

/// 0x114afa's input loop: one input op, then "look" or the text that follows it.
///
/// Port (spec 09 Design 1): an op that waits for the player first flushes what was said to the
/// speech tap (the speaker is the NPC again after it), and the input it gets starts a new turn.
async fn input_loop(cx: &Cx<'_>, npc: i16) {
    while !cx.cv(|c| c.bye || c.fault) {
        let op = cx.cv(|c| {
            c.saved_pc = c.pos();
            c.next()
        });
        #[cfg(test)]
        super::disasm::trace::input_byte(cx, op);
        let waits = matches!(op, 0xf7..=0xfc | 0xcb);
        if waits {
            cx.st(speech::before_input);
        }
        let ok = match op {
            0xf7 => {
                let m = cx.msg(Msg::YouSay);
                seams::print(cx, &m).await;
                let line = seams::read_line(cx, INPUT_MAX).await;
                cx.cv(|c| c.input = line);
                let m = cx.msg(Msg::Newline);
                seams::print(cx, &m).await;
                true
            }
            0xf9 => {
                let line = seams::read_line(cx, INPUT_MAX).await;
                cx.cv(|c| c.input = line);
                let (var, tag) = cx.cv(|c| (c.next(), c.next()));
                if tag != 0xb3 {
                    break;
                }
                cx.st(|s| {
                    let dst = vars::slot(s, var);
                    let text = s.converse.input.clone();
                    vars::write(s, dst, &text);
                });
                let m = cx.msg(Msg::TwoNewlines);
                seams::print(cx, &m).await;
                true
            }
            0xfb => {
                let line = seams::read_line(cx, INPUT_MAX).await;
                cx.cv(|c| c.input = line.clone());
                let (var, tag) = cx.cv(|c| (c.next(), c.next()));
                if tag != 0xb2 {
                    break;
                }
                let v = match cx.st(|s| seams::parse_number(s, &line)) {
                    -1 | -2 => 0,
                    v => v,
                };
                cx.st(|s| vars::set_int(s, usize::from(var), v as u16));
                let m = cx.msg(Msg::TwoNewlines);
                seams::print(cx, &m).await;
                true
            }
            0xfc => {
                let v = loop {
                    let key = seams::read_key(cx).await;
                    let one = key_input(key);
                    cx.cv(|c| c.input = one.clone());
                    let mut v = cx.st(|s| seams::parse_number(s, &one));
                    if key == 0x0d || key == 0x1b {
                        v = 0;
                    }
                    if v != -1 && v != -2 {
                        break v;
                    }
                };
                let (var, tag) = cx.cv(|c| (c.next(), c.next()));
                if tag != 0xb2 {
                    break;
                }
                cx.st(|s| vars::set_int(s, usize::from(var), v as u16));
                let text = format_one(&cx.msg(Msg::FormatNumber), v.to_string().as_bytes());
                seams::print(cx, &text).await;
                true
            }
            0xfa => {
                let key = seams::read_key(cx).await;
                cx.cv(|c| c.input = key_input(key));
                let text = format_one(&cx.msg(Msg::FormatString), &key_input(key));
                seams::print(cx, &text).await;
                true
            }
            0xcb => {
                seams::read_key(cx).await;
                false
            }
            0xf8 => {
                let allowed = cx.cv(|c| {
                    let mut a = Vec::new();
                    loop {
                        let b = c.next();
                        if b == 0xef || c.fault {
                            break;
                        }
                        a.push(b);
                    }
                    c.advance(-1);
                    c.scratch = a.clone();
                    a
                });
                let up = |b: u8| u16::from(b.to_ascii_uppercase());
                let key = loop {
                    let key = seams::read_key(cx).await;
                    if allowed
                        .iter()
                        .take_while(|&&b| b != 0)
                        .any(|&b| up(b) == key)
                    {
                        break key;
                    }
                };
                if key >= 0x20 {
                    let text = format_one(&cx.msg(Msg::FormatChar), &[key as u8]);
                    seams::print(cx, &text).await;
                }
                cx.cv(|c| c.input = key_input(key));
                let m = cx.msg(Msg::Newline);
                seams::print(cx, &m).await;
                true
            }
            0xee => false,
            0xef => {
                let m = cx.msg(Msg::KeyCommandExpected);
                seams::print(cx, &m).await;
                break;
            }
            0xff => break,
            _ => {
                let m = cx.msg(Msg::LogicalError);
                seams::print(cx, &m).await;
                break;
            }
        };
        if waits {
            cx.st(speech::after_input);
        }
        if ok {
            cx.cv(|c| c.answered = false);
        }
        cx.st(|s| vars::set_slot(s, SLOT_Z, StrRef::Input));
        let look = cx.st(|s| {
            let word = s.converse.text.as_ref().map(|t| t.get(Msg::Look).to_vec());
            stricmp(&s.converse.input, &word.unwrap_or_default()) == 0
        });
        if look {
            let m = cx.msg(Msg::YouSee);
            seams::print(cx, &m).await;
            cx.cv(|c| c.set_pos(c.look_pc));
            run_text(cx, npc, Kind::Look).await;
            cx.cv(|c| c.set_pos(c.saved_pc));
        } else {
            run_text(cx, npc, Kind::Speech).await;
        }
    }
}

/// The one-character input string a key leaves in A4+0x1388 (its low byte, then NUL).
fn key_input(key: u16) -> Vec<u8> {
    let b = key as u8;
    if b == 0 { Vec::new() } else { vec![b] }
}

/// The original's printf with one `%d` / `%s` / `%c` conversion (16-bit ints: the argument is
/// already formatted).
fn format_one(fmt: &[u8], arg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len() + arg.len());
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] == b'%' && i + 1 < fmt.len() && matches!(fmt[i + 1], b'd' | b's' | b'c') {
            out.extend_from_slice(arg);
            i += 2;
        } else {
            out.push(fmt[i]);
            i += 1;
        }
    }
    out
}
