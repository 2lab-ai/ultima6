---
title: ultima6 Amiga tile graphics and palette (REA of game)
type: note
tags: [ultima6, re, amiga, graphics, tiles, palette, rea]
---

# Amiga tile graphics and palette

How the Amiga `game` executable draws the 2048 16×16 tiles, from REA (Ghidra 12.1.4) of
`~/Games/ultima6/re/game.bin` (load base `0x00100000`). Addresses are image addresses. Facts come
from assembly read via the REA MCP client (`procedure_assembly`, `xrefs`); the bulk pseudocode in
`re/dumps/game/` was used only to find candidates. Every format rule was then checked against the
real files (§11). Decoder: `crates/u6-formats/src/gfx/`. No game bytes are in this repo: the
palette is read from the user's own `game` file at run time (§2), and the values the probes read
out of the files (counts, tile numbers, register values, sizes of variable files) are not
reproduced; the notes say what was checked and that it matched.

## 1. Screen

The game builds its own copper list in CHIP hunk 28 (`0x001519F2`) and points `COP1LC`
(`0xDFF080`) at it in `0x0016236A`. The moves in that list set (checked against the hardware
register meanings):

- `BPLCON0`: **5 bitplanes, 32 colours**, lores, colour on.
- `DDFSTRT`/`DDFSTOP`, `DIWSTRT`/`DIWSTOP`, both modulos 0: **320×200**, 40 bytes per row.
- `BPL1PT..BPL5PT` are filled by `0x0016236A` from one screen base, adding 8000 per plane.
  So the 5 planes sit back to back, 8000 bytes each (320×200/8). Plane `k` is colour bit `k`
  (`BPL1PT` = bit 0).
- 32 `COLOR00..COLOR31` moves start at `0x00151A3E`; their value words are written only by
  `0x00162358` (§2). Sprite pointer moves follow (mouse pointer, `0x001622D6`).

The blitters address rows through a table at `0x00157E9E` that holds `y·40` for y = 0..199.
There are two screen buffers. `0x00152FFA` uses the pointer at `0x00161BE2`, `0x00153002` the
one at `0x00161BDE`. Both run the same code from `0x00153008`.

## 2. Palette

- **Source**: 32 OCS colour words (`0RGB`, 4 bits per channel, big-endian) in the merged data
  hunk 33 at image `0x0015812C` (= `A4 − 0x35C4`, with `A4 = 0x0015B6F0`).
- **Load**: `0x0014EF06` does `lea (-0x35c4,A4),A0` at `0x0014F038` and then calls
  `0x0014F262`, a stub that jumps to `0x00162358`. That routine copies the 32 words into the
  copper colour moves (stride 4, `dbf` count 0x1F).
- **No other writer**: a byte search of the image finds the `-0x35C4(A4)` displacement once,
  at `0x0014F03A`. A scan of the image for every absolute longword into the copper colour moves
  (`0x001519F2..0x00151B05`) or the custom chips (`0x00DFF000..0x00DFFFFF`) finds only these:
  `0x00162358` (colours), `0x001622D6` and `0x0016236A` (sprite pointer, bitplane pointers,
  COP1LC), `BEAMCON0` in `0x00162294`, and DMACON/VHPOSR/audio registers in hunk 0. There is
  also a stored copper-list pointer at `0x001624A6` (no absolute or decompiled reference), and a custom-chip base
  at `A4 − 0x35C8` that is used only to restore COP1LC/COPJMP1 on exit (`0x0014EE8A`,
  `0x0014EE94`). So the palette is static: **no day/night palette, no colour cycling**. The
  animation effects come from §6 instead.
- **Shape**: entries 0..15 are an EGA-like set (8..15 the bright forms of 0..7, §6.1); 16..31
  are further shades. Entries 17..19 double as the sprite (pointer) colours (hardware).
- **Where to read it at run time**: in the PowerPacker-decrunched `d1/INSTALL/game`, the
  `HUNK_DATA` header of hunk 33 sits at file offset 243,036 (`HUNK_DATA`, then the block's
  length in longwords). Its data starts 8 bytes later, and the palette is `+0x4A3C` into that
  data, so file offset **262,048 (`0x3FFA0`)**, 64 bytes. On the disk, `game` is PP20-crunched,
  so it must be decrunched first. `gfx::palette(game)` checks the `HUNK_HEADER` id, the
  `HUNK_DATA` id and that the length field's data lies inside the file and holds the palette
  before it reads.
