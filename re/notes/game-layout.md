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

The hunks' sizes and loaded lengths, the file sizes and the reloc counts are values of the
user's files and are not reproduced: they come from the hunk headers at run time
(`tools/hunk2raw.py`; the port's `crates/u6-core/src/image.rs`). The tables keep each hunk's
number, what it holds (type and memory) and its base address in the flattened image.

## Summary

| file | input sha256 (16) | hunks CODE/DATA/BSS | CHIP hunks | symbols | overlay | entry |
|---|---|---|---|---|---|---|
| `game` | `34a6e9dd66d58ff2` | 35 = 26/5/4 | 28, 31 (DATA) | 0 | none | 0x00100000 |
| `u` | `2e9267dbb9fb509d` | 24 = 18/5/1 | 16 (DATA), 17 (CODE), 20 (DATA) | 0 | none | 0x00100000 |
| `ultima6` | `79ebabd9281f105c` | 7 = 5/2/0 | 2 (DATA) | 0 | none | 0x00100000 |
| `ultcli` | `dfd96f65b66fdccd` | 7 = 5/2/0 | 2 (DATA) | 0 | none | 0x00100000 |
| `installu6` | `580ff939de83bea6` | 11 = 9/1/1 | 3 (CODE) | 0 | none | 0x00100000 |

All five: no resident libraries, `first` = 0, `table_size` = `last` + 1. The only block types
present are HEADER, CODE, DATA, BSS, RELOC32, END — no SYMBOL, DEBUG, NAME, EXT,
RELOC32SHORT/0x3F7 or OVERLAY/BREAK. Those tool paths are exercised only by the synthetic
tests in `tools/test_hunk2raw.py`, not by real data.

### `game` segments

| # | type | mem | base |
|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 |
| 1 | CODE | ANY | 0x00101eb0 |
| 2 | CODE | ANY | 0x00104fb0 |
| 3 | CODE | ANY | 0x001089d8 |
| 4 | CODE | ANY | 0x00109ab8 |
| 5 | CODE | ANY | 0x0010a420 |
| 6 | BSS | ANY | 0x0010f080 |
| 7 | CODE | ANY | 0x001107a0 |
| 8 | CODE | ANY | 0x001109e0 |
| 9 | CODE | ANY | 0x001125b8 |
| 10 | CODE | ANY | 0x001128d0 |
| 11 | CODE | ANY | 0x00115228 |
| 12 | CODE | ANY | 0x0011b1c0 |
| 13 | CODE | ANY | 0x00121b08 |
| 14 | CODE | ANY | 0x00127278 |
| 15 | CODE | ANY | 0x001283b8 |
| 16 | CODE | ANY | 0x0012c8b8 |
| 17 | CODE | ANY | 0x00131138 |
| 18 | CODE | ANY | 0x00132338 |
| 19 | CODE | ANY | 0x00134570 |
| 20 | DATA | ANY | 0x00134668 |
| 21 | CODE | ANY | 0x001346a0 |
| 22 | CODE | ANY | 0x00134f08 |
| 23 | BSS | ANY | 0x00135588 |
| 24 | BSS | ANY | 0x00146a40 |
| 25 | BSS | ANY | 0x0014df70 |
| 26 | CODE | ANY | 0x0014ee70 |
| 27 | DATA | ANY | 0x0014f2c8 |
| 28 | DATA | CHIP | 0x001512c8 |
| 29 | CODE | ANY | 0x00151b08 |
| 30 | CODE | ANY | 0x00151ee0 |
| 31 | DATA | CHIP | 0x001535d0 |
| 32 | CODE | ANY | 0x001535d8 |
| 33 | DATA | ANY | 0x001536f0 |
| 34 | CODE | ANY | 0x00161ec8 |

### `u` segments

| # | type | mem | base |
|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 |
| 1 | CODE | ANY | 0x001029d0 |
| 2 | CODE | ANY | 0x00103830 |
| 3 | DATA | ANY | 0x001056d0 |
| 4 | CODE | ANY | 0x00105700 |
| 5 | CODE | ANY | 0x001070c0 |
| 6 | CODE | ANY | 0x001077e0 |
| 7 | CODE | ANY | 0x00107840 |
| 8 | CODE | ANY | 0x00109870 |
| 9 | CODE | ANY | 0x0010ac08 |
| 10 | CODE | ANY | 0x0010f548 |
| 11 | CODE | ANY | 0x0010fae8 |
| 12 | DATA | ANY | 0x0010fb60 |
| 13 | BSS | ANY | 0x0010fb68 |
| 14 | CODE | ANY | 0x00113b68 |
| 15 | CODE | ANY | 0x00114070 |
| 16 | DATA | CHIP | 0x00114328 |
| 17 | CODE | CHIP | 0x001148b0 |
| 18 | CODE | ANY | 0x00114d30 |
| 19 | CODE | ANY | 0x00115068 |
| 20 | DATA | CHIP | 0x00116678 |
| 21 | CODE | ANY | 0x00116680 |
| 22 | DATA | ANY | 0x00116798 |
| 23 | CODE | ANY | 0x001198f8 |

### `installu6` segments

| # | type | mem | base |
|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 |
| 1 | CODE | ANY | 0x00101a48 |
| 2 | BSS | ANY | 0x00103358 |
| 3 | CODE | CHIP | 0x00145208 |
| 4 | DATA | ANY | 0x00145768 |
| 5 | CODE | ANY | 0x00146748 |
| 6 | CODE | ANY | 0x00146840 |
| 7 | CODE | ANY | 0x001468d8 |
| 8 | CODE | ANY | 0x001468f0 |
| 9 | CODE | ANY | 0x001469d0 |
| 10 | CODE | ANY | 0x001469f8 |

### `ultima6` / `ultcli` segments (same hunk types and memory flags)

| # | type | mem | ultima6 base | ultcli base |
|---|---|---|---|---|
| 0 | CODE | ANY | 0x00100000 | 0x00100000 |
| 1 | CODE | ANY | 0x00100360 | 0x00100360 |
| 2 | DATA | CHIP | 0x001008e0 | 0x00100870 |
| 3 | CODE | ANY | 0x001009b0 | 0x00100940 |
| 4 | CODE | ANY | 0x001009c8 | 0x00100958 |
| 5 | DATA | ANY | 0x00100ae0 | 0x00100a70 |
| 6 | CODE | ANY | 0x00101118 | 0x00101060 |

## Entry and startup code

Entry = hunk 0 base `0x00100000` in all five. Read off the image; reloc targets from hunk 0's
RELOC32 entries. The listing itself is not reproduced; what it does:

`game`, hunk 0 `+0x00..+0x2f`: save d1-d6/a0-a6 on the stack (`+00`), keep a0 in a2 (`+04`)
and d0 in d2 (`+06`), point a4 at `0x0015b6f0` (`+08`, RELOC32 → hunk 33 (DATA) + 0x8000),
load AbsExecBase (`4.w`) into a6 (`+0e`), point a3 at the end of hunk 33's loaded part (`+12`,
a RELOC32 into hunk 33), then clear the rest of that hunk from a3 up with a `dbf` loop
(`+18..+24`, its count the immediate at `+1a`), and store a7 at `-$30b4(a4)` (`+28`) and a6 at
`-$30bc(a4)` (`+2c`).

