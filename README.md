# ultima6 — Ultima VI (Amiga) reverse-engineered and ported to Rust

Play: **https://ultima6.2lab.ai** — bring your own copy of the game. The page asks for the
archive.org `ultima-vi.zip` (Amiga flux dump), the three ADFs, or the extracted files; they are
decoded in your browser, kept in its IndexedDB for the next visit, and never uploaded
(`connect-src 'self'`, no third-party requests). This site hosts no game data.

Ultima VI is © Origin Systems / Electronic Arts. This project is not affiliated with or endorsed
by EA. See `.prd/01-spec.md` "Rights boundary" for what this repository does and does not contain.

## Status (milestone 1, 2026-10-14)

Works: boot into Britannia from the shipped new-game state, the full Amiga screen (map window,
party panel, message scroll, command bar), walking, passing a turn, party/solo mode, NPCs
following their schedules as the clock runs, talking to NPCs (portrait + the original
conversation scripts). Not yet: combat, magic, Look/Get/Drop/Move/Use, inventory, save/restore,
music, mouse, the intro and character creation — milestone 2 is in progress
(`.prd/browser-port/loop.md`).

Fidelity check: the original game running in an Amiga emulator (vAmigaWeb + AROS, `tools/oracle`)
and this port agree on 98.8% of the start screen by palette index; the differences are listed in
the loop file.

## How it was built

1. **Disk intake** — flux dump → HFE → Amiga MFM → ADF → AmigaDOS OFS → PowerPacker (`crates/u6-intake`).
2. **Reverse engineering** — the 68000 executable `game` (35 hunks) flattened with
   `tools/hunk2raw.py` and analysed with [REA](https://github.com/morluto/rea) + Ghidra 12.1.4, using
   an `m68k` raw-binary architecture added to REA for this project. All 1,383 procedures were
   decompiled locally (`tools/rea/dump-all.mjs`) and classified (`tools/rea/classify.py`,
   `re/notes/functions*.md`, `re/notes/subsystems.md`). Data formats: `re/notes/formats.md`,
   `re/notes/graphics.md`. Decompiler output is never committed.
3. **Port** — `crates/u6-core`: one Rust function per original function, each citing its address
   (`/// Original: 0x…`); `re/notes/port-map.md` tracks every address (ported / replaced / stubbed).
   Game tables, strings and the palette are read at run time from the user's own files.
4. **Platforms** — `crates/u6-native` (window, headless PNG runner), `crates/u6-web` + `site/`
   (wasm, Cloudflare Pages).

## Build

```sh
scripts/cargo.sh test --workspace --profile gate  # release + overflow and debug-assertion checks; the wrapper pins rustup stable
scripts/cargo.sh run -p u6-native --release -- --zip ultima-vi.zip
bash scripts/build-web.sh && node tools/web/test-web.mjs
scripts/deploy-web.sh main                          # Cloudflare Pages project ultima6
```

## Layout

| Path | What |
|---|---|
| `.prd/` | spec (01), architecture (02), core port contract (03), P3 refactor plan (04), loop |
| `crates/u6-intake` · `u6-formats` · `u6-core` · `u6-render` · `u6-native` · `u6-web` | Rust workspace |
| `re/notes/` | reverse-engineering notes (formats, graphics, functions, subsystems, oracle, port map) |
| `tools/` | PP20, hunk flattener, REA drivers, Amiga emulator oracle, web test harness |
| `site/` | the static page |
