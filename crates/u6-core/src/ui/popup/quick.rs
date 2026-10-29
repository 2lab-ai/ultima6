//! Ctrl-only default actions use the very same menu query, painter and command runner.
use super::*;

/// Alignment opposition used by the game's target search, without choosing a target or RNG.
/// Party members and dead actors are never representative attack targets.
fn hostile(s: &State, object: Slot) -> bool {
    // Attack resolves monster parts to their actor. Its owner lookup may move the location
    // cursor, so query a copy and keep the menu's selected part and target cell unchanged.
    let object = if object >= 0x100 {
        let kind = s.objects.kind(object);
        // These are exactly the kinds owner() can resolve to another slot. Ordinary map
        // objects cannot be actors, and need no State copy just to reject their Attack.
        if !((0x19b..=0x19d).contains(&kind) || kind >= 0x1a9) {
            return false;
        }
        crate::combat::leaves::owner(&mut s.clone(), object)
    } else {
        object
    };
    let Ok(i) = usize::try_from(object) else {
        return false;
    };
    if i >= 0x100 || s.objects.type_frame[i] == 0 || s.actors.attrs.status[i] & 0x90 != 0 {
        return false;
    }
    let Some(leader) = s.actors.party.leader() else {
        return false;
    };
    i != leader.index() && crate::combat::npc::opposes(s, object, leader.0 as i16)
}

/// Select only an existing command; disabled target actions keep their reason. Empty ground
/// offers travel only when reachable. A person's hostile state outranks their Talk entry.
fn representative(s: &State, menu: &Menu) -> Option<Menu> {
    if menu.kind != Kind::Map {
        return None;
    }
    for code in [menu::ATTACK, menu::USE, menu::TALK] {
        for section in &menu.sections {
            let applies = match code {
                menu::ATTACK => hostile(s, section.object),
                menu::USE => section
                    .kind
                    .is_some_and(|(kind, _)| crate::magic::is_door(s, kind) || kind == 0x14e),
                _ => !hostile(s, section.object),
            };
            if applies && let Some(entry) = section.entries.iter().find(|e| e.code == code) {
                let mut section = section.clone();
                section.name.clear();
                section.entries = vec![*entry];
                return Some(Menu {
                    kind: menu.kind,
                    cell: menu.cell,
                    underlined: None,
                    sections: vec![section],
                    go_here: None,
                });
            }
        }
    }
    let go_here = menu.go_here.filter(Entry::enabled)?;
    Some(Menu {
        kind: menu.kind,
        cell: menu.cell,
        underlined: None,
        sections: vec![],
        go_here: Some(go_here),
    })
}

impl Popup {
    pub fn quick(&self) -> Option<&Open> {
        self.quick.as_ref()
    }
    pub fn drawn(&self) -> Option<&Open> {
        self.open.as_ref().or(self.quick.as_ref())
    }

    pub(super) fn set_quick(&mut self, next: Option<Open>) {
        if self.quick != next {
            self.quick = next;
            self.version += 1;
        }
    }

    /// Preview and a fresh captured press share the world-command boundary. A missing
    /// representative action still cancels a world click, but readers outside that boundary
    /// (intro, ending, dialogue, more/target prompts and queued input) keep their own clicks.
    fn quick_ready(&self, s: &State, quiet: bool) -> bool {
        self.on
            && self.control
            && self.open.is_none()
            && self.run.is_none()
            && quiet
            && crate::engine::command_prompt(s)
            && s.main.program == crate::main_loop::Program::Game
    }

    /// Requery even a stationary pointer, so a changed allegiance, obstruction or prompt is
    /// reflected before the next draw/click. Query operates on a copy, never advances the game.
    pub(crate) fn refresh_quick(&mut self, s: &State, io: &Io, quiet: bool) {
        let next = (|| {
            if !self.quick_ready(s, quiet) {
                return None;
            }
            let (x, y) = self.last_pointer?;
            if self.hidden.iter().any(|&(x0, y0, x1, y1)| {
                i32::from(x) >= x0 && i32::from(x) <= x1 && i32::from(y) >= y0 && i32::from(y) <= y1
            }) {
                return None;
            }
            let menu = menu::query(s, io, x, y)?;
            let single = representative(s, &menu)?;
            Some(Open::new(&single, (x, y), s, &self.hidden))
        })();
        if self.quick_press.as_ref().is_some_and(|old| {
            next.as_ref().is_none_or(|now| {
                old.cell != now.cell || old.item(old.hover) != now.item(now.hover)
            })
        }) {
            self.quick_press = None;
        }
        self.set_quick(next);
    }

