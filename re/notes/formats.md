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
file bytes (no DOS copy was available). Throwaway probe scripts (not committed) produced every number here.
BE/LE = big/little-endian. `[추정]` = inference. Hex dumps are first bytes of the file or record.

## 0. Container layers — apply in this order

1. **PP20** (PowerPacker): crunching is **per file, not per type**. Check magic `PP20` at offset 0
   on every file and decrunch (`tools/pp20.py: decrunch(bytes)->bytes`). Measured on the raw disk
   tree `~/Games/ultima6/files/`: 79 of 130 files crunched. Crunched files: `map chunks objlist
   end maptiles.ega objtiles.ega animmask.ega basetile animdata schedule tileflag u6.ch
   book.dat ULTIMA_?a.MOD` and 54 of 69 `OBJBLK??`. Raw: `OBJBLK{AA,AF,BA,BF,CH,ED,EG,FD,FE,FH,GA,GF,GG,HA,HF}`
   (the ≤50-byte ones), `look.lzd masktype.ega tileindx.ega converse.* portrait.* *.bmp
   u6.set INTRO.PTR STARPOS.DAT *.new end.new`.
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
- Encoder behaviour (measured): it emits `0x100` exactly when next_free reaches `0x1000`
  (`intro.new` 9 clears, `end.new` 15; max next_free 4096). That clear is read at the current
  width (12), not 9. Small files have only the leading
  clear. The `0x101` code ends in the last byte (0 trailing bytes) in every stream measured: the
  table below, intro.new, end.new, and portrait.a item 0.
- Bit reader must tolerate reading past the end: the u24 read at `p/8` can reach 1-2 bytes
  beyond the last stream byte for the final code (Nuvie reads `[p/8+1]` always, `[p/8+2]` when
  needed, :320-325). Zero-pad the buffer by 2 bytes or bounds-check (a Rust slice index panics).
- Nuvie fallback, not needed by any Amiga file: a non-LZW file decodes as `bytes[8..]`
  (:299-309).

| LZW file | stored | decoded (= header) | clears | max next_free |
|---|---|---|---|---|
| look.lzd | 4193 | 5914 | 1 | 3282 |
| paper.bmp | 3881 | 64004 | 1 | 3074 |
| newmagic.bmp | 998 | 11812 | 1 | 1071 |
| worldmap.bmp | 3500 | 16388 | 1 | 2820 |
| u6.set | 1168 | 7004 | 1 | 1194 |
| INTRO.PTR | 746 | 1447 | 1 | 874 |
| masktype.ega | 341 | 2048 | 1 | 550 |
| STARPOS.DAT | 755 | 1200 | 1 | 881 |

## 2. U6Lib indexed archive (N: files/u6_lib_n.cpp:178 parse, :221 sizes, :271 count)

- Offset table at 0. Entries are u16 **LE** (`book.dat`) or u32 **LE** (`portrait.*`,
  `converse.*`). For u32, `offset = v & 0xFFFFFF` and `flag = v >> 24`. Every flag is 0 in the
  Amiga files (measured), so items are not flag-compressed (:158).
- Item count = `min(nonzero offset) / entry_size`. An offset of 0 means an empty item.
- `size(i)` = the next nonzero offset (or the file size) − `offset(i)`. No file-size header in U6 libs.

| lib | entry | items | first entry dump | item payload |
|---|---|---|---|---|
| book.dat 25132 | u16 | 128 | `00 01 1e 01 54 01` → 256, 286, 340 | ASCII text, NUL-terminated (128/128) |
| portrait.a 142495 | u32 | 98 | `88 01 00 00 cc 06 00 00` | LZW stream (§1) → 3584 B |
| portrait.b 139346 | u32 | 96 | `80 01 00 00 00 08 00 00` | same |
| portrait.z 20249 | u32 | 12 | `30 00 00 00 7a 07 00 00` | same |
| converse.a 162615 | u32 | 99 (97 non-empty) | `00 00 00 00 …` (item 0 empty) | see below |
| converse.b 152537 | u32 | 125 (103 non-empty) | `f4 01 00 00 86 07 00 00` | see below |

