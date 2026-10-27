//! Port-only (spec 17): the context menu of a right press at the command prompt, and the run of
//! a chosen entry.
//!
//! The query reads what each command reaches from the pressed map cell or inventory slot: the
//! selection the pointer target 0x105f7c leaves, then each command's own resolution of it and its
//! own checks, run on a copy of the state (they move the location cursor and the sums) through
//! the handlers' functions. An entry is a command that resolves to an object; a check of the
//! target's kind hides it, a check of the current state disables it with a [`Reason`]; a random
//! draw or a later prompt never does. The run is the original's mouse flow: a left press on the
//! command's icon, then a left press at the target once the command reads it ([`Run`]).

use super::cursor::{NO_ICON, target_from_pointer};
use super::target::LEADER_CELL;
use crate::engine::{InputEvent, Io};
use crate::geom::RULE_HALF;
use crate::objects::commands::{drop, get, push};
use crate::objects::{Shown, Slot};
use crate::state::State;

/// Command codes (the icon codes of the command bar, 0x10658e).
pub const ATTACK: u16 = 0x81;
pub const CAST: u16 = 0x82;
pub const TALK: u16 = 0x83;
pub const LOOK: u16 = 0x84;
pub const GET: u16 = 0x85;
pub const DROP: u16 = 0x86;
pub const MOVE: u16 = 0x87;
pub const USE: u16 = 0x88;
pub const REST: u16 = 0x89;
pub const COMBAT: u16 = 0x8a;
/// The codes of an inventory slot's own left press (0x10619c): open a container, ready, take off.
pub const OPEN: u16 = 0xa0;
pub const READY: u16 = 0xa7;
pub const UNREADY: u16 = 0xa8;

/// The row of the command bar's icons (click region 1, (8, 0xb0)..(0xa7, 0xbf)).
const ICON_Y: i16 = 0xb8;
/// The horse whose rider Look describes (0x1298b8).
const HORSE: u16 = 0x1ae;
/// The type 0x10619c readies though it is in the container set.
const READY_ANYWAY: u16 = 0x39;

/// What the menu shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A map cell.
    Map,
    /// An inventory or equipment slot.
    Panel,
}

/// Why an entry is shown disabled: a check of the current state that refuses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// More than one tile away.
    Far,
    /// Too heavy to carry.
    Heavy,
    /// Aboard a vessel.
    Aboard,
    /// Outside Wide's 11 × 11 window round the leader (spec 13 Decision 4).
    Window,
    /// No free hand for a lit torch.
    Hands,
    /// No readied spellbook.
    Book,
    /// The Negate Magic timer runs.
    Negate,
    /// The books hold no spell.
    Spells,
    /// The caster's level is below the spell's circle.
    Level,
    /// Too few magic points.
    Points,
    /// No reagents for the spell.
    Reagents,
    /// Solo mode.
    Solo,
    /// Something lies on the door.
    Door,
    /// Readied by another member.
    Hand,
    /// The object's own use refuses now.
    Case,
    /// The member is off screen.
    Screen,
    /// The one talked to cannot answer.
    Silent,
    /// The actor lies down.
    Lying,
}

impl Reason {
    /// The code the page words.
    pub fn code(self) -> &'static str {
        match self {
            Reason::Far => "far",
            Reason::Heavy => "heavy",
            Reason::Aboard => "aboard",
            Reason::Window => "window",
            Reason::Hands => "hands",
            Reason::Book => "book",
            Reason::Negate => "negate",
            Reason::Spells => "spells",
            Reason::Level => "level",
            Reason::Points => "points",
            Reason::Reagents => "reagents",
            Reason::Solo => "solo",
            Reason::Door => "door",
            Reason::Hand => "hand",
            Reason::Case => "case",
            Reason::Screen => "screen",
            Reason::Silent => "silent",
            Reason::Lying => "lying",
        }
    }
}

/// One command of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub code: u16,
    /// `None`: enabled.
    pub reason: Option<Reason>,
    /// Get of what is not the party's.
    pub theft: bool,
}

impl Entry {
    pub fn enabled(&self) -> bool {
        self.reason.is_none()
    }
}

/// The commands that resolve to one object, headed by its display name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The object (-1: the cell's tile).
    pub object: Slot,
    /// Its display name, UTF-8 (the user's game text or the locale pack's).
    pub name: String,
    pub entries: Vec<Entry>,
}