Over hunk 0 `+0x000..+0x1f3`, `game`, `u` and `installu6` are byte-identical except for
relocated longwords and 2-byte fields: `+0x1e` (BSS-length immediate), `+0x2a`/`+0x2e` (d16 of
the two a4-relative moves above), more further on (not decoded one by one). All three continue
at `+0x30` with a `clr.l (d16,a4)` and at `+0x34` with exec SetSignal(0, 0x3000) (LVO
−0x132), and hold `dos.library` at hunk 0 `+0x1e7`.

`ultima6`/`ultcli` use a different prologue: the same register save, then a5 = a7 + 0x34, a2 /
d2 as above, a4 → hunk 5 + 0, a6 = AbsExecBase, a6 stored at `$5a(a4)`, a7 at `$62(a4)`,
`$5e(a4)` cleared, a3 → hunk 5 + loaded size, then the same clear loop. Their `dos.library`
string is in DATA hunk 5 `+0x7a`, not in code.

### BSS tail cleared by the startup loop

| file | a4 = |
|---|---|
| `game` | h33 + 0x8000 |
| `u` | h22 + 0 |
| `installu6` | h4 + 0 |
| `ultima6` | h5 + 0 |
| `ultcli` | h5 + 0 |

In each file a3 points at the end of the loaded part of the hunk a4 points into, and the loop
count is the longwords from there to the hunk's end (both checked against the hunk headers). So
the loop zero-fills exactly the not-loaded tail of that hunk (data + BSS).

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
  reach all of its data hunk, which is longer than 0x8000 bytes; its startup displacements are
  negative (`-$30b4`).

### Strings that locate subsystems (image addresses)

- `game`: `dos.library` 0x001001e7 and `audio.device` 0x00101206 (h0); `intuition.library`
  0x0014eec6, `graphics.library` 0x0014eed8 (h26); `input.device` 0x00151c4e (h29); the song
  file names from 0x00110806 (h7 `+0x66`; the table at A4-0x779a points at them).
- `u`: the title and the transfer menu's prompt (h5, h8; e.g. 0x00109a5a). `ultima6`: the line
  that refuses a start outside Workbench (h1 `+0x4c0`). `installu6`: `trackdisk.device`
  (h3 `+0x9a`), `NIL:` (h0 `+0x67c`).
