---
title: ultima6 — 03 core port contract
type: spec
tags: [ultima6, rust, port, wasm]
---

# 03 — Core port contract (u6-core)

Status: in-progress (rev 2, 2026-10-14 — gpt6 spec review CHANGES ×8 applied)

How the original 68000 `game` becomes Rust. Inputs: `re/notes/subsystems.md` (boot chain, main
loop 0x1041d8, dispatcher, milestone-1 set of 320 game functions + 41 runtime), the
`functions-*.md` tables, `re/notes/formats.md`, `re/notes/graphics.md`.

## 1. Control flow: async port of a blocking program

The original blocks inside its command loop (key wait 0x10790a → 0x106bbc, delays, disk
prompts). The port keeps that shape: game code is `async fn`, and every place the original waits
on input, time or the display is an `.await` on the platform.

One retained, pinned game future preserves the original blocking call sequence. The host polls it
once per host tick, including while waiting; a no-op waker is valid only under this unconditional
polling contract. Input callbacks enqueue events and never poll or call game logic reentrantly.
Awaiting input consumes queued events in order; awaiting a delay uses a monotonic deadline;
awaiting VBlank requires a later display generation, not merely another poll. Every potentially
unbounded loop must reach a guaranteed yield or a documented bounded-work checkpoint that
preserves simulation order. The engine records task completion and never polls a completed
future. Wait-bearing functions and their transitive callers are async; recursive async call
cycles use explicit indirection (`Box::pin`) at the cycle edge. No blocking sleep, busy wait, or
browser-side blocking executor.

```rust
pub struct Engine { /* State, Io, the pinned task, the published frame */ }
impl Engine {
    pub fn new(files: Files) -> Result<Engine, BootError>;
    pub fn tick(&mut self, input: &[InputEvent], elapsed_ms: u32); // enqueue, advance time, poll once, publish frame
    pub fn frame(&self) -> &Framebuffer;                          // published copy, 320×200, 32 colours
}
```

Borrow rules: no `Ref`/`RefMut` guard into shared state or I/O survives an `.await`, a nested
helper that may borrow the same cell, or an external callback; copy IDs or values before waiting
and reborrow afterward. The game owns mutable drawing buffers through `Io`. After polling returns,
`Engine` publishes a completed frame into storage it owns directly; `frame()` refers only to that.
Presentation points preserve the original draw/swap order: prompts and intermediate frames become
visible before their waits. Display time and animation are distinct from the game calendar and
actor turns; host ticks alone do not advance turns.

Gate before fan-out: a nested prompt with buffered input, a delay and repeated VBlank waits,
rendered while suspended and resumed without borrow panics or skipped frames, deterministic
across host tick sizes, in native and wasm runners. The VBlank server 0x14f27a is read before any
timing-parity claim.

## 2. State: the original globals as one struct

`State` owns every game global, grouped by the original tables (subsystems.md "Global tables"):
object location (0x135588, 3 bytes/object), type+frame (0x137988), worktype (0x139188),
schedules (0x139988 + first-entry table), party list/size/leader position, clock
(hour/day/month), movement points, map chunk cache, view flags, message scroll, panel mode.
Field doc comments cite the original address. The loaded data (u6-formats types) is the
initial value.

Ownership: WU-C0 owns the `State` aggregate, `Engine`/`Io`, RNG, load orchestration and shared API
integration. Domain state types and their loaders live in the owning module: objects (identity,
location, type/frame, container relations), actors (actor-only attributes, movement points,
schedules/worktypes, party, calendar; also the hunk-13 leaf helpers), map (chunk caches, view
flags), ui (message scroll, panel/selection state), converse (interpreter state). `Io` owns input
queues, host timing and drawing buffers. Actors refer to canonical object IDs and never duplicate
object positions; any duplicated original global kept for fidelity documents its sync rule. Only
WU-C0 changes the aggregate after the skeleton merges; other units extend their own domain types.
Signatures across the named cycles (ui↔objects, map↔objects, actors↔combat) are frozen before
parallel work.

Saves capture authoritative game state and RNG only at the original save boundaries; they never
serialize futures, borrow guards, I/O handles or rebuildable caches. Restore invalidates pending
interactions and model replies.

## 3. Porting rule per function

- One Rust fn per original function in the milestone set, same name shape
  `fn_<hexaddr>_<role>` is NOT used in public API — public API gets real names; each fn carries
  `/// Original: 0x10996c (map: tile at x,y,z)` as its first doc line. `re/notes/port-map.md`
  (generated) maps every original address → Rust path → status (ported / stubbed / replaced).
- Behavior follows the original: same order of checks, same RNG calls (port 0x151ee4/0x151f06
  bit-exactly so runs are reproducible), same integer widths where overflow matters.