/// The menu of one right press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub kind: Kind,
    /// The map cell (map menus).
    pub cell: Option<(i16, i16)>,
    /// The underlined command (A4-0x7b09), shown first.
    pub underlined: Option<u16>,
    pub sections: Vec<Section>,
}

impl Menu {
    /// The entry of `code` in the section of `object`, if shown.
    pub fn entry(&self, object: Slot, code: u16) -> Option<Entry> {
        let section = self.sections.iter().find(|s| s.object == object)?;
        section.entries.iter().copied().find(|e| e.code == code)
    }
}

/// The menu of a right press at frame pixel (x, y): `None` unless the command loop waits for a
/// command (A4-0x7a2c set) and the pixel is on the map or on an inventory or equipment slot (in
/// Wide the inventory view's slot too). `s` is read only.
pub fn query(s: &State, io: &Io, x: i16, y: i16) -> Option<Menu> {
    if s.main.reading != 1 {
        return None;
    }
    let mut c = s.clone();
    c.ui.input.pointer = (x, y);
    c.ui.pointer.pressed = true;
    let underlined = Some(u16::from(super::globals::last_command(&c.actors)))
        .filter(|&u| u != u16::from(NO_ICON));
    let mut menu = match super::click::region_at(&c, x, y)? {
        0 => map_menu(c, io)?,
        2 if c.ui.panel.mode == 0x92 => panel_menu(c, io)?,
        #[cfg(feature = "wide")]
        i if super::pointer::SLOT_REGIONS.contains(&i) => {
            if !super::panels::begin_slot_press(&mut c, i - super::pointer::SLOT_REGIONS.start)
                || c.ui.panel.mode != 0x92
            {
                return None;
            }
            panel_menu(c, io)?
        }
        _ => return None,
    };
    menu.underlined = underlined;
    Some(menu)
}

/// Aboard a vessel (A4-0x7b7a = the party size): Cast, Get, Drop and Move are refused before
/// their target is read (0x1041d8, `main_loop/commands.rs` `targeted`).
fn aboard(s: &State) -> bool {
    s.actors.party.leader_index == s.actors.party.size
}

fn leader(s: &State) -> Slot {
    s.actors.party.leader().map_or(0, |l| l.0 as Slot)
}

/// Spec 13 Decision 4: a non-Look command takes a map target only in the original's 11 × 11
/// window round the leader (`ui/click.rs`, `in_reach`); in Original that is the whole view.
fn in_window(s: &State) -> bool {
    let (px, py) = s.ui.input.pointer;
    let (col, row) = super::pointer::view_cell(px, py);
    let (lx, ly) = (i16::from(LEADER_CELL.0), i16::from(LEADER_CELL.1));
    let rule = -RULE_HALF..=RULE_HALF;
    rule.contains(&(col - lx)) && rule.contains(&(row - ly))
}

/// The display name of `slot` as the commands print it (0x10eef8, [`crate::objects::shown_name`]),
/// its plural markers read for its quantity.
fn name_of(s: &State, slot: Slot) -> String {
    let plural = crate::objects::quantity(&s.objects, slot) != 1;
    shown_text(crate::objects::shown_name(s, slot), plural)
}

fn shown_text(shown: Shown, plural: bool) -> String {
    let text = match shown {
        Shown::English(t) => t,
        Shown::Pack(b) => String::from_utf8_lossy(&b).into_owned(),
    };
    u6_formats::look::resolve_plural(&text, plural)
}

/// The entries of one press, each with the object its command resolves to.
#[derive(Default)]
struct Entries(Vec<(Slot, Entry)>);

impl Entries {
    fn add(&mut self, object: Slot, code: u16, reason: Option<Reason>) {
        let entry = Entry {
            code,
            reason,
            theft: false,
        };
        self.0.push((object, entry));
    }

    /// One section per object, `first`'s first, the others in the order of their first entry.
    fn sections(self, s: &State, first: Slot) -> Vec<Section> {
        let mut order: Vec<Slot> = vec![first];
        for &(o, _) in &self.0 {
            if !order.contains(&o) {
                order.push(o);
            }
        }
        order
            .into_iter()
            .filter_map(|o| {
                let entries: Vec<Entry> = self.0.iter().filter(|e| e.0 == o).map(|e| e.1).collect();
                (!entries.is_empty()).then(|| Section {
                    object: o,
                    name: name_of(s, o),
                    entries,
                })
            })
            .collect()
    }
}

