---
title: ultima6 Amiga executables — hunk layout
type: note
tags: [ultima6, re, amiga, hunk]
---

# Amiga executables — hunk layout

Measured 2026-10-14 with `tools/hunk2raw.py` at the default base `0x00100000`.
Inputs: `~/Games/ultima6/unpacked/d1/INSTALL/<name>` (PP20-decrunched; `installu6` is
byte-identical to the uncrunched `files/d1/INSTALL/installu6`, `cmp` clean). Outputs stay
outside the repo: `~/Games/ultima6/re/<name>.{bin,json}`.

    python3 -I tools/hunk2raw.py <in> ~/Games/ultima6/re/<name>.bin ~/Games/ultima6/re/<name>.json

Ghidra import: raw binary, `68000:BE:32:default`, base = entry = `0x00100000`. Cross-check: a
throwaway scanner (not committed) agreed with the tool on every hunk's type, loaded bytes and
reloc count in all five files; `image_size` = last base + size − base; no tool warnings (incl.
the check that each reloc's stored offset ≤ the target segment size).

## Summary

| file | input bytes | input sha256 (16) | hunks CODE/DATA/BSS | CHIP hunks | relocs | symbols | overlay | entry | image bytes |
|---|---|---|---|---|---|---|---|---|---|
| `game` | 267,332 | `34a6e9dd66d58ff2` | 35 = 26/5/4 | 28, 31 (DATA) | 2,855 | 0 | none | 0x00100000 | 402,736 |
| `u` | 87,316 | `2e9267dbb9fb509d` | 24 = 18/5/1 | 16 (DATA), 17 (CODE), 20 (DATA) | 643 | 0 | none | 0x00100000 | 106,512 |
| `ultima6` | 9,212 | `79ebabd9281f105c` | 7 = 5/2/0 | 2 (DATA) | 48 | 0 | none | 0x00100000 | 9,320 |
| `ultcli` | 8,512 | `dfd96f65b66fdccd` | 7 = 5/2/0 | 2 (DATA) | 45 | 0 | none | 0x00100000 | 8,632 |
| `installu6` | 20,192 | `580ff939de83bea6` | 11 = 9/1/1 | 3 (CODE) | 339 | 0 | none | 0x00100000 | 289,380 |

All five: no resident libraries, `first` = 0, `table_size` = `last` + 1. The only block types
present are HEADER, CODE, DATA, BSS, RELOC32, END — no SYMBOL, DEBUG, NAME, EXT,
RELOC32SHORT/0x3F7 or OVERLAY/BREAK. Those tool paths are exercised only by the synthetic
tests in `tools/test_hunk2raw.py`, not by real data.

### `game` segments

| # | type | mem | base | size | loaded | relocs |
|---|---|---|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 | 0x1eb0 | 0x1eb0 | 45 |
| 1 | CODE | ANY | 0x00101eb0 | 0x3100 | 0x3100 | 130 |
| 2 | CODE | ANY | 0x00104fb0 | 0x3a28 | 0x3a28 | 93 |
| 3 | CODE | ANY | 0x001089d8 | 0x10dc | 0x10dc | 48 |
| 4 | CODE | ANY | 0x00109ab8 | 0x964 | 0x964 | 13 |
| 5 | CODE | ANY | 0x0010a420 | 0x4c60 | 0x4c60 | 177 |
| 6 | BSS | ANY | 0x0010f080 | 0x171c | 0x0 | 0 |
| 7 | CODE | ANY | 0x001107a0 | 0x240 | 0x240 | 10 |
| 8 | CODE | ANY | 0x001109e0 | 0x1bd4 | 0x1bd4 | 73 |
| 9 | CODE | ANY | 0x001125b8 | 0x318 | 0x318 | 12 |
| 10 | CODE | ANY | 0x001128d0 | 0x2958 | 0x2958 | 113 |
| 11 | CODE | ANY | 0x00115228 | 0x5f94 | 0x5f94 | 168 |
| 12 | CODE | ANY | 0x0011b1c0 | 0x6944 | 0x6944 | 345 |
| 13 | CODE | ANY | 0x00121b08 | 0x5770 | 0x5770 | 238 |
| 14 | CODE | ANY | 0x00127278 | 0x113c | 0x113c | 46 |
| 15 | CODE | ANY | 0x001283b8 | 0x4500 | 0x4500 | 188 |
| 16 | CODE | ANY | 0x0012c8b8 | 0x4880 | 0x4880 | 175 |
| 17 | CODE | ANY | 0x00131138 | 0x1200 | 0x1200 | 46 |
| 18 | CODE | ANY | 0x00132338 | 0x2238 | 0x2238 | 70 |
| 19 | CODE | ANY | 0x00134570 | 0xf8 | 0xf8 | 2 |
| 20 | DATA | ANY | 0x00134668 | 0x38 | 0x38 | 0 |
| 21 | CODE | ANY | 0x001346a0 | 0x868 | 0x868 | 16 |
| 22 | CODE | ANY | 0x00134f08 | 0x680 | 0x680 | 41 |
| 23 | BSS | ANY | 0x00135588 | 0x114b8 | 0x0 | 0 |
| 24 | BSS | ANY | 0x00146a40 | 0x7530 | 0x0 | 0 |
| 25 | BSS | ANY | 0x0014df70 | 0xf00 | 0x0 | 0 |
| 26 | CODE | ANY | 0x0014ee70 | 0x454 | 0x454 | 24 |
| 27 | DATA | ANY | 0x0014f2c8 | 0x2000 | 0x2000 | 0 |
| 28 | DATA | CHIP | 0x001512c8 | 0x83c | 0x83c | 0 |
| 29 | CODE | ANY | 0x00151b08 | 0x3d4 | 0x3d4 | 17 |
| 30 | CODE | ANY | 0x00151ee0 | 0x16f0 | 0x16f0 | 210 |
| 31 | DATA | CHIP | 0x001535d0 | 0x4 | 0x4 | 0 |
| 32 | CODE | ANY | 0x001535d8 | 0x118 | 0x118 | 2 |
| 33 | DATA | ANY | 0x001536f0 | 0xe7d8 | 0x4f0c | 421 |
| 34 | CODE | ANY | 0x00161ec8 | 0x668 | 0x668 | 132 |

### `u` segments

| # | type | mem | base | size | loaded | relocs |
|---|---|---|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 | 0x29cc | 0x29cc | 45 |
| 1 | CODE | ANY | 0x001029d0 | 0xe60 | 0xe60 | 38 |
| 2 | CODE | ANY | 0x00103830 | 0x1ea0 | 0x1ea0 | 34 |
| 3 | DATA | ANY | 0x001056d0 | 0x30 | 0x30 | 0 |
| 4 | CODE | ANY | 0x00105700 | 0x19bc | 0x19bc | 41 |
| 5 | CODE | ANY | 0x001070c0 | 0x720 | 0x720 | 13 |
| 6 | CODE | ANY | 0x001077e0 | 0x5c | 0x5c | 0 |
| 7 | CODE | ANY | 0x00107840 | 0x202c | 0x202c | 25 |
| 8 | CODE | ANY | 0x00109870 | 0x1398 | 0x1398 | 29 |
| 9 | CODE | ANY | 0x0010ac08 | 0x493c | 0x493c | 8 |
| 10 | CODE | ANY | 0x0010f548 | 0x5a0 | 0x5a0 | 13 |
| 11 | CODE | ANY | 0x0010fae8 | 0x74 | 0x74 | 4 |
| 12 | DATA | ANY | 0x0010fb60 | 0x4 | 0x4 | 0 |
| 13 | BSS | ANY | 0x0010fb68 | 0x4000 | 0x0 | 0 |
| 14 | CODE | ANY | 0x00113b68 | 0x504 | 0x504 | 23 |
| 15 | CODE | ANY | 0x00114070 | 0x2b8 | 0x2b8 | 4 |
| 16 | DATA | CHIP | 0x00114328 | 0x584 | 0x584 | 0 |
| 17 | CODE | CHIP | 0x001148b0 | 0x47c | 0x47c | 5 |
| 18 | CODE | ANY | 0x00114d30 | 0x338 | 0x338 | 17 |
| 19 | CODE | ANY | 0x00115068 | 0x1610 | 0x1610 | 259 |
| 20 | DATA | CHIP | 0x00116678 | 0x4 | 0x4 | 0 |
| 21 | CODE | ANY | 0x00116680 | 0x118 | 0x118 | 2 |
| 22 | DATA | ANY | 0x00116798 | 0x3160 | 0x1718 | 20 |
| 23 | CODE | ANY | 0x001198f8 | 0x718 | 0x718 | 63 |

### `installu6` segments

| # | type | mem | base | size | loaded | relocs |
|---|---|---|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 | 0x1a48 | 0x1a48 | 3 |
| 1 | CODE | ANY | 0x00101a48 | 0x1910 | 0x1910 | 27 |
| 2 | BSS | ANY | 0x00103358 | 0x41eb0 | 0x0 | 0 |
| 3 | CODE | CHIP | 0x00145208 | 0x560 | 0x560 | 12 |
| 4 | DATA | ANY | 0x00145768 | 0xfe0 | 0xc34 | 275 |
| 5 | CODE | ANY | 0x00146748 | 0xf8 | 0xf8 | 2 |
| 6 | CODE | ANY | 0x00146840 | 0x94 | 0x94 | 7 |
| 7 | CODE | ANY | 0x001468d8 | 0x14 | 0x14 | 0 |
| 8 | CODE | ANY | 0x001468f0 | 0xdc | 0xdc | 9 |
| 9 | CODE | ANY | 0x001469d0 | 0x24 | 0x24 | 2 |
| 10 | CODE | ANY | 0x001469f8 | 0x6c | 0x6c | 2 |

### `ultima6` / `ultcli` segments (same hunk types and memory flags)

| # | type | mem | ultima6 base | size | loaded | relocs | ultcli base | size | loaded | relocs |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 | 0x35c | 0x35c | 9 | 0x00100000 | 0x35c | 0x35c | 9 |
| 1 | CODE | ANY | 0x00100360 | 0x580 | 0x580 | 6 | 0x00100360 | 0x510 | 0x510 | 5 |
| 2 | DATA | CHIP | 0x001008e0 | 0xd0 | 0xd0 | 0 | 0x00100870 | 0xd0 | 0xd0 | 0 |
| 3 | CODE | ANY | 0x001009b0 | 0x14 | 0x14 | 1 | 0x00100940 | 0x14 | 0x14 | 1 |
| 4 | CODE | ANY | 0x001009c8 | 0x118 | 0x118 | 2 | 0x00100958 | 0x118 | 0x118 | 2 |
| 5 | DATA | ANY | 0x00100ae0 | 0x638 | 0x400 | 28 | 0x00100a70 | 0x5ec | 0x3b8 | 26 |
| 6 | CODE | ANY | 0x00101118 | 0x1350 | 0x1350 | 2 | 0x00101060 | 0x1158 | 0x1158 | 2 |

## Entry and startup code

Entry = hunk 0 base `0x00100000` in all five. Read off the image; reloc targets from hunk 0's
RELOC32 entries. The listing itself is not reproduced; what it does:

`game`, hunk 0 `+0x00..+0x2f`: save d1-d6/a0-a6 on the stack (`+00`), keep a0 in a2 (`+04`)
and d0 in d2 (`+06`), point a4 at `0x0015b6f0` (`+08`, RELOC32 → hunk 33 (DATA) + 0x8000),
load AbsExecBase (`4.w`) into a6 (`+0e`), point a3 at `0x001585fc` (`+12`, RELOC32 → hunk 33
+ 0x4f0c), then clear `0x2633 + 1` longwords from a3 up with a `dbf` loop (`+18..+24`), and store
a7 at `-$30b4(a4)` (`+28`) and a6 at `-$30bc(a4)` (`+2c`).

Over hunk 0 `+0x000..+0x1f3`, `game`, `u` and `installu6` are byte-identical except for
relocated longwords and 2-byte fields: `+0x1e` (BSS-length immediate), `+0x2a`/`+0x2e` (d16 of
the two a4-relative moves above), more further on (not decoded one by one). All three continue
at `+0x30` with a `clr.l (d16,a4)` and at `+0x34` with exec SetSignal(0, 0x3000) (LVO
−0x132), and hold `dos.library` at hunk 0 `+0x1e7`.

`ultima6`/`ultcli` use a different prologue: the same register save, then a5 = a7 + 0x34, a2 /
d2 as above, a4 → hunk 5 + 0, a6 = AbsExecBase, a6 stored at `$5a(a4)`, a7 at `$62(a4)`,
`$5e(a4)` cleared, a3 → hunk 5 + loaded size, then the same clear loop. Their `dos.library`
string is in DATA hunk 5 `+0x7a`, not in code.

### BSS tail cleared by the startup loop (measured)

| file | a4 = | a3 = | d0 (longs) | hunk size | loaded | loaded + d0×4 |
|---|---|---|---|---|---|---|
| `game` | h33 + 0x8000 | h33 + 0x4f0c | 0x2633 | 0xe7d8 | 0x4f0c | 0xe7d8 |
| `u` | h22 + 0 | h22 + 0x1718 | 0x692 | 0x3160 | 0x1718 | 0x3160 |
| `installu6` | h4 + 0 | h4 + 0xc34 | 0xeb | 0xfe0 | 0xc34 | 0xfe0 |
| `ultima6` | h5 + 0 | h5 + 0x400 | 0x8e | 0x638 | 0x400 | 0x638 |
| `ultcli` | h5 + 0 | h5 + 0x3b8 | 0x8d | 0x5ec | 0x3b8 | 0x5ec |

So the loop zero-fills exactly the not-loaded tail of the hunk a4 points into (data + BSS).

### Compiler identification

- [추정] SAS/C (Lattice C) runtime startup `c.o`: the sequence matches its published shape —
  save registers, keep a0/d0 in a2/d2, `lea _LinkerDB,a4`, ExecBase in a6, `lea __BSSBAS,a3` /
  `move.l #__BSSLEN,d0` / `move.l d1,(a3)+` / `dbf` — and the measured BSS-tail arithmetic is
  what a merged data+BSS hunk (`__MERGED`) addressed through a4 produces. `ultima6`/`ultcli`
  are [추정] a different variant of the same family.
- No string confirms this: none of `$VER:`, `SAS/C`, `Lattice`, `LATTICE`, `Manx`, `Aztec`,
  `AZTEC`, `DICE`, `GNU`, `GCC`, `gcc`, `__MERGED` occurs in any image; no symbol hunks. The
  substring `SAS` occurs 3× in `game` only, all inside binary bytes (h12 CODE
  `+0x57c4`/`+0x5836` and `0x00152340`, h30 CODE `+0x460`) — not a signature.
- [추정] `game`'s a4 bias of +0x8000 (the others use +0) lets signed 16-bit displacements
  reach its 0xe7d8-byte data hunk; its startup displacements are negative (`-$30b4`).

### Strings that locate subsystems (image addresses)

- `game`: `dos.library` 0x001001e7 and `audio.device` 0x00101206 (h0); `intuition.library`
  0x0014eec6, `graphics.library` 0x0014eed8 (h26); `input.device` 0x00151c4e (h29); the song
  file names from 0x00110806 (h7 `+0x66`; the table at A4-0x779a points at them).
- `u`: the title and the transfer menu's prompt (h5, h8; e.g. 0x00109a5a). `ultima6`: the line
  that refuses a start outside Workbench (h1 `+0x4c0`). `installu6`: `trackdisk.device`
  (h3 `+0x9a`), `NIL:` (h0 `+0x67c`).
