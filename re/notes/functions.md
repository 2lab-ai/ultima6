---
title: ultima6 game functions - index
type: note
tags: [ultima6, re, amiga, functions]
---

# Amiga `game` executable: function inventory (index)

All 1383 REA procedures of `game` (base 0x00100000, A4 = 0x15B6F0 [likely]) = 599 functions + 784 jump stubs (`thunk_FUN_*`: a 6-byte `JMP abs.l` at the end of a calling hunk, one per target that hunk calls). Generated from the local decompile dumps; no decompiler output is copied here, roles are one-line descriptions.

    python3 -I -B tools/rea/classify.py --notes re/notes   # also writes ~/Games/ultima6/re/inventory.json

Legend. `class`: crt = SAS/C runtime, os-wrapper, med = MED music player, file-io, graphics, input, game. `subsystem/conf`: conf h = curated or two distinctive strings, m = module default, one string, or a `[likely]` curated role, l = inferred from callers or an unreached MED-range function. `M1` = M in the milestone-1 set, S = stub point at its boundary (see subsystems.md). `stubs` = jump-stub addresses that jump to the function. `key evidence`: the address of a string the function uses (its text is not reproduced), library call (`?` = vector not checked by hand), custom-chip/CIA register, sole caller, or fan-in.

## Class histogram

| class | all procedures | functions only |
|---|---|---|
| game | 1091 | 465 |
| graphics | 96 | 41 |
| file-io | 81 | 23 |
| crt | 68 | 42 |
| os-wrapper | 21 | 10 |
| med | 19 | 14 |
| input | 7 | 4 |
| total | 1383 | 599 |

Strings: 428 of 515 listed strings are tied to a function (decompiler labels, small-data offsets, reloc pointer tables, LEA/PEA d16(PC) and absolute operands in the image).

## Hunks (one SAS/C module each)

| hunk | base | functions | stubs | default class/subsystem | module |
|---|---|---|---|---|---|
| 0 | 0x100000 | 55 | 1 | crt/runtime | SAS/C startup + MED player + C library |
| 1 | 0x101eb0 | 18 | 71 | game/main | command loop, timed events, per-turn status effects |
| 2 | 0x104fb0 | 41 | 56 | game/ui | command input, text output, view refresh, save/load front end |
| 3 | 0x1089d8 | 13 | 31 | game/map | map chunks and tile lookup |
| 4 | 0x109ab8 | 5 | 3 | game/map | map-view visibility / light grid |
| 5 | 0x10a420 | 58 | 17 | game/objects | object list, object blocks, object names |
| 7 | 0x1107a0 | 2 | 7 | game/sound | music start/stop |
| 8 | 0x1109e0 | 24 | 27 | game/ui | status panels, inventory and ready/unready |
| 9 | 0x1125b8 | 2 | 10 | game/converse | talk command, converse text substitutions |
| 10 | 0x1128d0 | 16 | 37 | game/converse | converse script interpreter |
| 11 | 0x115228 | 59 | 88 | game/magic | spellcasting and spell effects |
| 12 | 0x11b1c0 | 44 | 65 | game/actors | actor turns, NPC AI, schedules, party movement |
| 13 | 0x121b08 | 53 | 51 | game/combat | attacks, damage, monster specials |
| 14 | 0x127278 | 11 | 27 | game/party | rest/camp, ship repair, healing |
| 15 | 0x1283b8 | 38 | 75 | game/objects | look/get/drop/move/search, doors, traps, light |
| 16 | 0x12c8b8 | 47 | 87 | game/objects | use command and per-object use handlers |
| 17 | 0x131138 | 6 | 17 | game/unknown | direction/line helpers (purpose not pinned) |
| 18 | 0x132338 | 19 | 35 | game/ui | portrait/converse file loaders, view effects |
| 21 | 0x1346a0 | 26 | 9 | file-io/fileio | game file layer over dos.library, memory |
| 22 | 0x134f08 | 7 | 35 | game/boot | main(): initialisation and data loading |
| 26 | 0x14ee70 | 8 | 16 | os-wrapper/os | library open, memory, VBlank interrupt server |
| 29 | 0x151b08 | 4 | 5 | input/input | input.device handler and raw-key mapping |
| 30 | 0x151ee0 | 30 | 7 | graphics/graphics | tile data loading and blitting |
| 32 | 0x1535d8 | 1 | 2 | crt/runtime | SAS/C __main |
| 34 | 0x161ec8 | 12 | 5 | graphics/graphics | copper list / display setup |

## Files by subsystem

| file | subsystems (functions) |
|---|---|
| [functions-platform.md](functions-platform.md) | runtime 42, os 10, fileio 23, input 4, graphics 41, sound 16 |
| [functions-core.md](functions-core.md) | boot 7, main 13, time 1, saveload 3, util 2, unknown 4 |
| [functions-world.md](functions-world.md) | map 22, objects 143 |
| [functions-actors.md](functions-actors.md) | actors 37, schedules 2, party 17, combat 53 |
| [functions-magic.md](functions-magic.md) | magic 61 |
| [functions-converse.md](functions-converse.md) | converse 20 |
| [functions-ui.md](functions-ui.md) | ui 75, inventory 3 |

Subsystem map, main loop, dispatcher, porting order and the milestone-1 set: [subsystems.md](subsystems.md).