- **Portrait**: 56×64 = 3584 bytes, 1 byte per pixel, row-major (N: portraits/portrait_u6.cpp:55-56,
  :148-151). Pixel values are **256-colour (VGA) palette indices**: measured range 0..239 and
  221 distinct values in portrait.b. The Amiga does not use a reduced index space here.
- **Converse item** (N: core/converse.cpp:657-693): if the first 4 bytes are 0, the script is
  `item[4..]` uncompressed. Otherwise the whole item is one LZW stream (§1). Measured: 199 LZW
  items and 1 raw item (converse.b). The script bytecode reads multi-byte operands **LSB-first**
  (:696-728). It is byte-identical to DOS [추정]: the md5 of the first 5000 bytes of `converse.a`
  = `5065716423ef1389e3f7b4946d815c26` = ScummVM's GOG DOS detection entry
  (`engines/ultima/detection_tables.h:197`, size 162615 also equal).

## 3. File table (every file Nuvie opens for U6, plus every Amiga data file)

| DOS name (Nuvie) | Nuvie reader | Amiga file, size | layout (detail §) | layout vs DOS (Nuvie) |
|---|---|---|---|---|
| map | core/map.cpp:371,564-616 | d2 map 32256 | 12-bit chunk refs (§4.1) | identical |
| chunks | core/map.cpp:375,588-599 | d2 chunks 65536 | 1024 × 64 u8 (§4.1) | identical |
| tileflag | core/tile_manager.cpp:399-450; obj_manager.cpp:134-141 | d1 tileflag 7168 | u8 arrays (§4.2) | identical |
| basetile | core/obj_manager.cpp:117-125 | d1 basetile 2048 | 1024 × u16 **BE** (§4.2) | swap u16 |
| animdata | core/tile_manager.cpp:461-490 | d1 animdata 194 | u16 **BE** + u8 (§4.2) | swap u16 part |
| schedule | actors/actor_manager.cpp:675-720; actor.cpp:1184-1214 | d1 schedule 3329 | u16 **BE** header + 5-B records (§4.3) | swap header |
| savegame/objblk{a-h}{a-h}, objblk{a-e}i | save/save_game.cpp:137-185; core/obj_manager.cpp:147,1496-1540 | d2 OBJBLK?? ×69 | u16 **BE** count + 8-B records (§5) | swap count, rec+4, rec+6 |
| savegame/objlist | save/save_game.cpp:192-201,219-275 (+ readers §6) | d2 objlist 7539 | mixed, u16 **BE** (§6) | swap listed fields |
| lzobjblk, lzdngblk | save/save_game.cpp:95,109 (new game) | — | — | **missing** (§7) |
| look.lzd | core/look.cpp:58-100 | d2 look.lzd 4193 | LZW → u16 **LE** + strings (§4.4) | identical |
| book.dat | core/book.cpp:42-57; usecode/u6_usecode.cpp:2415-2418 | d3 book.dat 25132 | U6Lib u16 (§2) | identical |
| converse.a / .b | core/converse.cpp:125-145,657-693 | d3 162615 / 152537 | U6Lib u32 (§2) | identical [추정] |
| portrait.a/.b/.z | portraits/portrait_u6.cpp:59-69,148-151 | d3 142495/139346/20249 | U6Lib u32 → LZW (§2) | identical |
| u6.ch | fonts/font_manager.cpp:76-97; u6_font.cpp:62-70 | d1 u6.ch 2048 | 1-bpp glyphs (§4.5) | identical |
| u6.set | script/script_cutscene.cpp:1183; fonts/wou_font.cpp:45-59,108 | d1 u6.set 1168 | LZW → WOU font (§4.5) | identical |
| paper.bmp | gui/widgets/background.cpp:57; files/u6_bmp.cpp:41-62 | d1 paper.bmp 3881 | LZW → bitmap (§4.6) | identical |
| newmagic.bmp | views/spell_view.cpp:71 | d3 newmagic.bmp 998 | LZW → bitmap (§4.6) | identical |
| — (not read) | — | d3 worldmap.bmp 3500 | LZW → 128×128 bitmap (§4.6) | no Nuvie reader |
| maptiles.vga, masktype.vga, objtiles.vga, tileindx.vga, animmask.vga | core/tile_manager.cpp:132-215,565-600 | d2 `*.ega` (§8.1) | different format | **pending WU-4 (REA)** |
| u6pal | screen/game_palette.cpp:52-79 | — | 256 × RGB 6-bit (DOS) | **missing** (§8.2) |
| dither | screen/dither.cpp:47-63 (only if dither_mode ≠ none, :36-40) | — | 0x200 B (DOS) | missing, optional |
| u6mcga.ptr | core/cursor.cpp:61-100 | (d1 INTRO.PTR 746?) | LZW → lib of U6Shape (§8.3) | missing |
| brit.m, forest.m, … (Adlib) | sound/sound_manager.cpp:240-290 | d1 ULTIMA_{A-H}a.MOD ×8 | MMD0 (OctaMED) | different format |
| intro*.shp, blocks.shp, vellum1.shp, end.shp, palettes.int, endpal.lbm | L: intro.lua, ending.lua; N: script/script_cutscene.cpp:1264-1295 | d1 `*.new` ×14, d2 end.new | LZW → BE32 table (§8.4) | different format, pending |
| — | — | d1 STARPOS.DAT 755 | LZW → 1200 B (§8.2) | no Nuvie reader |
| — | — | d2 end 37724 | Amiga hunk exe (`00 00 03 f3`) | executable (ending) |

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
  `c2 = b2<<4 | b1>>4` (right) (map.cpp:564-586). Dump: `eb b1 1e …`.
