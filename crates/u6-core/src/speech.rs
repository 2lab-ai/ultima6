//! Port-only (spec 09 Design 1): the speech tap. While the page has voices on, the words an NPC
//! says in a conversation are kept apart from the screen, as utterances the platform takes
//! ([`take`]) and speaks. It changes no frame: it only reads what the text layer already puts.
//!
//! - **Kind.** The conversation's call sites of the text runner say what they run
//!   ([`Kind`]): the look text is [`Kind::Look`], the greeting and the answers [`Kind::Speech`].
//! - **Capture.** A flag ([`SpeechTap::capturing`]) is raised only around the three speech
//!   emitters (a text run's bytes, and both printing forms of op `0xb5`), and only for
//!   [`Kind::Speech`] ([`raise`], [`lower`]). The line assembler ([`crate::ui::print`]) hands each
//!   code it takes after the capital step and before the runic bit to [`capture`], which keeps
//!   it while the flag is up and the current window is the message scroll. Look text, engine
//!   messages, the name op `0xbf` prints and the player's echo are printed with the flag down.
//! - **Speaker.** The NPC talked to (`ConverseState::npc`); after op `0xbf`, that op's actor
//!   until the next input op ([`portrait`], [`before_input`]).
//! - **Flush.** What was kept becomes one utterance at each input op, at the `*` wait, the more
//!   prompt and op `0xcb`'s key wait, at `0xbf`, and at the conversation's end ([`flush`]). The
//!   more prompt waits before the line it ends is drawn, so there only what is drawn is let out
//!   ([`flush_drawn`]): the codes of that line stay for the next flush, after the player pages
//!   on. The tap knows them by a mark per cell of the line being assembled ([`mark`]).
//! - **Queue.** Capped like the message transcript ([`QUEUE`]), the oldest dropped.
//!
//! Rights (spec 01): an utterance's text is the user's game text. It leaves the engine only
//! through [`take`]; nothing here logs it, and the `Debug` forms show neither the text nor its
//! length.

use std::collections::VecDeque;
use std::fmt;

use crate::state::State;
use crate::ui::Language;

/// Utterances kept until the platform takes them: as many as the message transcript keeps
/// lines ([`crate::ui::textstate::TRANSCRIPT`]); the oldest is dropped.
pub const QUEUE: usize = crate::ui::textstate::TRANSCRIPT;
/// Characters one utterance holds: a text that runs on without a wait (a script that loops) is
/// cut into utterances of at most this many.
pub const TEXT_MAX: usize = 2048;

/// What a call of the conversation's text runner prints (spec 09 Design 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The look text (after `f1`, and when the player asks to look again): narration.
    Look,
    /// The greeting and the answers: the NPC's words.
    Speech,
}

/// The language of an utterance's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    /// Some of it came while the text layer had the Korean grammar (the Korean pack, spec 08).
    Ko,
}

impl Lang {
    /// The code the synthesizer takes.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ko => "ko",
        }
    }
}

/// Numbers about the speaker, for a later voice profile (spec 09 Design 4): its object type,
/// worktype and alignment (status bits 5..6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traits {
    pub obj_type: u16,
    pub worktype: u8,
    pub alignment: u8,
}

/// One flush of the tap.
#[derive(Clone, PartialEq, Eq)]
pub struct Utterance {
    /// Counts the utterances queued since voices went on, from 0.
    pub seq: u32,
    /// Counts the player's inputs in conversations since voices went on: a newer turn
    /// supersedes the speech of an older one.
    pub turn: u32,
    /// The actor speaking.
    pub speaker: i16,
    /// The conversation's script: (archive letter `a` / `b`, item); `None` if it has none.
    pub script: Option<(u8, u16)>,
    /// The words, as code points (the user's game text: never logged).
    pub text: String,
    pub lang: Lang,
    pub traits: Traits,
}

/// Shows everything but the text (spec 01: no value or length of the user's data).
impl fmt::Debug for Utterance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Utterance")
            .field("seq", &self.seq)
            .field("turn", &self.turn)
            .field("speaker", &self.speaker)
            .field("script", &self.script)
            .field("lang", &self.lang)
            .field("traits", &self.traits)
            .finish_non_exhaustive()
    }
}

/// The tap's state (part of [`State`]; nothing in it is saved or drawn).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SpeechTap {
    /// Voices are on: the page set it ([`set_enabled`]). Off, the tap keeps nothing.
    pub enabled: bool,
    /// Raised around a speech emitter ([`raise`]).
    pub capturing: bool,
    /// What the current utterance holds so far.
    buf: String,
    /// Code points in `buf`.
    chars: usize,
    /// Part of `buf` came with the Korean grammar.
    korean: bool,
    /// The actor op `0xbf` made the speaker, until the next input op.
    speaker: Option<i16>,
    /// Bit `n`: cell `n` of the line being assembled ([`crate::ui::TextState::line`]) holds a
    /// code kept in `buf` (the line holds at most 41 cells).
    cells: u64,
    seq: u32,
    turn: u32,
    queue: VecDeque<Utterance>,
}

/// Shows only the flags (spec 01: no value or length of the user's data).
impl fmt::Debug for SpeechTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpeechTap")
            .field("enabled", &self.enabled)
            .field("capturing", &self.capturing)
            .finish_non_exhaustive()
    }
}

/// Voices on or off. Off drops what is kept and queued; the counters go on from where they
/// were, so a turn after voices come back on is still newer than any before.
pub fn set_enabled(s: &mut State, on: bool) {
    let t = &mut s.speech;
    t.enabled = on;
    if !on {
        t.capturing = false;
        t.buf.clear();
        t.chars = 0;
        t.korean = false;
        t.speaker = None;
        t.cells = 0;
        t.queue.clear();
    }
}

