---
title: ultima6 Amiga data files — byte-level formats
type: note
tags: [ultima6, re, amiga, formats, nuvie]
---

# Amiga data files — byte-level formats

Parser spec for the Rust engine, which reads the **Amiga** files directly. Measured 2026-10-14 on
`~/Games/ultima6/unpacked/{d1/INSTALL,d2/DATA1,d3/DATA2}` (PP20-decrunched). Cross-reference
reader = ScummVM Nuvie; `N:` = `scummvm/engines/ultima/nuvie/`, `L:` =
`scummvm/devtools/create_ultima/files/ultima6/scripts/u6/`. Nuvie reads the DOS layout, so
"layout vs DOS (Nuvie)" compares against the layout Nuvie's reader expects, not against DOS
file bytes (no DOS copy was available). Throwaway probe scripts (not committed) checked every rule
here against the user's files. Values read out of those files (sizes of variable files, dumps,
counts, field values, names) are not reproduced: where a value proves a reading, the note says
what was checked and that it matched. BE/LE = big/little-endian. `[추정]` = inference.

## 0. Container layers — apply in this order

1. **PP20** (PowerPacker): crunching is **per file, not per type**. Check magic `PP20` at offset 0
   on every file and decrunch (`tools/pp20.py: decrunch(bytes)->bytes`). On the raw disk tree
   `~/Games/ultima6/files/` both forms occur, even among the `OBJBLK??` files (the smallest are
   stored raw), so no reader may assume either by name.
2. **U6-LZW** (§1): whole-file for `look.lzd *.bmp u6.set INTRO.PTR STARPOS.DAT masktype.ega *.new`;
   per item inside `portrait.*` and `converse.*` (§2).

## 1. U6-LZW (N: files/u6_lzw.cpp:176-262 decode, :316-349 codeword read)

- `[0..4]` u32 **LE** = decompressed size; byte 3 is always 0 (validity check :89-111).
- Bitstream from byte 4, **LSB-first**: the code at bit `p` is `(u24le(buf[p/8..]) >> (p%8)) & mask(width)`.
- Width starts at 9. Code `0x100` = clear (width 9, next_free = 0x102, dictionary emptied; the
  **next code is a literal root**, output directly, no dictionary add, and it becomes `prev`;
  N: u6_lzw.cpp:196-203,258). `0x101` = end.
- Every stream starts with `0x100`. That is the validity test: `buf[4]==0 && (buf[5]&1)==1`.
- Other codes: if `c < next_free`, output `string(c)`, add `(prev, first(string(c)))`. Otherwise
  `c` must equal `next_free` (KwKwK case): output `string(prev)+first(string(prev))` and add the
  same entry. Then `next_free++`; if `next_free >= 1<<width && width < 12`, `width++`.
- Encoder behaviour (checked on the long streams, e.g. `intro.new`, `end.new`): it emits `0x100`
  exactly when next_free reaches `0x1000`, and that clear is read at the current width (12), not
  9. Small files have only the leading clear. The `0x101` code ends in the last byte (no trailing
  bytes) in every stream checked: the whole-file LZW files below, intro.new, end.new, and
  portrait.a item 0.
- Bit reader must tolerate reading past the end: the u24 read at `p/8` can reach 1-2 bytes
  beyond the last stream byte for the final code (Nuvie reads `[p/8+1]` always, `[p/8+2]` when
  needed, :320-325). Zero-pad the buffer by 2 bytes or bounds-check (a Rust slice index panics).
- Nuvie fallback, not needed by any Amiga file: a non-LZW file decodes as `bytes[8..]`
  (:299-309).

Whole-file LZW files: `look.lzd paper.bmp newmagic.bmp worldmap.bmp u6.set INTRO.PTR
masktype.ega STARPOS.DAT`. For each, the decoded length equals the header and the leading clear
is the only one (checked).

## 2. U6Lib indexed archive (N: files/u6_lib_n.cpp:178 parse, :221 sizes, :271 count)

- Offset table at 0. Entries are u16 **LE** (`book.dat`) or u32 **LE** (`portrait.*`,
  `converse.*`). For u32, `offset = v & 0xFFFFFF` and `flag = v >> 24`. Every flag is 0 in the
  Amiga files (checked), so items are not flag-compressed (:158).