- No decompiler output pasted into source. Write idiomatic Rust from understanding.
- Stub points (subsystems.md, 45): every stub declares its argument/result contract, expected
  side effects, caller behavior, and whether an accepted M1 scenario reaches it. There is no
  universal neutral return: `todo_stub!("0xADDR name", fallback)` takes an explicit fallback,
  logs the first hit and counts hits (exposed by `Engine` for receipts). Required M1 paths may not
  hit undeclared stubs. Unsupported paths stop the affected operation visibly and safely; an
  omitted NPC worktype must not reschedule an actor forever without progress. Each stub is a row
  in port-map.md.

## 4. Platform replacements

| Original | Port |
|---|---|
| SAS/C runtime (41 fns) | Rust std |
| dos.library file layer (hunk 21) | `Files` (in-memory map from u6-intake) |
| input.device handler 0x151b08 + raw key map 0x151d00 | `InputEvent` queue; the raw-key → game-code table is ported |
| copper/bitplane display (hunk 34), blitter (0x153002/0x152ffa) | chunky 320×200 `Framebuffer` + two buffers; tile blit ported to chunky pixels with the same clipping and masking. `u6-render` = platform-free framebuffer/blit primitives; `u6-core/gfx` = world-view orchestration on top |
| MED music (hunk 0, 7) | stub in M1 |

## 5. Extension seams for P3 (kept cheap now, implemented later)

- **NPC turns**: P1 keeps the original NPC-turn/worktype routine behind one named entry point
  (0x1200b0), preserving scheduler order, side effects and RNG draws; it is not decomposed into a
  pure policy before parity is shown. The P3 boundary is suspendable: build an owned NPC view and
  legal-action list, release state borrows, `await` a brain response, validate it, apply it
  through the original action path. Requests carry a turn/state generation so stale or cancelled
  replies cannot mutate a restored world. `OriginalBrain` stays the default; a model reply never
  writes State directly; timeout, invalid reply and cancellation are explicit and tested; building
  the action list consumes no simulation RNG and mutates nothing.
- **Data, not constants**: each original table or embedded content item has a documented runtime
  source — an existing user-supplied data file, or extraction from the user-supplied `game`
  executable (as the palette already is, graphics.md §2). Extraction runs locally in the user's
  browser, validates the input version and ranges, and produces typed definitions; extracted
  content is never compiled into the engine or hosted. Algorithmic and binary-format constants
  with documented meaning may stay in code. Editable gameplay definitions are separate from
  mutable session state. The source of every M1 table is recorded before its loader is assigned.
- **Mods**: P3 applies validated, versioned data overrides to those definitions by stable
  identity; raw file replacement (layered `Files`) stays supported but is not the only editing
  interface. Overlay order and paths are deterministic, malformed overrides fail visibly, saves
  record the effective base/mod identity. No script runtime or plugin ABI in P1.

## 6. Module layout (disjoint ownership for parallel work units)

```text
crates/u6-core/src/
  engine.rs     executor, Engine, Io (WU-C0)
  state.rs      State + loading from u6-formats (WU-C0)
  rng.rs        0x151ee4 / 0x151f06 (WU-C0)
  gfx/          tile blit, map window draw 0x10202e/0x102c90, view refresh 0x102ef0 (WU-P1)
  map/          tile lookup 0x10996c, region load 0x108ac4, view flags 0x109e74 (WU-P2)
  objects/      object list/iterate/move/remove/names (WU-P3)
  ui/           message scroll + printf 0x108658, panels 0x103f64/0x110d2c, key read 0x10790a (WU-P4)
  actors/       turn scheduler 0x1213de, NPC turn 0x1200b0, schedules 0x121826, party step/follow, clock 0x10368a (WU-P5)
  converse/     Talk 0x1127c4, interpreter 0x1130a2, script/portrait load (WU-P6)
  main_loop.rs  boot 0x1351d8 + command loop 0x1041d8 (WU-P7, last)
```

## 7. Milestone-1 acceptance

Native headless runner and the browser both: boot from the user's disks → world map view
with party and objects drawn, side panels, message scroll; arrow keys walk the party; NPCs move
on schedule as the clock advances; `T` + direction talks to an NPC and shows portrait + text.
Receipt = frames read by the dispatcher (headless PNG sequence + live-site screenshots), not
test counts alone, plus deterministic behavior comparisons against the original (same initial
state, controlled RNG, same input sequence): command boundaries, actor order/positions/movement
points, clock transitions, RNG state, dialogue progression, stub hits. Scenarios: panel
navigation without a game turn, multi-step Talk input, a schedule transition, blocked movement,
a nested wait. Expected behavior comes from original observations (oracle) or inspected
routines, never from the port itself.
