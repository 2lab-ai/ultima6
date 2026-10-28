//! Port-only (spec 17): the context menu of a right press at the command prompt, and the run of
//! a chosen entry.
//!
//! The query reads what each command reaches from the pressed map cell or inventory slot: the
//! selection the pointer target 0x105f7c leaves, then each command's own resolution of it and its
//! own checks, through the handlers' functions, on one copy of the state (they move the location
//! cursor, the sums, the weapon list and the book's page; each check takes the selection back as
//! the press left it). An entry is a command that resolves to an object; a check of the target's
//! kind hides it, a check of the current state disables it with a [`Reason`]; a random draw or a
//! later prompt never does. The run is the original's mouse flow: a left press on the command's
//! icon, then a left press at the target once the command reads it ([`Run`]).

use super::cursor::{NO_ICON, target_from_pointer};
use crate::engine::{InputEvent, Io};
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
/// Port-only (spec 17, unit M3): "Go here", travel to the pressed map cell (unit M4, spec 21). No
/// code of the game is this high.
pub const GO_HERE: u16 = 0x100;

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
    /// More than one tile away, or out of the weapons' range.
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
    /// No arrows or bolts for the weapons.
    Ammo,
    /// A foe next to the leader stops the missiles.
    Blocked,
    /// It will not come off (a readied 0x4c).
    Stays,
    /// The chest is locked.
    Locked,
    /// Port-only (spec 17, unit M3): no way to the cell (unit M4's reachability, spec 21).
    Unreachable,
    /// Port-only (unit M3, spec 21's refusals): combat mode is on.
    Combat,
    /// Port-only (unit M3, spec 21's refusals): a hostile is in view.
    Foe,
}

impl Reason {
    /// Every reason, for the page's words.
    pub const ALL: [Reason; 25] = [
        Reason::Far,
        Reason::Heavy,
        Reason::Aboard,
        Reason::Window,
        Reason::Hands,
        Reason::Book,
        Reason::Negate,
        Reason::Spells,
        Reason::Level,
        Reason::Points,
        Reason::Reagents,
        Reason::Solo,
        Reason::Door,
        Reason::Hand,
        Reason::Case,
        Reason::Screen,
        Reason::Silent,
        Reason::Lying,
        Reason::Ammo,
        Reason::Blocked,
        Reason::Stays,
        Reason::Locked,
        Reason::Unreachable,
        Reason::Combat,
        Reason::Foe,
    ];

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
            Reason::Ammo => "ammo",
            Reason::Blocked => "blocked",
            Reason::Stays => "stays",
            Reason::Locked => "locked",
            Reason::Unreachable => "unreachable",
            Reason::Combat => "combat",
            Reason::Foe => "foe",
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
#[derive(Clone, PartialEq, Eq)]
pub struct Section {
    /// The object (-1: the cell's tile).
    pub object: Slot,
    /// The object's type and frame (none for the tile).
    pub kind: Option<(u16, u16)>,
    /// Its display name, UTF-8 (the user's game text or the locale pack's).
    pub name: String,
    pub entries: Vec<Entry>,
}

/// Everything but the name, which is the user's game text: its length only, so a failed
/// comparison or a debug print of a menu never shows it.
impl std::fmt::Debug for Section {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Section")
            .field("object", &self.object)
            .field("kind", &self.kind)
            .field("name_len", &self.name.len())
            .field("entries", &self.entries)
            .finish()
    }
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
    /// Port-only (unit M3): [`GO_HERE`] for the pressed map cell, last; none on a panel and on the
    /// leader's own cell.
    pub go_here: Option<Entry>,
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
/// Wide the inventory view's slot too). `s` is read only: the checks run on one copy of it.
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
        0 => map_menu(&mut c, io)?,
        2 if c.ui.panel.mode == 0x92 => panel_menu(&mut c, io)?,
        #[cfg(feature = "wide")]
        i if super::pointer::SLOT_REGIONS.contains(&i) => {
            if !super::panels::begin_slot_press(&mut c, i - super::pointer::SLOT_REGIONS.start)
                || c.ui.panel.mode != 0x92
            {
                return None;
            }
            panel_menu(&mut c, io)?
        }
        _ => return None,
    };
    menu.underlined = underlined;
    menu.go_here = menu
        .cell
        .filter(|_| menu.kind == Kind::Map)
        .and_then(|cell| go_here(s, cell));
    Some(menu)
}