/// The map menu: the pointer target (0x105f7c) on the copy `c`, then Talk, Look, Use, Get,
/// Move, Cast and Attack, each from that selection.
fn map_menu(mut c: State, io: &Io) -> Option<Menu> {
    target_from_pointer(&mut c);
    let sel = c.ui.input.selection;
    let cell = Some(c.ui.input.target);
    // An invisible selection (status 0x02) is the tile to Look as well (0x1298b8 pseudocode
    // l.52): it gets no section of its own.
    if sel < 0 || c.objects.status(sel) & 0x02 != 0 {
        let look = Entry {
            code: LOOK,
            reason: None,
            theft: false,
        };
        let section = Section {
            object: -1,
            name: tile_name(&c),
            entries: vec![look],
        };
        return Some(Menu {
            kind: Kind::Map,
            cell,
            underlined: None,
            sections: vec![section],
        });
    }
    let window = (!in_window(&c)).then_some(Reason::Window);
    let mut e = Entries::default();
    if let Some((o, r)) = talk(&c) {
        e.add(o, TALK, window.or(r));
    }
    e.add(look_object(&c), LOOK, None);
    if let Some((o, r)) = use_entry(&c, io) {
        e.add(o, USE, window.or(r));
    }
    if let Some((o, r, theft)) = get_entry(&c) {
        e.add(o, GET, window.or(r));
        if let Some(last) = e.0.last_mut() {
            last.1.theft = theft;
        }
    }
    if let Some(r) = move_entry(&c) {
        e.add(sel, MOVE, window.or(r));
    }
    let cast = if aboard(&c) {
        Some(Reason::Aboard)
    } else {
        crate::magic::cast::refusal_at_target(&mut c.clone()).map(cast_reason)
    };
    e.add(sel, CAST, window.or(cast));
    if sel != leader(&c) {
        let aboard = crate::combat::attack::refused_aboard(&c).then_some(Reason::Aboard);
        e.add(sel, ATTACK, window.or(aboard));
    }
    Some(Menu {
        kind: Kind::Map,
        cell,
        underlined: None,
        sections: e.sections(&c, sel),
    })
}

/// The name of the cell's map tile (what Look describes there); none for a cell that shows
/// nothing (A4-0x2efe).
fn tile_name(c: &State) -> String {
    if c.ui.input.target_void {
        return String::new();
    }
    let (x, y) = c.ui.input.target;
    let z = u16::from(c.map.centre.z);
    let tile = crate::actors::seams::tile_at(c, x as u16, y as u16, z);
    shown_text(crate::objects::shown_look(c, u16::from(tile)), false)
}

/// Look's object (0x1298b8 pseudocode l.52-60): an actor's or a horse's rider (0x1240fa).
fn look_object(c: &State) -> Slot {
    let sel = c.ui.input.selection;
    if sel < 0x100 || c.objects.kind(sel) == HORSE {
        crate::combat::leaves::owner(&mut c.clone(), sel)
    } else {
        sel
    }
}

/// Talk (0x1127c4, 0x114afa): the one it talks to and why it would refuse; `None` when Talk
/// does not take the selection: no actor, shrine or statue, the talker itself, or no script to
/// load (NPC 0, or past 0xe0 and not a talking object).
fn talk(c: &State) -> Option<(Slot, Option<Reason>)> {
    use crate::converse::text::Msg;
    let mut t = c.clone();
    let target = crate::converse::start::talk_target(&mut t, c.ui.input.selection)?;
    let talker = leader(&t);
    let refusal = crate::converse::start::refusal(&mut t, talker, target);
    let reason = match refusal {
        None => None,
        Some(Msg::TalkingToYourself) => return None,
        Some(Msg::NotOnScreen) => Some(Reason::Screen),
        Some(Msg::NotInSolo) => Some(Reason::Solo),
        Some(_) => Some(Reason::Silent),
    };
    let npc = t.converse.npc;
    if reason.is_none() && !crate::converse::start::may_have_script(npc, t.objects.kind(target)) {
        return None;
    }
    Some((target, reason))
}

