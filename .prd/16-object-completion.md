---
title: ultima6 — 16 object completion (stubs, stale seams, wrong ports, untranslated lines)
type: spec
tags: [ultima6, objects, port, i18n]
---

# 16 — Object completion

Status: in progress (2026-10-27). Item 1 of `objects-menu-fog-motion/ssot.md`; findings from round 1 units O1, O2 and O3 (`objects-menu-fog-motion/loop.md`).

## Measured facts (main 88eb5b0)

- `port-map.md` reports 3 stubbed game functions. `tools/rea/port_map.py` ranks ported above stubbed (`:40`) and records a `todo_stub!` only under its own address (`:112`). A stub whose address is ported elsewhere therefore disappears from the map. The game code holds such stale seams:
  - `actors/walk.rs:531`: the portcullis (0x1301e2, ported as `objects/effects.rs::toggle_passthrough`);
  - `actors/party.rs:441`: a carried object of a type (0x10b18c, ported as `objects/iter.rs::find_child`), which by the stub's own comment means swamp boots never protect (the code's claim; O1 traces the callers);
  - `actors/turn.rs:139`: the end of solo mode (0x104094, ported as `main_loop/main_hunk.rs::end_solo`);
  - `combat/seams.rs:95`: a staff or wand cast (0x11a586, ported as `magic/dispatch.rs::dispatch`, but async).
- Stubs whose routines are not ported:
  - `objects/commands/push.rs:117`: Move of a carried object to someone (0x12ae04);
  - `usecmd/misc.rs:435`: the serpent figure (0x12ff50);
  - `actors/npc.rs:585`: the confiscation on arrest (its stub names 0x10c928, which the map lists as `contained_in`; the address is to be checked);
  - `combat/seams.rs:68`: the remains (0x10c6a6);
  - `combat/seams.rs:140`: the overview sight, mode 2 of 0x109c12;
  - `usecmd/seams.rs:233`: the ending (0x133d1e, 0x13513a).
- Out of scope: the debug teleport (`main_loop/commands.rs:436`, 0x104140).

## E1 — ending program (2026-10-29)

Read-only research of the owner's `end` executable found 16 hunks, a 54,956-byte loaded
image and 231 recognized functions. The scene driver is end `0x103a1a`; passages are
`0x102850`, composition `0x1038d0`, input `0x103908`, and dissolve `0x10a1d6`. Much of the
image is the same planar graphics, font, C runtime and MED support already used by `u`.
The port reuses `intro::mem` and `intro::text`. It reads every passage, duration word,
palette, song filename and picture from the owner's files at runtime. This executable has
a separate address space from the game string registry.

The cube now validates `end`, `end.new` and `u6.set` before removing the shrine object.
Missing or malformed resources leave the object and game running. A valid use invokes
game `0x133d1e`, waits for the original message key, then requests game exit 2 through
`0x13513a`. The command dispatcher returns before another turn or movement-point charge.
Launcher state 2 (ultcli `0x1004e6`) runs `Program::End` once, then finishes; both the normal
launcher and native `--game-first` reach it.

The scene retains the original 320×200 composition in both layouts, with all 16 passages,
sprite animations, five dissolves, songs 10 → 11 → 12 → stop, and the last input wait.
Keyboard and either mouse button advance; mouse release ends a held click. End's copper
initialization (`0x10d342`) points to an empty sprite, so the host hides the old game sprite.
The game/map source plane is cleared and native Wide presentation centers the scene with
black margins. Original routines `0x109656` and `0x109680` are empty; no speculative star
animation was added.

Two details were verified against assembly rather than inferred from decompilation:

- Game `0x13522a..0x13524a` binds argv 2/3/4 to day/month/year buffers. Ultcli passes the
  same buffers to end, which formats year/month/day. Distinct 1/2/3 values cover this wiring.
  The original unsigned day-borrow comparison and 13-month adjustment are preserved.
- End's last dissolve sample omits the rectangle's x origin and copies screen x=0; regular
  samples add x=106. A regression preserves this original one-pixel behavior.

Tests construct the end conditions from the owner's disk state at runtime, then write an
optional private save fixture outside Git for real launcher/browser/native checks. This is
explicitly a constructed fixture, not a supplied completed playthrough save. Checks cover
invalid assets, incomplete conditions, handoff, every passage, frame progression, source
plane/pointer cleanup, music order and non-silent host audio, final exit, and deterministic
restart without changing the original inputs. Browser receipts include setup, `u`, loading
the constructed save, actual Use input, end progression with keys and clicks, and reload.
`tools/web/live/ending.mjs` only serves the local build and keeps screenshots outside Git.
An actual user's completed save remains a separate validation dependency; original artwork,
text, executable bytes, decoded resources and save files must never be committed.

## B13 — compiler-enforced game string registry (2026-10-29)

Recount on integration 7147fe3, production `game` code only (Rust AST, no test items,
`intro` and locale catalogue internals excluded): `GameImage`/`GameData.cstr` calls 9,
`game_string` 27, `a4_string` 7, `pointed_string` 10, `pointed` 6,
`a4_pointed` 5, `a4_pointed_list` 1; image argument variants 3. The 38 `printf`
calls take bytes, so are output sinks, not image readers. The earlier 32/31/46 numbers
mixed a different scope. Address-forwarding print wrappers are included in the migration;
the full form/site inventory is a local code-only receipt, not another maintained parser.

One `string_registry` module owns registration, reading and catalogue traversal. A record
selects a fixed address or bounded pointer-table entry and gives its catalogue disposition:
printed message/format, displayed label with its existing cell budget, or explicit exclusion
with a reason. Existing address lists are registration inputs in their current order; they
must not be copied into a second whitelist. Both catalogue traversal and runtime handle
construction use those same records. Private-field `Text` handles carry only registered
sources. Fixed addresses are checked in const contexts; dynamic slots must belong to a
registered table and stay within its bounds. Arbitrary raw address readers are not public
escape hatches. Forwarding wrappers take handles rather than numbers.

Clippy disallows the raw executable text readers outside the registry. A new direct or
aliased call must fail the compiler gate; an unregistered fixed handle must fail const
evaluation, and an out-of-range dynamic table entry must fail without reading bytes.
The old custom Rust-source scanner and its syn/proc-macro2 dev dependencies are removed.
The existing platform-free disallowed type/method rules remain. Test fixtures may explicitly
read synthetic or local reference strings; these exceptions are scoped to test code.

`u` (intro) retains its own catalogue and reader, separate from `game`. Its hunk loader's
song-file read has a narrow documented exception. The ending program's own `Exe` is also a
separate executable boundary, not permission to use a game-image text reader elsewhere.
The game conversation message table also lives in this registry, with explicit display
order or exclusion reasons and its original hunk-wide NUL bound. Its public MESSAGES/SHOWN
views are generated from those records. Generic byte-printing, conversation script bytecode
and binary image/table reads are not banned.

Compatibility: catalogue content, insertion order, label budgets, unlisted-string counting
and locale identity digest must remain unchanged. Preserve the raw-reader semantics and all
format/control bytes. Verify using content-free synthetic tests plus owner-data equality,
Original/Wide fmt and Clippy, and deliberate compiler RED controls. No game text belongs in
committed receipts or failure messages.

Review hardening: selectors restored from a save are not assumed valid. The date-line wind
byte is preserved on save/load; only -1 and 0..7 have registered names. Other values display
an empty field, without an unregistered read or panic. A synthetic encode/decode/engine test
covers all 256 bytes and the normal labels. Place-name selection also uses an empty fallback;
remaining table assertions iterate only compile-time mode/song ranges or registry-owned slots.