/// Port-only (unit M3): the "Go here" entry for map cell `cell` (world x, y on the leader's
/// level): none on the leader's own cell or when unit M4 refuses it as `Here` or `NoLeader`,
/// else disabled by its other refusals ([`super::popup::travel_reason`],
/// [`super::popup::go_here_reason`]).
fn go_here(s: &State, cell: (i16, i16)) -> Option<Entry> {
    let here = s
        .leader_position()
        .is_some_and(|p| (p.x as i16, p.y as i16) == cell);
    if here {
        return None;
    }
    let reason = match super::popup::travel_reason(s, cell) {
        None => None,
        Some(r) => Some(super::popup::go_here_reason(r)?),
    };
    Some(Entry {
        code: GO_HERE,
        reason,
        theft: false,
    })
}

/// Aboard a vessel (A4-0x7b7a = the party size): Cast, Get, Drop and Move are refused before
/// their target is read (0x1041d8, `main_loop/commands.rs` `targeted`).
fn aboard(s: &State) -> bool {
    s.actors.party.leader_index == s.actors.party.size
}

fn leader(s: &State) -> Slot {
    s.actors.party.leader().map_or(0, |l| l.0 as Slot)
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

/// The press's copy of the state, each check run on it as the press left it: the selection and
/// the target put back before each (the checks resolve them their own way); what else a check
/// writes (the location cursor, the sums, the weapon list, the book's page) no later check reads.
///
/// Kept to one copy for speed (spec 17 round 2, item 4e); in the tests `fresh` gives each check a
/// copy of its own instead, the press's state as it was, to show the shared copy skews no answer.
struct Press<'a> {
    c: &'a mut State,
    selection: Slot,
    target: (i16, i16),
    fresh: Option<State>,
}

impl<'a> Press<'a> {
    fn new(c: &'a mut State) -> Self {
        let (selection, target) = (c.ui.input.selection, c.ui.input.target);
        #[cfg(test)]
        let fresh = FRESH.with(std::cell::Cell::get).then(|| c.clone());
        #[cfg(not(test))]
        let fresh = None;
        Press {
            c,
            selection,
            target,
            fresh,
        }
    }

    /// The copy, its selection and target as the press left them.
    fn check(&mut self) -> &mut State {
        if let Some(f) = &self.fresh {
            self.c.clone_from(f);
            #[cfg(test)]
            FRESH_COPIES.with(|n| n.set(n.get() + 1));
        }
        self.c.ui.input.selection = self.selection;
        self.c.ui.input.target = self.target;
        self.c
    }
}

#[cfg(test)]
thread_local! {
    /// Each check on a fresh copy ([`Press`]): set only by the tests ([`query_fresh`]).
    static FRESH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The checks a [`Press`] ran on a fresh copy, in all ([`fresh_copies`]).
    static FRESH_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many checks ran on a fresh copy so far on this thread ([`Press::check`]), for the tests.
#[cfg(test)]
pub(crate) fn fresh_copies() -> usize {
    FRESH_COPIES.with(std::cell::Cell::get)
}

/// [`query`] with each check on a copy of its own of the press's state (each entry as if it were
/// the only one asked): the tests compare it with [`query`].
#[cfg(test)]
pub(crate) fn query_fresh(s: &State, io: &Io, x: i16, y: i16) -> Option<Menu> {
    fresh_scope(|| query(s, io, x, y))
}

/// Whether the checks run on fresh copies now ([`FRESH`]), for the tests.
#[cfg(test)]
pub(crate) fn fresh_now() -> bool {
    FRESH.with(std::cell::Cell::get)
}

/// Runs `f` with [`FRESH`] set, then puts back the value it found, also when `f` panics or
/// nests another scope.
#[cfg(test)]
pub(crate) fn fresh_scope<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            FRESH.with(|v| v.set(self.0));
        }
    }
    let _restore = Restore(FRESH.with(|v| v.replace(true)));
    f()
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
                    kind: Some((s.objects.kind(o), s.objects.tf(o) >> 10)),
                    name: name_of(s, o),
                    entries,
                })
            })
            .collect()
    }
}