    /// Down captures the target; up chooses only that same still-valid target. A release after
    /// Ctrl-up, a target change, a modal, or capture cancellation is swallowed without a command.
    pub(super) fn quick_pointer(
        &mut self,
        s: &State,
        (before, buttons): (u8, u8),
        quiet: bool,
        out: &mut Vec<InputEvent>,
    ) -> bool {
        if self.quick_down {
            if let Some((x, y)) = self.last_pointer {
                self.pointer = Some(Pointer { x, y, buttons: 0 });
            }
            if buttons & POINTER_RIGHT != 0 {
                self.quick_press = None;
            }
            if buttons & POINTER_LEFT == 0 {
                self.quick_down = false;
                if let Some(at) = self.pointer.take() {
                    out.push(InputEvent::Pointer {
                        x: at.x,
                        y: at.y,
                        buttons: 0,
                    });
                }
                let pressed = self.quick_press.take();
                if buttons == 0
                    && let (Some(old), Some(now)) = (pressed, self.quick.as_ref())
                {
                    let a = old.item(old.hover);
                    let b = now.item(now.hover);
                    if old.cell == now.cell
                        && a == b
                        && let Some(item) = a.filter(Item::enabled)
                    {
                        self.new_input = true;
                        self.set_quick(None);
                        self.run_item(s, item, old.at, old.cell, out);
                    }
                }
            }
            return true;
        }
        if self.quick_ready(s, quiet) && buttons == POINTER_LEFT && before == 0 {
            self.quick_down = true;
            if let Some((x, y)) = self.last_pointer {
                self.pointer = Some(Pointer { x, y, buttons: 0 });
            }
            self.quick_press = self.quick.clone();
            self.new_input = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(code: u16) -> Entry {
        Entry {
            code,
            reason: None,
            theft: false,
        }
    }
    fn fixture() -> (State, Menu) {
        let mut s = State::default();
        s.actors.party.size = 1;
        s.actors.party.members[0] = 1;
        s.actors.attrs.status[1] = 0xc0;
        s.objects.type_frame[2] = 1;
        let m = Menu {
            kind: Kind::Map,
            cell: Some((10, 10)),
            underlined: None,
            sections: vec![menu::Section {
                object: 2,
                kind: Some((1, 0)),
                name: String::new(),
                entries: vec![entry(menu::TALK), entry(menu::ATTACK)],
            }],
            go_here: Some(entry(menu::GO_HERE)),
        };
        (s, m)
    }
    fn code(s: &State, m: &Menu) -> Option<u16> {
        representative(s, m)
            .and_then(|m| {
                m.go_here
                    .or_else(|| m.sections.first()?.entries.first().copied())
            })
            .map(|e| e.code)
    }
    #[test]
    fn current_allegiance_not_appearance_selects_attack_before_talk() {
        let (mut s, m) = fixture();
        for (status, expected) in [
            (0, menu::TALK),
            (0x40, menu::TALK),
            (0x20, menu::ATTACK),
            (0x60, menu::ATTACK),
            (0xa0, menu::TALK),
            (0x30, menu::TALK),
            (0x40, menu::TALK),
        ] {
            s.actors.attrs.status[2] = status;
            assert_eq!(code(&s, &m), Some(expected), "status {status}");
        }
    }
    #[test]
    fn serpent_body_uses_its_hostile_heads_existing_attack() {
        let (mut s, mut m) = fixture();
        s.objects.type_frame[2] = 0x19d;
        s.actors.attrs.status[2] = 0x20;
        s.objects.type_frame[256] = 0x19d;
        s.objects.qq[256] = 2;
        m.sections[0].object = 256;
        m.sections[0].kind = Some((0x19d, 0));
        m.sections[0].entries = vec![entry(menu::ATTACK)];
        m.go_here = None;
        assert_eq!(crate::combat::leaves::owner(&mut s.clone(), 256), 2);
        assert_eq!(code(&s, &m), Some(menu::ATTACK));
    }
    fn part_fixture(kind: u16) -> (State, Menu) {
        let (mut s, mut m) = fixture();
        s.objects.type_frame[2] = kind;
        s.actors.attrs.status[2] = 0x20;
        s.objects.type_frame[257] = kind | (1 << 10);
        s.objects.qq[257] = 0xab02;
        s.objects.set_xyz(2, 10, 10, 0);
        s.objects.set_xyz(257, 11, 10, 0);
        s.objects.next[256] = 2;
        s.objects.next[2] = 257;
        s.objects.next[257] = -1;
        s.objects.set_window(0, 0, 0);
        s.tables.basetile.base = vec![0; 1024];
        // The head connects east; the adjacent part connects west to the head.
        s.tables.tileflags.flags1 = vec![0x40, 0x10];
        s.objects.at.cur = 19;
        s.objects.at.pos = [3, 4, 5];
        m.cell = Some((11, 10));
        m.sections[0].object = 257;
        m.sections[0].kind = Some((kind, 1));
        m.sections[0].entries = vec![entry(menu::ATTACK)];
        m.go_here = None;
        (s, m)
    }
    #[test]
    fn monster_parts_keep_the_selected_cell_entry_and_live_state() {
        for kind in [0x19b, 0x19c, 0x19d, 0x1aa, 0x1a9] {
            let (mut s, mut m) = part_fixture(kind);
            if kind == 0x1a9 {
                s.objects.type_frame[2] = 0x176;
                s.objects.type_frame[257] = kind | (2 << 12);
                s.objects.defs.dir_dx = vec![0, 1, 1];
                s.objects.defs.dir_dy = vec![-1, -1, 0];
            }
            assert_eq!(crate::combat::leaves::owner(&mut s.clone(), 257), 2);
            for reason in [
                None,
                Some(Reason::Far),
                Some(Reason::Blocked),
                Some(Reason::Window),
            ] {
                m.sections[0].entries[0].reason = reason;
                let before = s.clone();
                let picked = representative(&s, &m).expect("hostile part Attack");
                assert_eq!(picked.cell, Some((11, 10)));
                assert_eq!(picked.sections[0].object, 257);
                assert_eq!(picked.sections[0].entries, m.sections[0].entries);
                assert!(
                    s == before,
                    "query leaves the whole State, including cursors and RNG, intact"
                );
            }
            m.sections[0].entries.clear();
            assert_eq!(code(&s, &m), None, "never invents an Attack");
        }
    }
    #[test]
    fn monster_parts_track_their_owners_current_allegiance_and_death() {
        for kind in [0x19b, 0x19c, 0x19d, 0x1aa] {
            let (mut s, m) = part_fixture(kind);
            for (status, expected) in [
                (0x20, Some(menu::ATTACK)),
                (0, None),
                (0x40, None),
                (0x30, None),
                (0xa0, None),
                (0x20, Some(menu::ATTACK)),
            ] {
                s.actors.attrs.status[2] = status;
                assert_eq!(code(&s, &m), expected, "kind {kind}, status {status}");
            }
        }
    }
    #[test]
    fn dead_bodies_malformed_and_unowned_parts_are_not_hostile() {
        let (s, m) = part_fixture(0x19d);
        for (part, head, quantity) in [
            (0x19d, 0, 2),
            (0x19d, 0x153, 2),
            (0x153, 0x19d, 2),
            (0, 0x19d, 2),
            (0x19d, 0x19d, 3),
            (0x19d, 0x19d, 0xff),
        ] {
            let mut trial = s.clone();
            trial.objects.type_frame[257] = part;
            trial.objects.type_frame[2] = head;
            trial.objects.qq[257] = quantity;
            assert_eq!(code(&trial, &m), None);
        }
        for kind in [0x19b, 0x19c, 0x1aa] {
            let (mut s, m) = part_fixture(kind);
            s.tables.tileflags.flags1[0] = 0;
            assert_eq!(code(&s, &m), None, "no reciprocal connection");
            s.tables.tileflags.flags1[1] = 0;
            assert_eq!(code(&s, &m), None, "no part connection");
        }
        for object in [-1, i16::MAX] {
            let mut m = m.clone();
            m.sections[0].object = object;
            assert_eq!(code(&s, &m), None, "invalid slot");
        }
    }
    #[test]
    fn only_reachable_ground_and_existing_commands_are_offered() {
        let (s, mut m) = fixture();
        m.sections.clear();
        assert_eq!(code(&s, &m), Some(menu::GO_HERE));
        m.go_here.as_mut().expect("travel").reason = Some(Reason::Unreachable);
        assert_eq!(code(&s, &m), None);
        m.go_here = None;
        assert_eq!(code(&s, &m), None);
        m.kind = Kind::Panel;
        assert_eq!(code(&s, &m), None);
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn command_state() -> State {
        let mut s = State::default();
        s.main.program = crate::main_loop::Program::Game;
        s.main.reading = 1;
        s.ui.input.accept = true;
        s
    }

    #[test]
    fn fresh_control_clicks_preserve_other_programs_and_prompt_input() {
        use crate::main_loop::Program;
        let input = menu::left_press(50, 50);
        let io = Io::new(crate::Files::default());
        for (program, reading, targeting, accept, quiet) in [
            (Program::U, 1, 0, true, true),
            (Program::End, 1, 0, true, true),
            (Program::Game, 0, 0, true, true),
            (Program::Game, 1, 1, true, true),
            (Program::Game, 1, 0, false, true),
            (Program::Game, 1, 0, true, false),
        ] {
            let mut s = command_state();
            s.main.program = program;
            s.main.reading = reading;
            s.ui.input.targeting = targeting;
            s.ui.input.accept = accept;
            let mut p = Popup {
                on: true,
                ..Popup::default()
            };
            let out = p.input_controls(&s, &io, &input, quiet, (0, &[(0, true)]), &[]);
            assert_eq!(
                out, input,
                "{program:?}/{reading}/{targeting}/{accept}/{quiet}"
            );
            assert!(p.quick.is_none() && !p.quick_down && p.run.is_none());
        }
    }

    #[test]
    fn invalid_world_control_press_owns_its_release_across_a_prompt_change() {
        let mut s = command_state();
        let io = Io::new(crate::Files::default());
        let mut p = Popup {
            on: true,
            ..Popup::default()
        };
        // Outside any actionable map cell: Ctrl must not fall through to held-pointer walking.
        let input = menu::left_press(-1, -1);
        let out = p.input_controls(&s, &io, &input[..1], true, (0, &[(0, true)]), &[]);
        assert!(out.is_empty() && p.quick_down && p.quick.is_none());
        s.main.program = crate::main_loop::Program::End;
        let out = p.input_controls(&s, &io, &input[1..], true, (20, &[]), &[]);
        assert!(
            out.iter()
                .all(|e| matches!(e, InputEvent::Pointer { buttons: 0, .. }))
        );
        assert!(!p.quick_down && p.run.is_none());
        // Only a fresh ending click is the ending's input; the earlier world's tail is not.
        assert_eq!(
            p.input_controls(&s, &io, &input, true, (40, &[]), &[]),
            input
        );
    }
    #[test]
    fn representative_commands_use_the_existing_icon_and_target_reads() {
        for code in [menu::USE, menu::TALK, menu::ATTACK] {
            let mut s = command_state();
            let at = (40, 40);
            let m = Menu {
                kind: Kind::Map,
                cell: Some((4, 4)),
                underlined: None,
                sections: vec![menu::Section {
                    object: 2,
                    kind: None,
                    name: String::new(),
                    entries: vec![Entry {
                        code,
                        reason: None,
                        theft: false,
                    }],
                }],
                go_here: None,
            };
            let mut p = Popup {
                on: true,
                control: true,
                last_pointer: Some(at),
                quick: Some(Open::new(&m, at, &s, &[])),
                ..Popup::default()
            };
            let mut out = Vec::new();
            assert!(p.quick_pointer(&s, (0, POINTER_LEFT), true, &mut out));
            assert!(out.is_empty(), "down captures without executing");
            assert!(p.quick_pointer(&s, (POINTER_LEFT, 0), true, &mut out));
            assert_eq!(
                &out[1..],
                &menu::icon_press(code),
                "the original command icon"
            );
            let run = p.run.expect("original target run");
            assert_eq!(run.code, code);
            assert_eq!(run.at, at);
            assert_eq!(run.step(&s, true), Step::Hold);
            s.ui.input.target_pending = 1;
            s.ui.input.target_command = code;
            s.ui.input.accept = true;
            s.ui.input.in_wait = true;
            assert_eq!(
                run.step(&s, true),
                Step::Release(menu::left_press(at.0, at.1))
            );
            let mut disabled = m.clone();
            disabled.sections[0].entries[0].reason = Some(Reason::Far);
            p.quick = Some(Open::new(&disabled, at, &s, &[]));
            p.run = None;
            out.clear();
            p.quick_pointer(&s, (0, POINTER_LEFT), true, &mut out);
            p.quick_pointer(&s, (POINTER_LEFT, 0), true, &mut out);
            assert!(p.run.is_none(), "a disabled command starts no target read");
            assert!(
                out.iter()
                    .all(|e| matches!(e, InputEvent::Pointer { buttons: 0, .. })),
                "no command press bypasses its reason"
            );
        }
    }
}