- Dungeon level `L` = 0..4 (z = L+1) at `24576 + L*1536`: 32 rows × 16 triplets = 256×256 tiles (:601-616).
- Chunk `c` = `chunks[c*64..+64]`: 8 rows × 8 u8 map-tile numbers, row-major (:588-599).
- Proof: under this packing all 21504 refs are ≤ 1023 (= chunk count − 1). The other nibble
  order gives 4716 refs > 1023.

### 4.2 tileflag (7168), basetile (2048), animdata (194)
- tileflag, u8 arrays (tile_manager.cpp:404-449, obj_manager.cpp:139-141):
  `[0x0000,0x0800)` flags1[tile]: bit0 water, bit1 impassable, bit3 damaging.
  `[0x0800,0x1000)` flags2[tile]: bit2|bit3 boundary, bit4 top tile, bit6 double height, bit7 double width.
  `[0x1000,0x1400)` weight[obj_n] (dump `00 04 0c 18 1b 0a 0f 1a`).
  `[0x1400,0x1C00)` flags3[tile], bits 6-7 = article (none/a/an/the).
- basetile: `u16 BE[1024]`, obj_n → base tile; `tile = basetile[obj_n] + frame`
  (obj_manager.cpp:417,1302-1304). Dump `00 00 02 00 02 01 02 02` → 0, 512, 513, 514.
  BE max is 2040 (< 2048 tiles); LE max would be 65283. Non-decreasing for obj 0..690; obj 691..1023 = 0.
- animdata: `u16 BE n` @0 (= 29), `u16 BE tile_to_animate[32]` @2, `u16 BE first_anim_frame[32]`
  @66, `u8 and_mask[32]` @130, `u8 shift[32]` @162. Each tick: `tileindex[tile_to_animate[i]] =
  tileindex[first_anim_frame[i] + ((counter & and_mask[i]) >> shift[i])]` (tile_manager.cpp:351-370).
  Proof: BE gives n = 29 and contains tiles 862, 1009 and 1020, the exact tiles Nuvie
  hard-codes (:490-492). LE gives n = 7424. Dump `00 1d 00 08 00 09`.

### 4.3 schedule (3329 = 514 + 563×5)
- `u16 BE first[256]` @0 = index of each actor's first record. `u16 BE total` @512 = 563.
  Records start @514. For actor `a`: if `first[a] > total−1`, count = 0; else if `a == 255` or
  `first[a+1] > total−1`, count = `total − first[a]`; else count = `first[a+1] − first[a]`
  (actor_manager.cpp:686-703).
