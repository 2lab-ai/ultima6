---
title: ultima6 — 17 context menu on a right press
type: spec
tags: [ultima6, ui, input]
---

# 17 — Context menu

Status: shipped (2026-10-27, deploy 30, main bd3e670). The contract below is the implemented one, not a future
target. Item 2 of `objects-menu-fog-motion/ssot.md`. Research: round 1 unit M1
(`~/Games/ultima6/work/dispatch/r1027-m1/menu.md`, code at `587d973`); build: round 2 unit M2; live check:
`objects-menu-fog-motion/loop.md`, deploy 30.

## Measured facts (main 88eb5b0, re-read at 587d973)

- The page sends the right button to the engine (`sendPointer` in `site/app.js`, `buttons & 3`) and suppresses the browser menu
  on the canvas (its `contextmenu` listener).
- In the original a right press on the map with a command underlined repeats that command on the cell (0xa9); with
  none it walks like the left button (`crates/u6-core/src/ui/click.rs:116-148`). On the command bar a right press only
  underlines (`:167-179`); on the panels it repeats the underlined command on the object under the pointer
  (`:181-197`).
- Every map command takes its selection from 0x105f7c (`ui/cursor.rs:55-75`): the cell under the pointer, then
  0x1225c0 (`combat/leaves.rs:159-192`): the first visible live actor in the cell's location list, else the first
  visible object, else the first hidden one; -1 for a cell that shows nothing. Each command then resolves that
  selection its own way: Talk, Look, Move and Attack act on it (a horse's rider for Talk and Look); Get and Use act on
  the object under an actor or a cover (`get.rs:114-117`, `usecmd/dispatch.rs:100-128`).
- Use's type gate is the 85-word table A4-0x66dc (0x128b6a, `objects/commands/keys.rs:27-37`); it equals the type set
  of the port's Use switch (`usecmd/cases.rs:13-123`), both directions.
- Commands that refuse before reading a target (aboard: Cast, Get, Drop, Move `main_loop/commands.rs:588-598`; Attack
  `combat/attack.rs:166-187`; Cast without a book, with the negate timer, with no spells `magic/cast.rs:72-96`,
  `:171-191`) return to the prompt; a target press queued in advance would then walk the party (`ui/click.rs:141-149`).
- Wide (spec 13 Decision 4): a non-Look command takes a map target only inside the 11 × 11 window round the leader
  (`ui/click.rs:127`, `:156-165`); Look takes any cell of the view.
- Escape at the prompt passes a turn (`main_loop/commands.rs:376`).

## Contract

**When.** A right press on the map or on an inventory or equipment slot (Wide's slots included) opens the menu only
at the command prompt (`at_prompt`). Everywhere else the right press does what it does today: it confirms a pending
target, underlines on the command bar, passes on the rest of the screen, and keeps its current effect in a
conversation, at a more prompt, at Drop's count or spot and at Move's direction. Left-button walking and the arrows
are unchanged; right-button walking on the map at the prompt is gone (the menu replaces it).

**What it lists.** One section per resolved object, headed by its display name (`objects::shown_name`, locale aware;
never an actor's own name), in cell order: the selection's section, then the object under it. Each entry is a command
that resolves to that object. Map: Attack, Cast (the book's current spell, on this cell), Talk, Look, Get, Move, Use.
Panel: Look, Drop, Move, Use, and the slot's own left-press action (ready, open, take off). The underlined command,
when there is one, is the first entry. Rest and Combat appear only as that first entry.

**Tiers** (M1 §2): a condition of the target's kind hides the entry (no Use on a type outside the 85, no Talk to
oneself, no Get of an actor, no Move, Drop or Ready of type 0x4c, which the object moves refuse at the prompt:
`move_to` always, `move_into` unless a body transfer sets its flag, objects/moves.rs). A situational, pure condition
(a read of the current state: out of reach, too heavy, aboard, outside Wide's window, no free hand, no book, negate
timer, no spells, level, points, reagents, solo, a blocked door, a locked door or chest (Use, reason Locked), readied
by another member, a case's own refusal) disables it with a reason code the page words. A random draw
or a later prompt never disables. Get of what is not the party's is marked as theft. An invisible selection gets no
section (the cell shows as its tile with Look only); a cover keeps its section.

**Running a choice.** The original's own mouse flow: a left press and release on the command's icon, then a left
press and release at the target, the target press released only once the command reads its target: A4-0x7a32 is 1,
A4-0x7a2e is the code, `ui.input.accept` is true, no queued events, and the engine is blocked inside the target
read (a port-only flag set at `wait_event`'s entry, cleared on return). If the engine comes back to the prompt
first, the target press is dropped. No underline is written and the savegame is untouched. Rest, Combat and the
slot's own action are a single press.

**Engine API (port-only, `u6-web`).** `Game::context_menu(x, y) -> String`: `""` unless at the prompt and on the map
or a slot; else JSON `{kind: "map"|"panel", cell, underlined, sections: [{object, name, entries: [{code, enabled,
reason, theft}]}]}`, computed read-only on a clone of `State` through the ported resolution functions.
`Game::command_at(code, x, y) -> bool` queues the icon press now and holds the target press for the gate.

**Predicates.** Each situational condition is a small function beside its handler citing the original's lines, with
a test that runs the real handler on a clone and checks enabled ⇔ the handler reached its action.

**Page.** An HTML menu inside `#screen-box` at the press point, flipped and clamped inside the box; works in Original,
HD, Wide and fullscreen. While open it takes focus and swallows pointer events until the right button is up; Up/Down,
Enter/Space, Home/End, the command letters, Escape and an outside press work, and Escape and the outside press never
reach the game. Labels and reasons are the page's own (`site/i18n.js`, en and ko); object names come from the engine
at run time. Disabled entries are greyed with the reason under the label (`aria-disabled`).

## Acceptance

1. Right press on a closed door next to the leader → a menu with the door's name, Look and Use enabled (Attack last);
   choosing Use opens the door exactly as `U` then a left press on it does (same state after, same RNG).
2. Right press on a door three cells away → Use shown, disabled, reason "far"; nothing is printed and no turn passes.
3. Right press on a villager on a chair → two sections: the villager (Talk, Look, Move, Cast, Attack; Cast is the
   cell's, as for every map selection), the chair (Get). The
   chair has no Use: its type (0xfc) is not among Use's 85, a condition of the kind, so the tier rule hides it
   (amended 2026-10-27 after M2 r3 measured the real chair; the first draft listed Use against its own rule).
4. Right press on an inventory object → the panel menu with its ready/open/take-off action, Look, Drop, Move, Use.
5. Cast without a book: the entry is disabled; if forced through the engine, the target press is dropped and the
   party does not walk.
6. Escape or an outside press closes the menu and passes no turn; a held right button moving over the map sends no
   press to the engine.
7. Wide: a non-Look entry on a cell outside the 11 × 11 window is disabled, reason "window".

## Not settled

- The cost of one `State` clone per right press (estimate: a few MB, once per press, not per frame).
- Talk's script-presence test without loading the script: until an index lookup exists, the slot/type part decides
  and a scriptless NPC prints the original's refusal.