- 8-bit expansion: a channel nibble `x` becomes `x·17` (`(x<<4)|x`).

## 3. Load sequence (`0x00152E50`, hunk 30)

`0x00152E50` is reached only through the jump stub at `0x00135510` (`4EF9` = `jmp abs.l`). File helpers:
`0x00153360` open (D0 = mode), `0x0015337E` read (A0 = buffer, stack = length), `0x00153366`
close, `0x00153372` load a whole file, `0x00153378` allocate.

1. `maptiles.ega` (string `0x00152DF6`). Read `u16`, byte-swap it into `n`, store `n` at
   `0x00152E4A`. Then read `5·n` bytes into the buffer whose pointer is at `0x00152E38`.
2. `masktype.ega` (`0x00152E1D`): loaded with D0 = 1 into `0x0015759E` (2048 bytes after
   U6-LZW). `tileindx.ega` (`0x00152E10`): loaded with D0 = 0 into the buffer at `0x00155564`.
3. Loop over 2048 entries (`dbf` from 0x7FF). Each `u16` is read low byte first (LE) and turned
   into a longword in the table at `0x0015559E`. A value `v < 0x5FFF` becomes `v << 4` (an
   **object tile** file offset). Any other value becomes `maptiles_buffer + (v − 0x6000)` (a
   **map tile** pointer). Code: `cmp.w #0x5fff` / `bcs` / `subi.w #0x6000` + `add.l A2`
   (`0x00152F0C..0x00152F1A`).
4. `animmask.ega` (`0x00152E2A`): `u16` length `n1`, allocate and read `n1` bytes into
   `0x00152E3C`. Then `u16` length `n2`, allocate and read into `0x00152E40`.
5. Jump through the stub at `0x0015336C` to `0x0014F16C`, the object-tile cache prefill (§5).

The decoder checks the same split differently: by masktype, as the blitter does (§4). It then
requires the index to be in the matching range. Every real entry agrees (§11).

## 4. masktype (`masktype.ega`, U6-LZW → 2048 × u8)

The blitter (`0x00153008`) loads `masktype[tile & 0x7FF]` into `0x0015335E`:

| bits | meaning |
|---|---|
| `& 3 == 0` | map tile, opaque copy (§4.1) |
| `& 3 == 3` | object tile, 16 columns, 2 bytes per row (§5) |
| `& 3 == 2` | object tile, left 8 columns, 1 byte per row |
| `& 3 == 1` | object tile, right 8 columns (screen byte + 1, `0x0015327C`) |
| `& 4` | an animmask reference follows the pixel data (§6) |

No value above 7 occurs (checked). The tile word passed to the blitter has the tile in bits 0..10.
Bits 11..12 select an outline mode (§7).

### 4.1 Map tiles (`maptiles.ega`)

- File = `u16 LE n`, then **5 plane blocks of `n` bytes each** (file size = 2 + 5·n, checked).
- A tile at offset `o` into the blocks (= tileindx `v − 0x6000`) is `[first row][row count h]`
  at `o`, then `h` big-endian row words for plane 0 at `o + 2`. Plane `k`'s rows sit at the
  same position in block `k` (`o + 2 + k·n`): `0x00153072..0x00153084` builds four pointers as
  `A0 + D6`, with `D6 = n` from `0x00152E4A`. The header bytes in blocks 1..4 are padding.
- The blitter copies `h` rows with `move.w` into the five planes (`+0x1F40` per plane). If the
  screen byte is odd it uses byte moves instead (`btst #0` at `0x00153088`), so tiles go on
  8-pixel steps. It **does not touch rows outside `[first, first + h)`**, so those rows are
  transparent. In a row word, bit 15 is the leftmost pixel.
- Masktype `& 4`: a `u16 LE` animation reference follows the last plane-0 row (§6). That is why
  a full masktype-4 tile takes 2 bytes more in block 0 than a plain one (checked).
- Headers (checked): most map tiles are `(0, 16)`; some are `(16, 0)` (nothing drawn) and a
  few are row-sparse. Almost all of the empty ones are exactly the `animdata` targets; their
  image arrives at run time by pointer copy (§6.2). The few others are empty and in no table.