- Item count = `min(nonzero offset) / entry_size`. An offset of 0 means an empty item.
- `size(i)` = the next nonzero offset (or the file size) − `offset(i)`. No file-size header in U6 libs.

| lib | entry | item payload |
|---|---|---|
| book.dat | u16 | ASCII text, NUL-terminated (every item checked) |
| portrait.a / .b / .z | u32 | LZW stream (§1) → 3584 B |
| converse.a / .b | u32 | see below (item 0 of converse.a is empty) |

- **Portrait**: 56×64 = 3584 bytes, 1 byte per pixel, row-major (N: portraits/portrait_u6.cpp:55-56,
  :148-151). Pixel values are **256-colour (VGA) palette indices**: the values in portrait.b
  span far more than 32 colours (checked), so the Amiga does not use a reduced index space here.
- **Converse item** (N: core/converse.cpp:657-693): if the first 4 bytes are 0, the script is
  `item[4..]` uncompressed. Otherwise the whole item is one LZW stream (§1). Both forms occur
  (the raw one in converse.b). The script bytecode reads multi-byte operands **LSB-first**
  (:696-728). It is byte-identical to DOS [추정]: the md5 of the first 5000 bytes of `converse.a`
  equals ScummVM's GOG DOS detection entry (`engines/ultima/detection_tables.h:197`), and so does
  the file size (both checked).

## 3. File table (every file Nuvie opens for U6, plus every Amiga data file)