/// The utterances queued since the last call, oldest first.
pub fn take(s: &mut State) -> Vec<Utterance> {
    s.speech.queue.drain(..).collect()
}

/// Raises the capture flag around a speech emitter of `kind`; returns what it was, for
/// [`lower`].
pub(crate) fn raise(s: &mut State, kind: Kind) -> bool {
    let t = &mut s.speech;
    let was = t.capturing;
    t.capturing = t.enabled && kind == Kind::Speech;
    was
}

/// Puts the capture flag back as [`raise`] found it.
pub(crate) fn lower(s: &mut State, was: bool) {
    s.speech.capturing = was;
}

/// The line assembler took code `c` (after the capital step, before the runic bit) into the
/// current window: kept while the flag is up and that window is the message scroll
/// (`messages`). Only printable codes and the newline are kept (not a flush, nor a control
/// glyph). Returns whether it was kept, for the line's cell marks ([`mark`]).
pub(crate) fn capture(s: &mut State, messages: bool, language: Language, c: i32) -> bool {
    let t = &mut s.speech;
    if !(t.enabled && t.capturing && messages) || (c < 0x20 && c != 0x0a) {
        return false;
    }
    let code = u16::try_from(c).unwrap_or(u16::MAX);
    t.buf.push(crate::ui::wide::cell_char(code));
    t.chars += 1;
    t.korean |= language == Language::Korean;
    if t.chars >= TEXT_MAX {
        flush(s);
    }
    true
}

/// Cell `n` of the line being assembled now holds a code the tap kept (`said`) or not.
pub(crate) fn mark(s: &mut State, n: usize, said: bool) {
    let bit = 1u64.checked_shl(n as u32).unwrap_or(0);
    let t = &mut s.speech;
    t.cells = if said { t.cells | bit } else { t.cells & !bit };
}

/// The line's cells from `n` on moved to its start (a wrapped row's tail).
pub(crate) fn shift_marks(s: &mut State, n: usize) {
    s.speech.cells = s.speech.cells.checked_shr(n as u32).unwrap_or(0);
}

/// The kept codes among cells `start..end` of the line being assembled (not drawn yet).
pub(crate) fn marked(s: &State, start: usize, end: usize) -> usize {
    (start..end.min(64))
        .filter(|&n| s.speech.cells >> n & 1 != 0)
        .count()
}

/// [`flush`] at a wait before the line being assembled is drawn (the more prompt): the last
/// `held` codes kept, those of that line, stay for the next flush; what comes before them, all
/// drawn, is let out.
pub(crate) fn flush_drawn(s: &mut State, held: usize) {
    if !s.speech.enabled {
        return;
    }
    let t = &mut s.speech;
    let held = held.min(t.chars);
    let at = t
        .buf
        .char_indices()
        .nth(t.chars - held)
        .map_or(t.buf.len(), |(i, _)| i);
    let tail = t.buf.split_off(at);
    let korean = t.korean;
    flush(s);
    let t = &mut s.speech;
    t.buf = tail;
    t.chars = held;
    t.korean = korean && held > 0;
}

/// Ends the current utterance: queued when it holds a character other than whitespace.
pub(crate) fn flush(s: &mut State) {
    if !s.speech.enabled {
        return;
    }
    let text = std::mem::take(&mut s.speech.buf);
    let korean = std::mem::take(&mut s.speech.korean);
    s.speech.chars = 0;
    if text.chars().all(char::is_whitespace) {
        return;
    }
    let npc = s.converse.npc;
    let speaker = s.speech.speaker.unwrap_or(npc);
    let script = crate::converse::script_item(s, npc);
    let traits = traits(s, speaker);
    let t = &mut s.speech;
    let u = Utterance {
        seq: t.seq,
        turn: t.turn,
        speaker,
        script,
        text,
        lang: if korean { Lang::Ko } else { Lang::En },
        traits,
    };
    t.seq = t.seq.wrapping_add(1);
    t.queue.push_back(u);
    while t.queue.len() > QUEUE {
        t.queue.pop_front();
    }
}

/// An input op of the conversation, before it waits: what was said is flushed and the speaker
/// is the NPC again.
pub(crate) fn before_input(s: &mut State) {
    flush(s);
    s.speech.speaker = None;
}

/// The player gave the input an input op waited for.
pub(crate) fn after_input(s: &mut State) {
    if s.speech.enabled {
        s.speech.turn = s.speech.turn.wrapping_add(1);
    }
}

/// Op `0xbf` shows `actor`'s portrait: what was said is flushed, then that actor speaks until
/// the next input op.
pub(crate) fn portrait(s: &mut State, actor: i16) {
    flush(s);
    if s.speech.enabled {
        s.speech.speaker = Some(actor);
    }
}

/// The conversation ended.
pub(crate) fn end(s: &mut State) {
    flush(s);
    s.speech.speaker = None;
}

/// The traits of actor `a` (also the NPC bundles', spec 09 Design 5).
pub(crate) fn traits(s: &State, a: i16) -> Traits {
    let at = |v: &[u8]| usize::try_from(a).ok().and_then(|i| v.get(i)).copied();
    Traits {
        obj_type: u16::try_from(a).map_or(0, |a| s.objects.obj_type(crate::objects::ObjectId(a))),
        worktype: at(&s.actors.worktype).unwrap_or(0),
        alignment: (at(&s.actors.attrs.status).unwrap_or(0) >> 5) & 3,
    }
}
