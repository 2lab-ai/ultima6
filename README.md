# ultima6 — Ultima VI (Amiga) reverse-engineered and ported to Rust

Play: **https://ultima6.2lab.ai** — bring your own copy of the game. The page asks for the
archive.org `ultima-vi.zip` (Amiga flux dump), the three ADFs, or the extracted files; they are
decoded in your browser, kept in its IndexedDB for the next visit, and never uploaded
(`connect-src 'self'`, no third-party requests). This site hosts no game data.

Ultima VI is © Origin Systems / Electronic Arts. This project is not affiliated with or endorsed
by EA. See `.prd/01-spec.md` "Rights boundary" for what this repository does and does not contain.

## Status (milestone 2, 2026-10-15)

Works: the launcher as on the Amiga — the opening, the menu, character creation, the
introduction, the transfer of an Ultima IV/V character, Journey Onward — and the game: the full
screen, walking by keys or mouse, NPCs on their schedules, conversations, combat, all spells,
Look/Get/Drop/Move/Use, inventory with Ready/Unready, Rest, Save/Restore (the savegame is the
original's format), music and sound effects. The port map (`re/notes/port-map.md`) lists the
few routines still stubbed or planned.

Fidelity checks against the original running in an Amiga emulator (vAmigaWeb + AROS,
`tools/oracle`): the start screen agrees on 98.8% by palette index, the menu and the inventory
panel on 100%; timing is taken from traces of the original — a fight turn in the start scene
takes 301 PAL frames against the original's 277, a quiet step in the castle 31 against 33.
Open items are in `.prd/browser-port/loop.md`.

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
scripts/cargo.sh run -p u6-native --release -- --zip ultima-vi.zip  # the window: - / = (or numpad -/+) slower / faster, 1x to 8x and Max, shown in the title
bash scripts/build-web.sh && node tools/web/test-web.mjs
tools/speech/fetch-models.sh                        # the voice model into site/models/ (gitignored), before a deploy
scripts/deploy-web.sh main                          # Cloudflare Pages project ultima6
```

## Layout

| Path | What |
|---|---|
| `.prd/` | spec (01), architecture (02), core port contract (03), P3 refactor plan (04), HD/4K output design (05), fixed tick and speed (06), HD art (07), Korean i18n (08), NPC voices (09), loops |
| `crates/u6-intake` · `u6-formats` · `u6-core` · `u6-render` · `u6-audio` · `u6-native` · `u6-web` · `u6-testkit` | Rust workspace (`u6-audio`: the emulated Amiga audio machine; `u6-testkit`: content-free comparisons for the real-data tests) |
| `re/notes/` | reverse-engineering notes (formats, graphics, functions, subsystems, oracle, port map) |
| `crates/u6-hdpack` | the HD pack format `u6-hdpack/1` (spec 07 HD1): writer, strict reader, the drop classifier, the content hash of a classic image; no game data. The page loads a dropped pack (H2), keeps it in IndexedDB and draws its pictures in front of MMPX |
| `tools/` | PP20, hunk flattener, REA drivers, Amiga emulator oracle, web test harness |
| `tools/hd` | `u6-hd`, the local imagegen HD tool (spec 07 HD2): run by the owner on the owner's files; its sheets, outputs and packs stay in `~/Games/ultima6/hd/`, outside the repo. It moves each generated image out of `~/.codex/generated_images/`; codex's own session log (`~/.codex/sessions/`) still keeps every sheet that was sent. The example `scaler_pack` writes a full-size pack of MMPX ×4 of the owner's images there, for timing the page |
| `tools/i18n` | `translate.mjs` (spec 08, slice K1): run by the owner on the NPC bundles the page downloads, for one script (`--item a:N`) or the first N; it sends that script's lines through his llmux (gpt-6-astra by default) after a preview and a typed `yes`, checks each answer with the mechanical gates and writes `~/Games/ultima6/i18n/ko/ko.u6lp` (dropped on the page, used in 한글) and a count-only coverage receipt; `style-ko.md` is the 사극체 register guide it sends |
| `tools/speech` | `fetch-models.sh` (spec 09 V2): the pinned Supertonic-3 revision and onnxruntime-web 1.30.0, checked against their sha256 pins and hosted in parts of at most 24 MiB under `site/models/` with their licences and notices; `--check` checks them as hosted |
| `site/` | the static page; `site/voice/` its on-device voices (spec 09 V3: the worker, the player, the model's terms) |