| DOS name (Nuvie) | Nuvie reader | Amiga file | layout (detail §) | layout vs DOS (Nuvie) |
|---|---|---|---|---|
| map | core/map.cpp:371,564-616 | d2 map (32256 = 64×384 + 5×1536) | 12-bit chunk refs (§4.1) | identical |
| chunks | core/map.cpp:375,588-599 | d2 chunks (65536 = 1024×64) | 1024 × 64 u8 (§4.1) | identical |
| tileflag | core/tile_manager.cpp:399-450; obj_manager.cpp:134-141 | d1 tileflag (7168) | u8 arrays (§4.2) | identical |
| basetile | core/obj_manager.cpp:117-125 | d1 basetile (2048 = 1024 × 2) | 1024 × u16 **BE** (§4.2) | swap u16 |
| animdata | core/tile_manager.cpp:461-490 | d1 animdata (194 = 2 + 64 + 64 + 32 + 32) | u16 **BE** + u8 (§4.2) | swap u16 part |
| schedule | actors/actor_manager.cpp:675-720; actor.cpp:1184-1214 | d1 schedule (514 + 5 × total) | u16 **BE** header + 5-B records (§4.3) | swap header |
| savegame/objblk{a-h}{a-h}, objblk{a-e}i | save/save_game.cpp:137-185; core/obj_manager.cpp:147,1496-1540 | d2 OBJBLK?? ×69 | u16 **BE** count + 8-B records (§5) | swap count, rec+4, rec+6 |
| savegame/objlist | save/save_game.cpp:192-201,219-275 (+ readers §6) | d2 objlist (0x1D73) | mixed, u16 **BE** (§6) | swap listed fields |
| lzobjblk, lzdngblk | save/save_game.cpp:95,109 (new game) | — | — | **missing** (§7) |
| look.lzd | core/look.cpp:58-100 | d2 look.lzd | LZW → u16 **LE** + strings (§4.4) | identical |
| book.dat | core/book.cpp:42-57; usecode/u6_usecode.cpp:2415-2418 | d3 book.dat | U6Lib u16 (§2) | identical |
| converse.a / .b | core/converse.cpp:125-145,657-693 | d3 converse.a / .b | U6Lib u32 (§2) | identical [추정] |
| portrait.a/.b/.z | portraits/portrait_u6.cpp:59-69,148-151 | d3 portrait.a / .b / .z | U6Lib u32 → LZW (§2) | identical |
| u6.ch | fonts/font_manager.cpp:76-97; u6_font.cpp:62-70 | d1 u6.ch (2048 = 256 × 8) | 1-bpp glyphs (§4.5) | identical |
| u6.set | script/script_cutscene.cpp:1183; fonts/wou_font.cpp:45-59,108 | d1 u6.set | LZW → WOU font (§4.5) | identical |
| paper.bmp | gui/widgets/background.cpp:57; files/u6_bmp.cpp:41-62 | d1 paper.bmp | LZW → bitmap (§4.6) | identical |
| newmagic.bmp | views/spell_view.cpp:71 | d3 newmagic.bmp | LZW → bitmap (§4.6) | identical |
| — (not read) | — | d3 worldmap.bmp | LZW → bitmap (§4.6) | no Nuvie reader |
| maptiles.vga, masktype.vga, objtiles.vga, tileindx.vga, animmask.vga | core/tile_manager.cpp:132-215,565-600 | d2 `*.ega` (§8.1) | different format | **pending WU-4 (REA)** |
| u6pal | screen/game_palette.cpp:52-79 | — | 256 × RGB 6-bit (DOS) | **missing** (§8.2) |
| dither | screen/dither.cpp:47-63 (only if dither_mode ≠ none, :36-40) | — | 0x200 B (DOS) | missing, optional |
| u6mcga.ptr | core/cursor.cpp:61-100 | (d1 INTRO.PTR?) | LZW → lib of U6Shape (§8.3) | missing |
| the Adlib music files | sound/sound_manager.cpp:240-290 | d1 song files (named by `game`'s table A4-0x779a) | MMD0 (OctaMED) | different format |
| intro*.shp, blocks.shp, vellum1.shp, end.shp, palettes.int, endpal.lbm | L: intro.lua, ending.lua; N: script/script_cutscene.cpp:1264-1295 | d1 `*.new` ×14, d2 end.new | LZW → BE32 table (§8.4) | different format, pending |
| — | — | d1 STARPOS.DAT | LZW → 600 u16 (§8.2) | no Nuvie reader |
| — | — | d2 end | Amiga hunk exe (`HUNK_HEADER`) | executable (ending) |

Not data, out of scope: d1 `game u ultcli ultima6 installu6 start c/endcli s/startup-sequence
disk.info ultima6.info` and d2 `Trashcan.info` (executables, scripts and icons; exe layout =
`re/notes/game-layout.md`).

Directory layout: Nuvie opens every name relative to `gamedir` (N: misc/u6_misc.cpp:73-88). The
object files live in the `savegame/` subdirectory with lowercase names (save_game.cpp:57-58).
ScummVM detection needs only `converse.a` (md5 of the first 5000 bytes, advancedDetector.cpp:994).

## 4. Per-file layouts

### 4.1 map (32256 = 64×384 + 5×1536) and chunks (65536 = 1024×64)
- Surface superchunk `s` = 0..63 at `s*384`: `sx = s%8`, `sy = s/8`, covering world tiles
  `[128·sx, +128) × [128·sy, +128)`. It holds 16 rows × 8 triplets, row `i` = tile y `128·sy + 8i`.
  A triplet `(b0,b1,b2)` holds two 12-bit chunk refs: `c1 = b0 | (b1&0xF)<<8` (left),
  `c2 = b2<<4 | b1>>4` (right) (map.cpp:564-586).
- Dungeon level `L` = 0..4 (z = L+1) at `24576 + L*1536`: 32 rows × 16 triplets = 256×256 tiles (:601-616).
- Chunk `c` = `chunks[c*64..+64]`: 8 rows × 8 u8 map-tile numbers, row-major (:588-599).
- Proof: under this packing every ref is ≤ 1023 (= chunk count − 1). The other nibble order
  gives refs above 1023 (checked).

### 4.2 tileflag (7168), basetile (2048), animdata (194)
- tileflag, u8 arrays (tile_manager.cpp:404-449, obj_manager.cpp:139-141):
  `[0x0000,0x0800)` flags1[tile]: bit0 water, bit1 impassable, bit3 damaging.
  `[0x0800,0x1000)` flags2[tile]: bit2|bit3 boundary, bit4 top tile, bit6 double height, bit7 double width.
  `[0x1000,0x1400)` weight[obj_n].
  `[0x1400,0x1C00)` flags3[tile], bits 6-7 = article (none/a/an/the).
- basetile: `u16 BE[1024]`, obj_n → base tile; `tile = basetile[obj_n] + frame`
  (obj_manager.cpp:417,1302-1304). Under BE every value is below 2048 (the tile count); under LE
  most are not. Non-decreasing up to the last object type in use; the rest are 0 (checked).
- animdata: `u16 BE n` @0, `u16 BE tile_to_animate[32]` @2, `u16 BE first_anim_frame[32]`
  @66, `u8 and_mask[32]` @130, `u8 shift[32]` @162. Each tick: `tileindex[tile_to_animate[i]] =
  tileindex[first_anim_frame[i] + ((counter & and_mask[i]) >> shift[i])]` (tile_manager.cpp:351-370).
  Proof: under BE `n` ≤ 32 and the targets include the three tiles Nuvie hard-codes
  (:490-492); under LE `n` is far above 32 (checked).

### 4.3 schedule (514 + 5 × total)
- `u16 BE first[256]` @0 = index of each actor's first record. `u16 BE total` @512.
  Records start @514. For actor `a`: if `first[a] > total−1`, count = 0; else if `a == 255` or
  `first[a+1] > total−1`, count = `total − first[a]`; else count = `first[a+1] − first[a]`
  (actor_manager.cpp:686-703).
- Record, 5 bytes, byte-oriented (actor.cpp:1194-1204): `[0]` bits0-4 hour, bits5-7 day-of-week;
  `[1]` worktype; `[2..5)` x,y,z packed as in §5 bytes 1-3.
- Proof: BE `first[]` is non-decreasing and `514 + 5 × total` is the file size; under LE the
  total would need a file of tens of kilobytes. Record 0 decodes to a valid hour and worktype and
  to a position next to its actor's objlist position (§6) (checked).

### 4.4 look.lzd (LZW)
- Entries `[u16 LE tile][NUL-terminated ASCII]`, ascending tile. Stop at tile ≥ 2048 (look.cpp:75-95).
  A tile without its own entry uses the next entry's string (:93-94). Strings carry plural
  markers `\` and `/` (:125). Checked: the tiles ascend, the last entry is below 2048 and the
  list ends with a terminator tile ≥ 0x0800.

### 4.5 fonts
- u6.ch: 256 glyphs × 8 bytes; one byte per row, bit 7 = leftmost pixel. Glyphs 0-127 are
  English, 128-255 the runic and the gargoyles' script (font_manager.cpp:85-95, u6_font.cpp:62-70).
- u6.set (LZW): `[0]` height, `[2]` ink value, `[4+c]` width, `[0x104+c]` offset lo,
  `[0x204+c]` offset hi. Glyph = width×height bytes at `hi*256+lo`; a byte equal to ink = set pixel
  (wou_font.cpp:105-118). Checked: the last glyph ends exactly at the decoded size.

### 4.6 bitmaps (paper.bmp, newmagic.bmp, worldmap.bmp)
- LZW → `u16 LE width, u16 LE height`, then `width×height` u8 palette indices, row-major
  (u6_bmp.cpp:41-62). Checked: for each of the three, the decoded size = 4 + w·h under LE; under
  BE the header would describe a bitmap far larger than the file.

## 5. Object blocks — `OBJBLK??` (d2/DATA1, 69 files)

- Names: `OBJBLK` + X + Y, with X, Y ∈ A..H = superchunk column and row. The surface index is
  `s = (Y−'A')·8 + (X−'A')`, the same `s` as in map §4.1 (save_game.cpp:150-160). `OBJBLK{A..E}I` =
  dungeon levels z = 1..5. All 64 + 5 are present; the names are uppercase on Amiga, while Nuvie
  wants lowercase `savegame/objblkxy`.
- File = `u16 BE count`, then `count` × 8-byte records. `2 + 8·count == size` holds for all 69
  (checked).

| off | size | field | encoding (Amiga) | DOS (Nuvie loadObj, obj_manager.cpp:1496-1540) |
|---|---|---|---|---|
| 0 | 1 | status | bits 3-4 = where (0x00 map, 0x08 container, 0x10 inventory, 0x18 readied); 0x01 ok-to-take, 0x02 invisible, 0x04 charmed, 0x20 temporary, 0x40 egg-active/broken, 0x80 lit (obj.h:36-56) | same |
| 1 | 3 | x, y, z | bitfield: `x = b1 \| (b2&3)<<8`, `y = b2>>2 \| (b3&0xF)<<6`, `z = b3>>4` | same bytes |
| 4 | 2 | obj_n, frame | **u16 BE** `v`: `obj_n = v & 0x3FF`, `frame = v >> 10` | u16 LE |
| 6 | 2 | quality, qty | **u16 BE** `w`: `quality = w>>8` (byte 6), `qty = w & 0xFF` (byte 7); stackable `qty = w` | LE word: byte 6 qty, byte 7 quality |

- Container child (status 0x08): `x | (y&0x3F)<<10` = list index of its container in the same
  file (obj_manager.cpp:1479). Inventory or readied: `x` = actor number (:170).
- Stackable obj_n (is_stackable, obj_manager.cpp:562-600): 55, 56, 63, 65-72, 77, 83, 88, 89,
  90 (frame ≠ 1), 91, 92, 128, 129, 337. Readied objects never stack.
- All four placements (map, container, inventory, readied) occur in the shipped blocks.

Proof, per field (checked on every record; the values are not reproduced):
- x,y,z: every surface on-map record lies inside its own file's superchunk with z = 0. Dungeon
  on-map records have z = 1..5 for AI..EI and x within the 256-tile level.
- obj_n: under BE every record is ≤ 431 = `OBJ_U6__LAST_` (u6_objects.h:400); under LE many are
  not. Spot checks of BE values against Nuvie's object list (u6_objects.h) gave objects that
  fit their map places.
- quality vs qty:
  - Signs (332): byte 6 is distinct per sign and byte 7 is always 0. Byte 6 is therefore the
    book index, i.e. quality (u6_usecode.cpp:2415). Books, scrolls and tombstones look the same.
  - Eggs (335): byte 7 is a percentage (at most 101) = hatch percent = qty. Byte 6 is a small
    code = alignment+period = quality (egg_manager.cpp:121,162-175).
  - Stackables: byte 6 = 0 and byte 7 ≠ 0, and a stack's quantity reads as the whole word `w`
    (checked on gold in containers).

## 6. objlist (d2/DATA1, 0x1D73 bytes)

Actor arrays are indexed by actor number 0..255. `AM` = actors/actor_manager.cpp. Not-read
regions are listed so that a parser can skip them.

| offset | layout | field | reader |
|---|---|---|---|
| 0x0000 | u8[256] | obj_flags | AM:158-162 |
| 0x0100 | 3 B[256] | x,y,z (as §5 bytes 1-3) | AM:104-136 |
| 0x0400 | **u16 BE**[256] | obj_n/frame (`v&0x3FF`, `v>>10`) | AM:140-155 |
| 0x0600 | **u16 BE**[256] [추정] | unknown; not read by Nuvie; every nonzero byte sits at an odd offset | — |
| 0x0800 | u8[256] | status flags (alignment bits 5-6) | AM:166-171 |
| 0x0900 / 0x0A00 / 0x0B00 | u8[256] each | str / dex / int | AM:198-219 |
| 0x0C00 | **u16 BE**[256] | experience | AM:222-227 |
| 0x0E00 | u8[256] | hp | AM:230-235 |
| 0x0F00 | 16 × 14 B | party names, NUL-padded | core/party.cpp:100-102 |
| 0x0FE0 / 0x0FF0 | u8[16] / u8 | party roster / party size | party.cpp:96-107 |
| 0x0FF1 | u8[256] | level | AM:238-243 |
| 0x10F1 | u8[256] | unknown, not read (nonzero bytes at both parities) | — |
| 0x11F1 / 0x12F1 / 0x13F1 / 0x14F1 | u8[256] each | worktype / combat mode / magic / movement points | AM:247-317 |
| 0x15F1 | **u16 BE**[256] | base obj_n/frame | AM:175-195 |
| 0x17F1 / 0x19F1 | u8[256] each | talk flags / movement flags | AM:280-293 |
| 0x18F1, 0x1AF1 | 256 B each | not read for U6, all 0 | — |
| 0x1BF1 / 0x1BF2 | u8 / u8 | quest flag / rest counter | core/player.cpp:85; game_clock.cpp:96 |
| 0x1BF3 | u8 ×4 | minute, hour, day, month | core/game_clock.cpp:63-70 |
| 0x1BF7 | **u16 BE** | year | game_clock.cpp:71 |
| 0x1BF9 / 0x1BFA | u8 / u8 | karma / wind direction | player.cpp:88; weather.cpp:102 |
| 0x1BFB | **u16 BE**[4] [추정] | unknown, not read (nonzero bytes at odd offsets only) | — |
| 0x1C03 | u8[16] | timers; [15] = eclipse (0x1C12) | game_clock.cpp:90-93; game_clock.h:32,38 |
| 0x1C13 | **u16 BE** | vanish obj (`>1023` encodes frame) | L: init.lua:57,366-377 |
| 0x1C17 | u8 | alcohol | player.cpp:91 |
| 0x1C1B | **u16 BE**[8][3] | moonstone x, y, z | L: init.lua:58,380-389 |
| 0x1C4B | **u16 BE** | keg timer | L: init.lua:59,391-392 |
| 0x1C5F / 0x1C69 / 0x1C6A | u8 each | gargoyle-tongue flag / combat mode / solo member (0xFF = party) | player.cpp:94-98; party.cpp:112 |
| 0x1C6C / 0x1C71 / 0x1C72 | u8 each | command-bar selection / gender / hero portrait (1-based) | command_bar.cpp:113; player.cpp:100; portrait_u6.cpp:80-82 |
| 0x1C73..0x1D72 | 256 B | residue, never read: ASCII text fragments mixed with LE-looking words | — |

Proof (checked; the values are not reproduced): under BE every actor obj_n is ≤ 431, under LE
many are not. Actor 1's BE obj_n is the hero's type in Nuvie's object list (u6_objects.h:372).
The BE experience of actors 2-4 is a small number where LE gives tens of thousands. The BE year
is the game's start year. Every BE moonstone x,y is ≤ 1023, every LE one is > 1023. The party
names at 0xF00 are NUL-padded ASCII, and the clock bytes read as a valid time and date.

## 7. Starting a new game

- Nuvie (nuvie.cpp:358-381): with `config/newgame` set, or no prior save, it calls
  `SaveGame::load_new` (save_game.cpp:82-131):
  - LZW `lzobjblk` = the 64 surface blocks concatenated in order `s = 0..63` (each block = a §5
    file body).
  - LZW `lzdngblk` = the 5 dungeon blocks, then the objlist (the rest of the buffer).
  - Then `update_objlist_for_new_game_u6` (:438-486) patches: name @0xF00 (≤ 13 chars + NUL),
    str @0x901, dex @0xA01, int @0xB01, exp @0xC02 (u16), level @0xFF2, magic @0x13F2,
    gender @0x1C71 (`gender==1 ? 0 : 1`), portrait @0x1C72 (`gender*6 + portrait + 1`).
- Nuvie's "original save" path `load_original` (:137-205) reads `savegame/objblk{x}{y}`, looping
  y outer and x inner (so `s = y*8+x`), then `objblk{a..e}i`, then `savegame/objlist`.
- **Amiga**: there is no `lzobjblk`/`lzdngblk`. DATA1 ships the expanded set (64+5 OBJBLK + objlist),
  i.e. the start state [추정: the installer copies DATA1 to the play/save location; confirm in
  `installu6` RE]. Rust new game = parse DATA1 as §5/§6 and apply the same patches. The exp at
  0xC02 is written **BE**, because that region is BE on Amiga.

## 8. Pending formats (target spec = what Nuvie expects from DOS)

### 8.1 `.ega` tile graphics — pending WU-4 (REA)
| Amiga (d2/DATA1) | checked | DOS target (core/tile_manager.cpp) |
|---|---|---|
| maptiles.ega | PP20-crunched, then not LZW | maptiles.vga: LZW → 8-bit 16×16 tiles, 256 B each (:132-150) |
| objtiles.ega | PP20-crunched, then not LZW | objtiles.vga: raw, appended after decoded maptiles (:183-191) |
| tileindx.ega | 2048 × u16, monotonic in neither byte order | tileindx.vga: u16 LE[2048], ×16 = byte offset into maptiles+objtiles (:195-200) |
| masktype.ega | LZW → 2048 B, values 0..7 | masktype.vga: LZW → u8[2048] ∈ {0 plain, 5 transparent, 0xA pixel-block} (:202-221; .h:36-38) |
| animmask.ega | PP20-crunched, then not LZW | animmask.vga: LZW → 32 × 64-B run masks for tiles 16..47 (:565-600) |

The Amiga layouts are in [graphics.md](graphics.md).

The DOS pixel-block tile is `[0]` count, then `{u16 LE disp, u8 len, len bytes}` until len = 0;
`x = disp%160 + (disp ≥ 1760 ? 160 : 0)` (decodePixelBlockTile, :510). The dither step
(`dither_bitmap`, :224) runs per tile after decode.

### 8.2 Other gaps
- Palette: no `u6pal` on Amiga. DOS = 256 × (r,g,b) 6-bit, shifted left by 2
  (game_palette.cpp:75-80); background index 0x31 (:95-99). The Amiga palette source is
  unknown (game exe RE) [추정].
- Cursor: DOS `u6mcga.ptr` = LZW → lib (u32 LE size + u32 LE offsets) → U6Shape with u16 LE fields
  (cursor.cpp:83-104; u6_shape.cpp:175-215). Amiga `INTRO.PTR` (d1, raw) = LZW, its inner words
  LE-plausible (checked: a size, then offsets inside the data). Whether it holds the main-game
  cursors [추정: intro only] is unknown.
- Cutscenes: 14 files in d1 (`blocks gypsy intro intro_1a intro_1b intro_2 intro_2a mainmenu
  montage title vellum1 woods` + `.new`, plus `INTRO_1.NEW INTRO_3.NEW`) and d2 `end.new` are whole-file LZW. The
  decoded data opens with a BE32 table of item offsets (checked on blocks.new and vellum1.new:
  the first offset is the table's size). DOS `.shp` = LZW → lib → U6Shape
  (script_cutscene.cpp:1276-1291). The inner format is pending (milestone-1 non-goal, .prd/01-spec.md).
- Music: the song files `game` names in its table at A4-0x779a = MMD0 (OctaMED, magic `MMD0`).
  DOS = Adlib. Non-goal for milestone 1.
- STARPOS.DAT (raw): U6-LZW → 600 u16. As u16 LE, the first and the second half each stay in a
  small screen-sized range (as BE they do not; checked). Reading: 300 stars, `x[300]` then
  `y[300]`, u16 LE [추정]. No Nuvie reader.

## Tensions

- The dispatcher brief said basetile is "monotonic". Checked: BE is non-decreasing up to the
  last object type in use only, then drops to 0 for the rest.
- objlist 0x0600 and 0x1BFB are marked BE u16 only from byte parity; nothing reads them in Nuvie.
  Settle in the exe RE.
- "converse.* identical to DOS" rests on the first-5000-byte md5 and an equal size; the
  remainder has not been checked against a DOS copy.

## Reproduce (paths outside the repo; run with `python3 -I`)

    D=~/Games/ultima6/unpacked/d2/DATA1
    # every OBJBLK: BE count fits size, BE obj_n <= 431
    python3 -I -c "import os,sys;d=sys.argv[1];[print(f) for f in sorted(os.listdir(d)) if f.startswith('OBJBLK') and (lambda b:(int.from_bytes(b[:2],'big')*8+2!=len(b)) or any((((b[2+i*8+4]<<8)|b[2+i*8+5])&0x3ff)>431 for i in range(int.from_bytes(b[:2],'big'))))(open(os.path.join(d,f),'rb').read())]" $D   # prints nothing
    # objlist year (BE u16 @0x1BF7): prints the start year
    python3 -I -c "import sys;b=open(sys.argv[1],'rb').read();print(int.from_bytes(b[0x1bf7:0x1bf9],'big'))" $D/objlist
    # schedule: 514 + 5*total == size -> True
    python3 -I -c "import sys;b=open(sys.argv[1],'rb').read();print(514+5*int.from_bytes(b[512:514],'big')==len(b))" ~/Games/ultima6/unpacked/d1/INSTALL/schedule
