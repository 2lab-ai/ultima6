---
title: ultima6 — 11 HD font (the pixel font or an HD font, chosen at the first run)
type: spec
tags: [ultima6, hd, font, korean, text]
---

# 11 — HD font: text drawn at the HD layout's own size

Status: in progress (2026-10-22). User item 2 of `hd-layout-font-register/ssot.md`, verbatim: "hd 해상도에 맞게 한글 폰트 개선해줘. 처음 설정에 니가 만든 기본 폰트랑 hd 화면에 맞는 적절한 폰트 사용해줘." It builds on 10 (the layout) and 08 (the Hangul cells).

## Measured facts (main 6e4e337)

- In the game program every character is one 8 × 8 cell drawn whole: set pixels in the window's ink, clear ones in its paper, colours 0..15 (`ui/text.rs:118-132` `draw_glyph`/`draw_rows`, `:140-157` `draw_char`). A code below 0x100 is a glyph of `u6.ch`; a Hangul syllable U+AC00..=U+D7A3 is its cell of the Hangul table (Galmuri7, OFL, `site/fonts/galmuri7-hangul.bin`, made by `tools/font/galmuri.py`, ink in rows 0..=6). The pixels are written with `Framebuffer::set`, so their source is classic (`u6-render/src/lib.rs:530`), and the HD compositor shows each one as an S × S block of its colour (`u6-render/src/hd.rs:686-780`): blocky text at HD.
- Scrolling a window moves pixels with their sources (`ui/text.rs:160-188`, `copy_px_within`).
- `u` (the opening, the story, creation) draws its text into the emulated chip planes (`intro/text.rs:195-250`, `intro/mem.rs:199`), which carry no source plane.
- `Src` is 12 bytes: `ver`, `id: u16`, `u`, `v`, `kind`, `flags` (`lib.rs:89-104`); `flags != 0` means the compositor shows the classic pixel.

## Design

1. **Engine: glyph sources.** `Kind` gains `Glyph` (4). `draw_rows` writes each pixel with `set_src`: kind `Glyph`, `id` the cell code (`u6.ch` code or the syllable's code point, both under 0x10000), `(u, v)` the pixel's place in the cell, `flags` a new `FLAG_INK` (4) on set pixels. The pixel colours are unchanged, so every parity receipt is bit-identical, and native and the tests still allocate no plane (07 Acceptance 1).
2. **Compositor: the HD font mode.** `set_hd_font(on)`. Off (the pixel font): a glyph pixel is shown as a classic pixel, as today. On: a cell is an HD glyph when all 64 of its pixels are `Glyph` sources of the same code with `(u, v)` running 0..7 over the cell, and its code is printable ASCII (0x21..=0x7e) or a Hangul syllable; its 64 blocks are filled with the cell's paper colour and the cell is listed. Any other glyph cell (the space, runes, the game's own symbols, a cell under the pointer sprite, which is drawn as classic pixels) stays classic. `hd_glyphs()` gives the list of the last composition: per cell its screen x and y, its code, its ink and its paper as RGB.
3. **Page: drawing the HD glyphs.** After the layout (10), each listed cell is drawn on the visible canvas at its destination rect: the character, centred, in the chosen HD font at about 0.85 of the cell's height, in its ink colour. The page waits for the font (`document.fonts.load`) and draws the pixel font until it has it.
4. **The HD font.** Gowun Batang (고운바탕, SIL OFL 1.1, Google Fonts), subset to printable ASCII and the 11,172 Hangul syllables, as WOFF2 under `site/fonts/` with its licence beside it, as Galmuri7 is. Chosen for the game's archaic register (08): a book face, readable at the message scroll's 24 to 30 px.
5. **The choice.** The first-run chooser (`#setup`) and Settings gain "Font: HD font / Pixel font" ("글꼴: HD 글꼴 / 기본 픽셀 글꼴"), HD by default, kept in `localStorage` (`u6.font`). The HD font applies only with the HD layout (10); with Original or HD off the pixel font is drawn.
6. **Not in this unit:** `u`'s text (the opening caption, the story, the gypsy, creation) stays in the pixel font until a source plane reaches the chip planes (follow-up F2).

## Acceptance

1. Synthetic tests: a window's text gives `Glyph` sources with `FLAG_INK` on ink pixels and unchanged colours; a scroll keeps them; a cell half overwritten by a tile or by a classic write is not listed; a Hangul and an ASCII cell are listed with their ink and paper; the space and a code under 0x21 are not.
2. HD font on: the message scroll and the side panel show the HD font in Korean and in English, at the layout's scale; off, they show the pixel font exactly as before.
3. The chooser and Settings keep the choice across a reload.
4. Gates pass; live check on the deployed site: a fresh visitor, the zip only, Korean, sees the HD font in game and can switch to the pixel font.