/// Use (0x1305e0): its object and why it would refuse; `None` when it reaches nothing or a type
/// outside the use list (0x128b6a).
fn use_entry(c: &State, io: &Io) -> Option<(Slot, Option<Reason>)> {
    use crate::usecmd::dispatch;
    let mut u = c.clone();
    let (obj, _) = dispatch::resolve(&mut u);
    if obj < 0 {
        return None;
    }
    let ty = u.objects.kind(obj);
    if !crate::objects::commands::in_type_list(&u, ty) {
        return None;
    }
    let reason = if aboard(&u) && crate::usecmd::refused_aboard(&u, io, obj) {
        Some(Reason::Aboard)
    } else if dispatch::out_of_reach(&u, obj) {
        Some(Reason::Far)
    } else if dispatch::not_now(&mut u, obj) {
        Some(Reason::Door)
    } else if dispatch::in_other_hand(&u, obj) {
        Some(Reason::Hand)
    } else {
        let frame = u.objects.tf(obj) >> 10;
        dispatch::case_refusal(&u, obj, ty, frame).map(|r| match r {
            dispatch::CaseRefusal::Solo => Reason::Solo,
            dispatch::CaseRefusal::Case => Reason::Case,
        })
    };
    Some((obj, reason))
}

/// Get (0x12a7a4): its object, why it would refuse, and whether taking it is a theft; `None`
/// when it reaches nothing on the map, a harmful tile, or what cannot be taken.
fn get_entry(c: &State) -> Option<(Slot, Option<Reason>, bool)> {
    let mut g = c.clone();
    get::resolve(&mut g);
    let obj = g.ui.input.selection;
    if obj == -1 || g.objects.place(obj) != 0 || get::hazard(&g, obj) {
        return None;
    }
    let actor = leader(&g);
    let wt = get::weigh(&mut g, actor, obj);
    if get::cannot_take(&g, obj, wt) {
        return None;
    }
    let reason = if aboard(&g) {
        Some(Reason::Aboard)
    } else if get::out_of_reach(&g, actor) {
        Some(Reason::Far)
    } else if get::too_heavy(&g, actor, wt) {
        Some(Reason::Heavy)
    } else if get::no_free_hand(&g, obj) {
        Some(Reason::Hands)
    } else {
        None
    };
    Some((obj, reason, get::theft(&g, obj)))
}

/// Move of a map object (0x12ae04): why it would refuse; `None` when it does not take the
/// selection: the shown container, what has no weight or a dragon part, the pusher.
fn move_entry(c: &State) -> Option<Option<Reason>> {
    let sel = c.ui.input.selection;
    if sel as u16 == c.ui.panel.container || push::immovable(c, sel) || sel == leader(c) {
        return None;
    }
    Some(if aboard(c) {
        Some(Reason::Aboard)
    } else if push::out_of_reach(c) {
        Some(Reason::Far)
    } else if push::lying_down(c, sel) {
        Some(Reason::Lying)
    } else {
        None
    })
}

fn cast_reason(r: crate::magic::cast::Refusal) -> Reason {
    use crate::magic::cast::Refusal;
    match r {
        Refusal::Book => Reason::Book,
        Refusal::Negate => Reason::Negate,
        Refusal::Spells => Reason::Spells,
        Refusal::Level => Reason::Level,
        Refusal::Points => Reason::Points,
        Refusal::Reagents => Reason::Reagents,
    }
}

/// The panel menu: the slot's object (0x111e3c / 0x111eec), its own left-press action, Look,
/// Drop, Move and Use.
fn panel_menu(mut c: State, io: &Io) -> Option<Menu> {
    let (px, py) = c.ui.input.pointer;
    // The container figure goes back to the parent (0x10619c), it is no slot.
    if (0x110..0x120).contains(&px) && (0x10..0x20).contains(&py) {
        return None;
    }
    let code = target_from_pointer(&mut c);
    let sel = c.ui.input.selection;
    if sel < 0 || (code != 0x9b && code != 0x9c) {
        return None;
    }
    let own = if code == 0x9c {
        UNREADY
    } else {
        let kind = c.objects.kind(sel);
        if crate::objects::in_type_set_670c(&c.objects, kind) && kind != READY_ANYWAY {
            OPEN
        } else {
            READY
        }
    };
    let aboard = aboard(&c).then_some(Reason::Aboard);
    let mut e = Entries::default();
    e.add(sel, own, None);
    e.add(sel, LOOK, None);
    if !drop::nothing_to_drop(&c, sel) {
        e.add(sel, DROP, aboard);
    }
    if sel as u16 != c.ui.panel.container {
        e.add(sel, MOVE, aboard);
    }
    if let Some((o, r)) = use_entry(&c, io) {
        e.add(o, USE, r);
    }
    Some(Menu {
        kind: Kind::Panel,
        cell: None,
        underlined: None,
        sections: e.sections(&c, sel),
    })
}