- Record, 5 bytes, byte-oriented (actor.cpp:1194-1204): `[0]` bits0-4 hour, bits5-7 day-of-week;
  `[1]` worktype; `[2..5)` x,y,z packed as in §5 bytes 1-3.
- Proof: BE `first[]` is non-decreasing, and LE would need 65804 bytes. Record 0 =
  `08 92 33 71 05` → hour 8, worktype 0x92, (307,348,0), which is actor 5's objlist position (§6).

### 4.4 look.lzd (LZW → 5914)
- Entries `[u16 LE tile][NUL-terminated ASCII]`, ascending tile. Stop at tile ≥ 2048 (look.cpp:75-95).
  A tile without its own entry uses the next entry's string (:93-94). Strings carry plural
  markers `\` and `/` (:125). Measured: 512 entries, last tile 2038, terminator 0x0800 at
  decoded offset 5904. Dump `00 00 "Nothing" 00 01 00 "grass" 00 04 00 "swamp"`.

### 4.5 fonts
- u6.ch: 256 glyphs × 8 bytes; one byte per row, bit 7 = leftmost pixel. Glyphs 0-127 are
  English, 128-255 runic/gargish (font_manager.cpp:85-95, u6_font.cpp:62-70). 'A' = `1e 36 66 7e 66 66 c6 00`.
- u6.set (LZW → 7004): `[0]` height (8), `[2]` ink value (15), `[4+c]` width, `[0x104+c]` offset lo,
  `[0x204+c]` offset hi. Glyph = width×height bytes at `hi*256+lo`; a byte equal to ink = set pixel
  (wou_font.cpp:105-118). Measured max glyph end = 7004 = decoded size.

### 4.6 bitmaps (paper.bmp, newmagic.bmp, worldmap.bmp)
- LZW → `u16 LE width, u16 LE height`, then `width×height` u8 palette indices, row-major
  (u6_bmp.cpp:41-62). Measured: paper 320×200, newmagic 144×82, worldmap 128×128; each decoded
  size = 4 + w·h. Under BE the header would need ≥ 773 MB.

## 5. Object blocks — `OBJBLK??` (d2/DATA1, 69 files)

- Names: `OBJBLK` + X + Y, with X, Y ∈ A..H = superchunk column and row. The surface index is
  `s = (Y−'A')·8 + (X−'A')`, the same `s` as in map §4.1 (save_game.cpp:150-160). `OBJBLK{A..E}I` =
  dungeon levels z = 1..5. All 64 + 5 are present; the names are uppercase on Amiga, while Nuvie
  wants lowercase `savegame/objblkxy`.
- File = `u16 BE count`, then `count` × 8-byte records. `2 + 8·count == size` holds for all 69
  (OBJBLKAB `00 d0` = 208 → 1666).

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
- Status histogram over 16838 records: map 15071, container 1638, inventory 62, readied 67.

Proof, per field:
- x,y,z: all 8275 surface on-map records lie inside their own file's superchunk with z = 0
  (OBJBLKAB `00 45 c4 02` → (69,177,0); OBJBLKHH `00 84 03 0e` → (900,896,0)). Dungeon on-map
  records have z = 1..5 for AI..EI and max x 254.
- obj_n: under BE every record is ≤ 431 = `OBJ_U6__LAST_` (u6_objects.h:400). Under LE the first
  OBJBLKHH record is obj 769. OBJBLKHH BE `01 33` → 307 volcano, `00 ab` → 171 fumarole
  (u6_objects.h:289,200); OBJBLKAB → 284 rope, 414 skiff.
- quality vs qty:
  - Signs (332): byte 6 = 1..N, each distinct; byte 7 = 0 in 126/126. Byte 6 is therefore the
    book index, i.e. quality (u6_usecode.cpp:2415). Books, scrolls and tombstones look the same
    (byte 6 = 56.., 46..54, 78..115).
  - Eggs (335): byte 7 ∈ {10,20,25,30,33,50,60,66,75,80,90,99,100,101} = hatch percent = qty.
    Byte 6 ∈ 0..24 = alignment+period = quality (egg_manager.cpp:121,162-175).
  - Stackables: 536 have byte 6 = 0 and byte 7 ≠ 0 (meat portion `00 81 00 01` → qty 1). Gold in
    containers `00 58 01 2c` → 300 and `00 58 02 58` → 600.

## 6. objlist (d2/DATA1, 7539 = 0x1D73 bytes)

Actor arrays are indexed by actor number 0..255. `AM` = actors/actor_manager.cpp. Not-read
regions are listed so that a parser can skip them.

| offset | layout | field | reader |
|---|---|---|---|
| 0x0000 | u8[256] | obj_flags | AM:158-162 |
| 0x0100 | 3 B[256] | x,y,z (as §5 bytes 1-3) | AM:104-136 |
| 0x0400 | **u16 BE**[256] | obj_n/frame (`v&0x3FF`, `v>>10`) | AM:140-155 |
| 0x0600 | **u16 BE**[256] [추정] | unknown; not read by Nuvie; all 200 nonzero bytes sit at odd offsets (`00 01 00 01 …`) | — |
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
| 0x1BFB | **u16 BE**[4] [추정] | unknown, not read; `00 07 00 c5 00 e2 00 00` | — |
| 0x1C03 | u8[16] | timers; [15] = eclipse (0x1C12) | game_clock.cpp:90-93; game_clock.h:32,38 |
| 0x1C13 | **u16 BE** | vanish obj (`>1023` encodes frame) | L: init.lua:57,366-377 |
| 0x1C17 | u8 | alcohol | player.cpp:91 |
| 0x1C1B | **u16 BE**[8][3] | moonstone x, y, z | L: init.lua:58,380-389 |
| 0x1C4B | **u16 BE** | keg timer | L: init.lua:59,391-392 |
| 0x1C5F / 0x1C69 / 0x1C6A | u8 each | gargish flag / combat mode / solo member (0xFF = party) | player.cpp:94-98; party.cpp:112 |
| 0x1C6C / 0x1C71 / 0x1C72 | u8 each | command-bar selection / gender / avatar portrait (1-based) | command_bar.cpp:113; player.cpp:100; portrait_u6.cpp:80-82 |
| 0x1C73..0x1D72 | 256 B | residue, never read: ASCII text fragments mixed with LE-looking words | — |

Proof: under BE, all 256 actor obj_n values are ≤ 431; under LE, 118 are invalid. Actor 1 =
(307,352,0) obj 410 = `OBJ_U6_AVATAR` (u6_objects.h:372). Actors 2-4 have exp 374/373/372
(LE: 30209/29953/29697). Year `00 a1` = 161. Moonstone 0 = (935,262,0); every BE x,y is ≤ 1023, and every LE x,y is
> 1023 (moonstone 6 LE z = 256). Names at 0xF00 read "Avatar", "Dupre", "Shamino", "Iolo".
Time = 08:00, day 4, month 7.

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
| Amiga (d2/DATA1) | size | measured | DOS target (core/tile_manager.cpp) |
|---|---|---|---|
| maptiles.ega | 76612 (PP) | not LZW: `da 3b 00 10 00 00 ff 7f` | maptiles.vga: LZW → 8-bit 16×16 tiles, 256 B each (:132-150) |
| objtiles.ega | 265248 (PP) | `00 0f 00 ff 7f df 7d e6` | objtiles.vga: raw, appended after decoded maptiles (:183-191) |
| tileindx.ega | 4096 | 2048 × u16, monotonic in neither order (BE 96, 9312, …; LE 24576, 24612, …) | tileindx.vga: u16 LE[2048], ×16 = byte offset into maptiles+objtiles (:195-200) |
| masktype.ega | 341 | LZW → 2048 B, values 0..7 (0:471 1:47 2:41 3:1391 4:17 5:2 7:79) | masktype.vga: LZW → u8[2048] ∈ {0 plain, 5 transparent, 0xA pixel-block} (:202-221; .h:36-38) |
| animmask.ega | 15842 (PP) | not LZW: `b4 3a 0a 04 04 40 00 00` | animmask.vga: LZW → 32 × 64-B run masks for tiles 16..47 (:565-600) |

The DOS pixel-block tile is `[0]` count, then `{u16 LE disp, u8 len, len bytes}` until len = 0;
`x = disp%160 + (disp ≥ 1760 ? 160 : 0)` (decodePixelBlockTile, :510). The dither step
(`dither_bitmap`, :224) runs per tile after decode.

### 8.2 Other gaps
- Palette: no `u6pal` on Amiga. DOS = 256 × (r,g,b) 6-bit, shifted left by 2
  (game_palette.cpp:75-80); background index 0x31 (:95-99). The Amiga palette source is
  unknown (game exe RE) [추정].
- Cursor: DOS `u6mcga.ptr` = LZW → lib (u32 LE size + u32 LE offsets) → U6Shape with u16 LE fields
  (cursor.cpp:83-104; u6_shape.cpp:175-215). Amiga `INTRO.PTR` (d1, raw) = LZW → 1447 B, inner
  words LE-plausible (`f5 01 00 00 28 00 00 00 df 00 …`; item at 40 = `08 00 01 00 01 00 0c 00`).
  Whether it holds the main-game cursors [추정: intro only] is unknown.
- Cutscenes: 14 files in d1 (`blocks gypsy intro intro_1a intro_1b intro_2 intro_2a mainmenu
  montage title vellum1 woods` + `.new`, plus `INTRO_1.NEW INTRO_3.NEW`) and d2 `end.new` are whole-file LZW. The
  decoded data opens with a BE32 table (blocks.new `00 00 00 10 00 00 0f ba 00 00 2b 1c`;
  vellum1.new `00 00 00 4c 00 00 63 e6`). DOS `.shp` = LZW → lib → U6Shape
  (script_cutscene.cpp:1276-1291). The inner format is pending (milestone-1 non-goal, .prd/01-spec.md).
- Music: `ULTIMA_{A..H}a.MOD` = MMD0 (OctaMED, magic `MMD0`). DOS = Adlib `.m`. Non-goal for milestone 1.
- STARPOS.DAT (755, raw): U6-LZW → 1200 B. As u16 LE, values 0..299 are ≤ 314 and values 300..599 are
  ≤ 119 (as BE, up to 64256). Reading: 300 stars, `x[300]` then `y[300]`, u16 LE [추정]. No Nuvie reader.

## Tensions

- The dispatcher brief said basetile is "monotonic". Measured: BE is non-decreasing for obj
  0..690 only, then drops to 0 for obj 691..1023 (2040 → 0 at 690→691).
- objlist 0x0600 and 0x1BFB are marked BE u16 only from byte parity; nothing reads them in Nuvie.
  Settle in the exe RE.
- "converse.* identical to DOS" rests on the first-5000-byte md5 and an equal size; the
  remainder has not been checked against a DOS copy.

## Reproduce (paths outside the repo; run with `python3 -I`)

    D=~/Games/ultima6/unpacked/d2/DATA1
    # every OBJBLK: BE count fits size, BE obj_n <= 431
    python3 -I -c "import os,sys;d=sys.argv[1];[print(f) for f in sorted(os.listdir(d)) if f.startswith('OBJBLK') and (lambda b:(int.from_bytes(b[:2],'big')*8+2!=len(b)) or any((((b[2+i*8+4]<<8)|b[2+i*8+5])&0x3ff)>431 for i in range(int.from_bytes(b[:2],'big'))))(open(os.path.join(d,f),'rb').read())]" $D   # prints nothing
    # objlist year (BE u16 @0x1BF7) -> 161
    python3 -I -c "import sys;b=open(sys.argv[1],'rb').read();print(int.from_bytes(b[0x1bf7:0x1bf9],'big'))" $D/objlist
    # schedule: 514 + 5*total == size -> True
    python3 -I -c "import sys;b=open(sys.argv[1],'rb').read();print(514+5*int.from_bytes(b[512:514],'big')==len(b))" ~/Games/ultima6/unpacked/d1/INSTALL/schedule