/// The map menu: the pointer target (0x105f7c) on the copy `c`, then Talk, Look, Use, Get,
/// Move, Cast and Attack, each from that selection.
fn map_menu(c: &mut State, io: &Io) -> Option<Menu> {
    target_from_pointer(c);
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
            kind: None,
            name: tile_name(c),
            entries: vec![look],
        };
        return Some(Menu {
            kind: Kind::Map,
            cell,
            underlined: None,
            sections: vec![section],
            go_here: None,
        });
    }
    // Spec 13 Decision 4: a non-Look command takes a map target only in the original's window.
    let window = (!super::click::in_reach(c, USE)).then_some(Reason::Window);
    let (lead, aboard) = (leader(c), aboard(c));
    let mut p = Press::new(c);
    let mut e = Entries::default();
    if let Some((o, r)) = talk(p.check()) {
        e.add(o, TALK, window.or(r));
    }
    e.add(look_object(p.check()), LOOK, None);
    if let Some((o, r)) = use_entry(p.check(), io) {
        e.add(o, USE, window.or(r));
    }
    if let Some((o, r, theft)) = get_entry(p.check()) {
        e.add(o, GET, window.or(r));
        if let Some(last) = e.0.last_mut() {
            last.1.theft = theft;
        }
    }
    if let Some(r) = move_entry(p.check()) {
        e.add(sel, MOVE, window.or(r));
    }
    let cast = if aboard {
        Some(Reason::Aboard)
    } else {
        crate::magic::cast::refusal_at_target_on(p.check()).map(cast_reason)
    };
    e.add(sel, CAST, window.or(cast));
    if sel != lead {
        e.add(sel, ATTACK, window.or(attack_reason(p.check())));
    }
    let c = p.check();
    Some(Menu {
        kind: Kind::Map,
        cell,
        underlined: None,
        sections: e.sections(c, sel),
        go_here: None,
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
fn look_object(c: &mut State) -> Slot {
    let sel = c.ui.input.selection;
    if sel < 0x100 || c.objects.kind(sel) == HORSE {
        crate::combat::leaves::owner(c, sel)
    } else {
        sel
    }
}

/// Talk (0x1127c4, 0x114afa): the one it talks to and why it would refuse; `None` when Talk
/// does not take the selection: no actor, shrine or statue, the talker itself, or no script to
/// load (NPC 0, or past 0xe0 and not a talking object).
fn talk(c: &mut State) -> Option<(Slot, Option<Reason>)> {
    use crate::converse::start;
    use crate::converse::text::Msg;
    let target = start::talk_target(c, c.ui.input.selection)?;
    let talker = leader(c);
    let reason = match start::refusal(c, talker, target) {
        None => None,
        Some(Msg::TalkingToYourself) => return None,
        Some(Msg::NotOnScreen) => Some(Reason::Screen),
        Some(Msg::NotInSolo) => Some(Reason::Solo),
        Some(_) => Some(Reason::Silent),
    };
    let npc = start::npc_of(c, target);
    if reason.is_none() && !start::may_have_script(npc, c.objects.kind(target)) {
        return None;
    }
    Some((target, reason))
}

/// Use (0x1305e0): its object and why it would refuse; `None` when it reaches nothing or a type
/// outside the use list (0x128b6a).
fn use_entry(u: &mut State, io: &Io) -> Option<(Slot, Option<Reason>)> {
    use crate::usecmd::{cases, dispatch};
    let (obj, _) = dispatch::resolve(u);
    if obj < 0 {
        return None;
    }
    let ty = u.objects.kind(obj);
    if !crate::objects::commands::in_type_list(u, ty) {
        return None;
    }
    let reason = if aboard(u) && crate::usecmd::refused_aboard(u, io, obj) {
        Some(Reason::Aboard)
    } else if dispatch::out_of_reach(u, obj) {
        Some(Reason::Far)
    } else if dispatch::not_now(u, obj) {
        Some(Reason::Door)
    } else if dispatch::in_other_hand(u, obj) {
        Some(Reason::Hand)
    } else {
        let frame = u.objects.tf(obj) >> 10;
        let case = cases::refusal(u, obj, ty, frame).map(|r| match r {
            cases::CaseRefusal::NotSolo => Reason::Solo,
            _ => Reason::Case,
        });
        case.or_else(|| crate::objects::commands::world::locked(u, obj).then_some(Reason::Locked))
    };
    Some((obj, reason))
}

/// Get (0x12a7a4): its object, why it would refuse, and whether taking it is a theft; `None`
/// when it reaches nothing on the map, a harmful tile, or what cannot be taken.
fn get_entry(g: &mut State) -> Option<(Slot, Option<Reason>, bool)> {
    get::resolve(g);
    let obj = g.ui.input.selection;
    if obj == -1 || g.objects.place(obj) != 0 || get::hazard(g, obj) {
        return None;
    }
    let actor = leader(g);
    let wt = get::weigh(g, actor, obj);
    if get::cannot_take(g, obj, wt) {
        return None;
    }
    let reason = if aboard(g) {
        Some(Reason::Aboard)
    } else if get::out_of_reach(g, actor) {
        Some(Reason::Far)
    } else if get::too_heavy(g, actor, wt) {
        Some(Reason::Heavy)
    } else if get::no_free_hand(g, obj) {
        Some(Reason::Hands)
    } else {
        None
    };
    Some((obj, reason, get::theft(g, obj)))
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

/// Attack (0x124caa): refused aboard a vessel other than a ship, else when every weapon refuses
/// at the cell (its ammunition, its range, a foe in the way of a missile).
fn attack_reason(c: &mut State) -> Option<Reason> {
    use crate::combat::attack::{self, WeaponRefusal};
    if attack::refused_aboard(c) {
        return Some(Reason::Aboard);
    }
    attack::refusal_on(c).map(|r| match r {
        WeaponRefusal::Ammo => Reason::Ammo,
        WeaponRefusal::Range => Reason::Far,
        WeaponRefusal::Blocked => Reason::Blocked,
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

/// The panel menu: the slot's object (0x111e3c / 0x111eec), its own left-press action (0x10619c:
/// open, ready, take off), Use, Look, Drop and Move. Drop and Move are hidden on a type 0x4c,
/// readied or not, and so is Ready on an unworn one: the object moves refuse that type at the
/// prompt (0x10c9d2 always, 0x10cda8 unless the game allows it, `objects::moves::FIXED_TYPE`),
/// so none of them can ever put it anywhere. A readied one's take-off stays, disabled.
fn panel_menu(c: &mut State, io: &Io) -> Option<Menu> {
    let (px, py) = c.ui.input.pointer;
    // The container figure goes back to the parent (0x10619c), it is no slot.
    if (0x110..0x120).contains(&px) && (0x10..0x20).contains(&py) {
        return None;
    }
    let code = target_from_pointer(c);
    let sel = c.ui.input.selection;
    if sel < 0 || (code != 0x9b && code != 0x9c) {
        return None;
    }
    let mut own = Entry {
        code: READY,
        reason: None,
        theft: false,
    };
    if code == 0x9c {
        own.code = UNREADY;
        // 0x1122dc: the take-off meets the amulet of submission first: it stays.
        own.reason = crate::inventory::ready::stays(c, sel).then_some(Reason::Stays);
    } else {
        let kind = c.objects.kind(sel);
        if crate::objects::in_type_set_670c(&c.objects, kind) && kind != READY_ANYWAY {
            own.code = OPEN;
            own.reason = super::click::locked_chest(c, sel).then_some(Reason::Locked);
        }
    }
    let aboard = aboard(c).then_some(Reason::Aboard);
    let fixed = push::fixed(c, sel);
    let mut p = Press::new(c);
    let mut e = Entries::default();
    if !(fixed && own.code == READY) {
        e.add(sel, own.code, own.reason);
    }
    if let Some((o, r)) = use_entry(p.check(), io) {
        e.add(o, USE, r);
    }
    e.add(sel, LOOK, None);
    let c = p.check();
    if !drop::nothing_to_drop(c, sel) && !fixed {
        e.add(sel, DROP, aboard);
    }
    if sel as u16 != c.ui.panel.container && !fixed {
        let away = push::giver_away(c).then_some(Reason::Screen);
        e.add(sel, MOVE, aboard.or(away));
    }
    Some(Menu {
        kind: Kind::Panel,
        cell: None,
        underlined: None,
        sections: e.sections(c, sel),
        go_here: None,
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

/// A chosen entry being run: its icon press went in at the prompt read `prompt`
/// ([`super::InputState::prompt_reads`]), its target press waits for the target read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub code: u16,
    /// The target's frame pixel.
    pub at: (i16, i16),
    /// The prompt read the icon press goes to.
    pub prompt: u32,
}

/// What a run does at a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep waiting.
    Hold,
    /// Queue the target press now; the run is over.
    Release([InputEvent; 2]),
    /// A prompt read came after the icon's: the command ended without reading its target.
    /// Forget the press.
    Drop,
}

impl Run {
    /// Starts command `code` (0x81..0x88) on the target at frame pixel `at`, the prompt up in
    /// `s`: the icon press to queue now, and the run that holds the target press.
    pub fn start(s: &State, code: u16, at: (i16, i16)) -> Option<([InputEvent; 2], Run)> {
        let prompt = s.ui.input.prompt_reads;
        (ATTACK..=USE)
            .contains(&code)
            .then(|| (icon_press(code), Run { code, at, prompt }))
    }

    /// The step for the state `s` between two ticks, `quiet` when no event that could change
    /// what the command reads is queued (keys, text, a button): a new prompt read drops the
    /// press whatever is queued; the command's own target read takes it once the queues are
    /// quiet.
    pub fn step(&self, s: &State, quiet: bool) -> Step {
        if s.ui.input.prompt_reads != self.prompt {
            Step::Drop
        } else if quiet && target_read(s, self.code) {
            Step::Release(left_press(self.at.0, self.at.1))
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
        let s = waiting(USE);
        let (icon, run) = Run::start(&s, USE, (40, 56)).expect("a map command");
        assert_eq!(icon, icon_press(USE));
        assert_eq!(run.step(&s, true), Step::Release(left_press(40, 56)));
        let mut more = s.clone();
        more.ui.input.accept = false;
        let mut printing = s.clone();
        printing.ui.input.in_wait = false;
        let mut other = s.clone();
        other.ui.input.target_command = CAST;
        let mut none = s.clone();
        none.ui.input.target_pending = 0;
        for held in [&more, &printing, &other, &none] {
            assert_eq!(run.step(held, true), Step::Hold);
        }
        assert_eq!(run.step(&s, false), Step::Hold, "a key queued first");
        let mut again = none.clone();
        again.ui.input.prompt_reads += 1;
        assert_eq!(run.step(&again, true), Step::Drop, "a new prompt read");
        assert_eq!(
            run.step(&again, false),
            Step::Drop,
            "also with a key queued"
        );
        let mut waits = s.clone();
        waits.ui.input.prompt_reads += 1;
        assert_eq!(
            run.step(&waits, true),
            Step::Drop,
            "another command's target read"
        );
    }

    #[test]
    fn only_the_targeted_commands_start_a_run_and_each_icon_is_its_own_column() {
        let s = State::default();
        assert!(Run::start(&s, REST, (0, 0)).is_none() && Run::start(&s, OPEN, (0, 0)).is_none());
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
        let mut codes: Vec<&str> = Reason::ALL.iter().map(|r| r.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), Reason::ALL.len());
    }

    /// Spec 17 round 5, item 1: a menu's debug print (what a failed `assert_eq!` shows) has no
    /// section name, the user's game text: its length instead (a made-up name here).
    #[test]
    fn a_menus_debug_print_leaves_out_the_names() {
        let name = "made-up name";
        let m = Menu {
            kind: Kind::Map,
            cell: Some((1, 2)),
            underlined: None,
            go_here: None,
            sections: vec![Section {
                object: 300,
                kind: Some((0x80, 0)),
                name: name.into(),
                entries: vec![Entry {
                    code: LOOK,
                    reason: None,
                    theft: false,
                }],
            }],
        };
        let shown = format!("{m:?} {m:#?}");
        assert!(!shown.contains(name), "no name");
        assert!(shown.contains("name_len: 12") && shown.contains("object: 300"));
    }

    /// Spec 17 round 5, item 8: the fresh-copy switch is put back as it was after a scope, nested
    /// or ended by a panic.
    #[test]
    fn the_fresh_copy_switch_comes_back_as_it_was() {
        let now = || FRESH.with(std::cell::Cell::get);
        assert!(!now());
        fresh_scope(|| {
            assert!(now());
            fresh_scope(|| assert!(now()));
            assert!(now(), "still set after the inner scope");
        });
        assert!(!now(), "clear after the outer");
        let ended = std::panic::catch_unwind(|| fresh_scope(|| panic!("ended")));
        assert!(ended.is_err());
        assert!(!now(), "clear after a panic");
    }
}