/// The pointer events of a left press and release at frame pixel (x, y).
pub fn left_press(x: i16, y: i16) -> [InputEvent; 2] {
    [
        InputEvent::Pointer { x, y, buttons: 1 },
        InputEvent::Pointer { x, y, buttons: 0 },
    ]
}

/// The left press on the command bar's icon of `code` (0x81..0x8a; x = (code − 0x81) × 16 + 8,
/// the icon's middle), as 0x10658e reads it.
pub fn icon_press(code: u16) -> [InputEvent; 2] {
    let x = (code as i16 - ATTACK as i16) * 16 + 8 + 8;
    left_press(x, ICON_Y)
}

/// True while command `code` waits for its target in the event wait: A4-0x7a32 is 1, A4-0x7a2e
/// is the code, keys go through command processing (A4-0x7a42: not at a more prompt) and the
/// engine is blocked in 0x106bbc ([`super::InputState::in_wait`]).
pub fn target_read(s: &State, code: u16) -> bool {
    let i = &s.ui.input;
    i.target_pending == 1 && i.target_command == code && i.accept && i.in_wait
}

/// A chosen entry being run: its icon press went in, its target press waits for the target read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub code: u16,
    /// The target's frame pixel.
    pub at: (i16, i16),
}

/// What a run does at a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep waiting.
    Hold,
    /// Queue the target press now; the run is over.
    Release([InputEvent; 2]),
    /// The command came back to the prompt without reading its target: forget the press.
    Drop,
}

impl Run {
    /// Starts command `code` (0x81..0x88) on the target at frame pixel `at`: the icon press to
    /// queue now, and the run that holds the target press.
    pub fn start(code: u16, at: (i16, i16)) -> Option<([InputEvent; 2], Run)> {
        (ATTACK..=USE)
            .contains(&code)
            .then(|| (icon_press(code), Run { code, at }))
    }

    /// The step for the state `s` between two ticks: `quiet` when no event that could change
    /// what the command reads is queued (keys, text, a button), `at_prompt` when the command loop
    /// waits for a command with nothing queued.
    pub fn step(&self, s: &State, quiet: bool, at_prompt: bool) -> Step {
        if quiet && target_read(s, self.code) {
            Step::Release(left_press(self.at.0, self.at.1))
        } else if at_prompt {
            Step::Drop
        } else {
            Step::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(code: u16) -> State {
        let mut s = State::default();
        let i = &mut s.ui.input;
        i.target_pending = 1;
        i.target_command = code;
        i.accept = true;
        i.in_wait = true;
        s
    }

    #[test]
    fn the_target_press_goes_in_only_while_the_command_waits_in_the_event_wait() {
        let (icon, run) = Run::start(USE, (40, 56)).expect("a map command");
        assert_eq!(icon, icon_press(USE));
        let s = waiting(USE);
        assert_eq!(run.step(&s, true, false), Step::Release(left_press(40, 56)));
        let mut more = s.clone();
        more.ui.input.accept = false;
        let mut printing = s.clone();
        printing.ui.input.in_wait = false;
        let mut other = s.clone();
        other.ui.input.target_command = CAST;
        let mut none = s.clone();
        none.ui.input.target_pending = 0;
        for held in [&more, &printing, &other, &none] {
            assert_eq!(run.step(held, true, false), Step::Hold);
        }
        assert_eq!(run.step(&s, false, false), Step::Hold, "a key queued first");
        assert_eq!(
            run.step(&none, true, true),
            Step::Drop,
            "back at the prompt"
        );
    }

    #[test]
    fn only_the_targeted_commands_start_a_run_and_each_icon_is_its_own_column() {
        assert!(Run::start(REST, (0, 0)).is_none() && Run::start(OPEN, (0, 0)).is_none());
        for code in ATTACK..=COMBAT {
            let [
                InputEvent::Pointer { x, y, buttons: 1 },
                InputEvent::Pointer { buttons: 0, .. },
            ] = icon_press(code)
            else {
                panic!("a press and a release");
            };
            assert_eq!(((x - 8) >> 4) + 0x81, code as i16, "0x10658e's icon");
            assert!((0xb0..=0xbf).contains(&y) && (8..=0xa7).contains(&x));
        }
    }

    #[test]
    fn every_reason_has_its_own_code() {
        use Reason::*;
        let all = [
            Far, Heavy, Aboard, Window, Hands, Book, Negate, Spells, Level, Points, Reagents, Solo,
            Door, Hand, Case, Screen, Silent, Lying,
        ];
        let mut codes: Vec<&str> = all.iter().map(|r| r.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all.len());
    }
}