## 5. Object tiles (`objtiles.ega`) and the cache

- Tile `t` with `masktype & 3 ≠ 0` lives at file offset `tileindx[t] << 4`. The cache reads
  **0xCC bytes** per tile: prefill `0x0014F16C` fills up to `0x655A(A4)` slots of 0xCC bytes
  from `-0x6168(A4)` in tile order. On a miss, `0x001623D6` evicts a slot (LRU counters at
  `0x00161BCC`), reopens `objtiles.ega` (string `0x001623C8`) and reads 0xCC bytes at that
  offset (`0x00162486..0x0016248C`). The slot pointer per tile is the table at `0x0014F2C8`
  (DATA hunk 27, 2048 × 4).
- Record: `[0]` first row, `[1]` row count `h`, `[2]` unused (§9), then the **AND mask**,
  `h·w` bytes (`w` = 2 for 16-wide, 1 for 8-wide). After that, plane blocks
  `[plane 0..4][h·w bytes]`, repeated until the byte `0xFF`. If `masktype & 4`, a `u16 LE`
  animation reference follows the `0xFF` (§6).
- Blit (`0x001531C6..0x001532DE`): every one of the 5 planes is ANDed with the mask (mask bit
  set = keep the screen pixel = **transparent**). Then each listed plane's bytes are ORed into
  that plane (`dbf` loop `0x00153254` builds `plane × 0x1F40`). Planes that are not listed stay 0
  inside the opaque area. Checked: no plane bit ever falls under a set mask bit in any object
  tile, so each pixel is either transparent or a colour 0..31, and every record fits the 0xCC
  cache slot.
- 8-wide tiles: masktype 2 draws columns 0..7, masktype 1 draws columns 8..15. On the contact
  sheet, adjacent right-half + left-half pairs together form one 16-pixel object across two
  tiles.

## 6. Animation

### 6.1 Plane-3 overlays (`animmask.ega`)

- File = `u16 LE n1`, `n1` bytes, `u16 LE n2`, `n2` bytes (file size = 4 + n1 + n2, checked).
- Reference resolution (`0x001532E0..0x0015330E`): `v < 0xFC00` gives offset `v + 0x4D00`
  (16-bit wrap) into block 1, otherwise `v + 0x400` into block 2. It is added with `adda.w`,
  i.e. sign-extended. Every real reference lies in block 1 (checked). **No tile references
  block 2**, so what its bytes are for is still open (§Tensions).
- Record = `[first row][h]`, then 8 frames of `h` big-endian row words. The frame comes from the
  byte at `0x0015803E`, which only `0x00152DCA` writes (`andi.b #7`), reached through the stub
  `0x001354E0`. Frame `f` starts at `2 + f·2h`. Its rows are **ORed into plane 3** (`+0x5DC0`),
  so an animated pixel gains colour bit 3. Since entries 8..15 are the bright versions of
  0..7 (EGA-like), this gives the shimmer on water and similar tiles.
- Every reference resolves, all 8 frames fit, and no overlay bit lands on a transparent pixel
  (checked).

### 6.2 Tile substitution (animdata)

`0x00152CD2` copies masktype, the `0x0015559E` pointer and (for object tiles) the cache entry
from tile `D1` to tile `D0`. It is skipped when `D1 ≥ 0x200` (`cmp.w #0x200,D1w` / `bcc`). In
the real `animdata`, every source (`first_anim_frame` + step) lies below 0x200 and every target
is an empty tile of §4.1, which fits this guard (checked). It is the Amiga form of Nuvie's
`tileindex[target] = tileindex[source]`.

## 7. Outline mode (tile word bits 11..12)

If `tile & 0x1800 ≠ 0` and the tile is an object, the blitter does not draw the sprite. It builds
a coverage mask in the scratch buffer at `0x00155584`: NOT of the AND mask, then ANDed with NOT
of every stored plane. That leaves exactly the opaque **colour-0** pixels, i.e. the black
outline. It then paints only those pixels in one colour, using a routine taken from the
12-entry table at `0x00152BC2`. Routine `i` ORs the mask into the planes of colour `i` and ANDs
it out of the others (hand-decoded: each plane step is an `or.b D0,(A1)` or an `and.b D2,(A1)`).
If `tile & 0xF800` is `0x0800` the colour is 11, if `0x1000` it is 4, otherwise 3
(`0x001530DE..0x0015310A`). What the game uses this for (highlight, invisibility?)
is [추정] and not traced. The decoder ignores the mode: it is a draw-time effect, not tile data.

## 8. Rotation converter (`0x0015265A`)

This routine turns an object tile into a 16×16 chunky buffer (at `0x00155584 + 0x6800`) and then
samples it rotated. It reads a 128-entry table at `0x001525D9` at angle and angle + 32,
so [추정] sine and cosine, for thrown or flying objects. It is an independent check on the
decoder's reading of the format. Rows before `first` are filled with
`0xFF` (= transparent). Each mask bit becomes `0xFF` or `0` via `smi` (`0x001526EE`), bit 7
first. Each plane block ORs `1 << plane` into the pixels. It also skips byte `[2]` (`addq.l #3`
at `0x001526B0`).

## 9. Object header byte `[2]`

No reader in `game` uses byte `[2]` of an object record. The non-outline blit loads it into D4
at `0x001531EE` and then never reads D4. The outline path skips it (`addq.w #1,A0` at
`0x0015312A`). The rotation converter starts at `+3`. Checked: it is 0 in most object tiles,
and wherever it is non-zero it never shares a bit with the set of stored planes. It reads like a
plane set the encoder dropped. A pair of tiles whose byte `[2]` names planes while none are
stored renders as black dither patterns with holes, which is plausible darkness art. The
decoder keeps the byte as `Tile::unused_byte` and renders as the game does (§Tensions).

## 10. Decoder (`u6_formats::gfx`)

- `decode_tiles(maptiles, objtiles, tileindx, masktype) -> Tileset`: 2048 `Tile`s, each with
  `pixels` (colour 0..31, row-major 16×16), `transparent`, `kind` (Map / Object Full·Left·Right),
  `first_row`/`rows`, `unused_byte`, `anim_ref`. `masktype` may be the stored LZW file or the
  2048 decoded bytes. Records that would read past what the game reads (plane 0 block, 0xCC
  cache slot) are errors, as are plane bits under the mask and plane numbers above 4.
- `AnimMask::parse(animmask)`, `AnimMask::overlay(ref)`, `Tile::frame(&anim, f)` (OR bit 3,
  `f mod 8`).
- `palette(game) -> [Rgb; 32]` (§2), `palette_from_ocs_words(64 bytes)`.
- Real-data test `gfx_real_data_all_tiles_and_contact_sheet` writes `~/Games/ultima6/re/tiles.png`
  (64×32 tiles at 2×, transparent = teal checkerboard) and `tiles-{0..3}.png` (512 each at 3×).
  The sheet is for a person to compare with the running game; it stays outside the repo.

## 11. Checks on the real files (2026-10-14, `~/Games/ultima6/unpacked/d2/DATA1`)

Each passed; the counts the probes printed are the user's data and are not reproduced.

| check | result |
|---|---|
| tileindx split vs masktype (map ≥ 0x6000, object < 0x5FFF) | no disagreement |
| largest object record vs the 0xCC cache slot | fits |
| plane bits under a set mask bit | none |
| overlay bits on transparent pixels | none |
| object byte `[2]` non-zero and overlapping stored planes | none |

## Tensions

- Byte `[2]` (§9): it reads like a set of solid planes, yet no reader uses it. If the game shows
  colours on those tiles that differ from this decode, the reading is wrong. Settle it by
  comparing with a capture from the running game (emulator), not by guessing.
- animmask block 2 is loaded, and the blitter can address it (`v ≥ 0xFC00`), but no shipped
  tile references it. Possibly references are built at run time; not traced.
- formats.md §8.1/§8.2 still mark the `.ega` files and the palette as pending. Pointing those
  sections here is for the owner of formats.md to do.

## Reproduce

    scripts/cargo.sh test -p u6-formats gfx --release   # decodes all 2048, writes the sheets
    # REA: open game.bin (raw-binary, m68k, entry 0x00100000) and procedure_assembly 0x00152e50,
    # 0x00153002, 0x0014f16c, 0x001623d6, 0x0014ef06, 0x00162358, 0x0016236a, 0x0015265a, 0x00152cd2
