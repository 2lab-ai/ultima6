---
title: ultima6 oracle — original Amiga Ultima VI under AROS (vAmigaWeb, headless)
type: note
tags: [ultima6, oracle, amiga, aros, vamiga, emulator]
---

# Oracle: original Amiga Ultima VI in an emulator

Measured 2026-10-14 on macOS 26.6.2 arm64. Harness = `tools/oracle/`. Everything that touches game
data stays in `~/Games/ultima6/oracle/` (disks, extracted tree, shots, logs).

**Result (WU-O2):** **Britannia is reached** under the AROS ROM, on floppies built from the
installer's own copy table (§A), with two documented deviations: a PAL fix applied in memory
after boot, and the `--game-first` disk set, which starts `game` directly with a "character
created" byte set, because `u` (intro, menu, character creation) stops in its intro (§C). The
party (Avatar, Dupre, Shamino, Iolo, 90 HP each) stands in Lord British's throne room in a fight
with gargoyles (DATA1's objlist has the combat byte 0x1C69 = 1; [추정] the game's normal opening);
three arrow-key presses that move the party each scroll the view by one tile (§D).
No character-creation screen was captured: `u` never reaches its menu.

Earlier result (WU-O, kept below): the installer runs under AROS, but the AROS-written floppy set
had broken files, and its boot stopped after the first intro scene; the HD layout crashes before
any picture (§5).

## A. The four floppies, from installu6's copy table (`run.sh floppies`)

`tools/oracle/mkadf.py floppies` reads everything from `files/d1/INSTALL/installu6` (sha256
`580ff939…`, flattened with `tools/hunk2raw.py` at base 0x100000) and file bytes from
`files/d{1,2,3}/` (the original disks). Evidence, image addresses:

| what | where | finding |
|---|---|---|
| copy table | `lea $24(a4),a3` 0x102ee6, entry size `moveq #$14,d1` 0x102f00; a4 = hunk 4 + 0 | 129 entries of 20 bytes: +0 src name, +4 dst volume (index into a4+0: PROGRAM, PORTRAIT, PLAYER, POPULACE), +5 flags, +6 dst name, +$a/+$e buffer/size, +$12 state, +$13 src disk (a4+$14: INSTALL, DATA1, DATA2) |
| which entries a path writes | 0x103000..0x103026 | written iff flags < 2, or floppy and flags & 2, or HD and flags & 4. Floppy: 126 files. HD only: `ultima6` (Workbench launcher), `ultima6.info`, `disk.info` (as `<dir>.info`). Floppy only: `ultcli` → `PROGRAM:ultima6`, `start` → `PROGRAM:s/startup-sequence`, `c/endcli` |
| decrunch | flags & 1 → 0x146748 before the write (0x102f1e/0x102f48) | every flag-1 source is PP20 and every PP20 source has flag 1; `tools/pp20.py` output = `unpacked/` byte for byte |
| target path | 0x102f8a..0x102fe2 | floppy: `<VOLUME>:` + dst name; HD: `<dir>/` + dst name |
| directories | 0x101cde/0x101d02 `PROGRAM:c`, `PROGRAM:s`; 0x101e3a `CreateDir("savegame")` | the PLAYER one is relative to the CLI's current dir, not `PLAYER:`. [추정] that is why the AROS run left PLAYER without `savegame/`. The builder puts it on PLAYER, where the table writes `PLAYER:savegame/*` (objlist + 69 OBJBLK) |
| boot block | hunk 3 + $160, written by trackdisk CMD_WRITE (0x145318), checksum 0x1452f4 | standard 1.x block: `DOS\0`, root 880, FindResident("dos.library"). PROGRAM only. All disks OFS (the installer writes `DOS\0`) |
| gypsya.new | entry 7: `INSTALL:gypsya.new`, not on disk 1 | the read pass ignores stat()'s result (0x102e26), reuses the previous size (gypsy.new, 72,880), AllocMem's it without MEMF_CLEAR (0x102e5c), read() on the failed open() reads nothing: the file is uninitialised memory. The builder writes 72,880 zero bytes. `u` names only `gypsy.new` |

Readback: `mkadf.py verify` compares every file with `manifest.json` through `amifs.py` and through
amitools (`uvx --from amitools xdftool <adf> unpack`): 126/126 equal, both readers. Against the
WU-O HD install, 122 shared names are byte-identical; `gypsya.new` and `PROGRAM:ultima6` (= ultcli,
by design) differ. Free blocks: PROGRAM 48, POPULACE 403, PLAYER 408, PORTRAIT 873. The build is
deterministic (fixed date 1991-11-21): `adf/` PROGRAM `a944beac…`, POPULACE `7dbbc468…`, PLAYER
`e0e1daad…`, PORTRAIT `9bfef693…`.

## B. PAL/NTSC under the AROS ROM

ultcli (`PROGRAM:ultima6`) is a state machine (jump table at 0x100452, state in d7: 0 = run
`PROGRAM:u`, 1 = check PAL then run `PORTRAIT:game`, 2 = run `PORTRAIT:end`, 3/4 = exit with a
requester). State 1 tests `btst #0,$cf(GfxBase)` (DisplayFlags NTSC) and on NTSC shows the "SORRY
... REQUIRES A PAL MACHINE" requester. In several boots AROS set ExecBase VBlankFrequency and
PowerSupplyFrequency (+$212/+$213) to 60 and DisplayFlags to `$0011` (NTSC|REALLY_PAL), while
the core runs PAL fields and VPOSR reads `$A000` (PAL Agnus ID $20); in later boots the same
fields read 50/50/`$0014`. With 60/`$0011` the game-first boot ended in an empty-bodied "System
Request" (ultcli's requester). [가설] AROS calibrates against something host-timed (the RTC
follows host time), which would also explain why runs differ. `lib.mjs palFix` writes what
Kickstart sets on PAL hardware (50, 50, `$0014`) at frame 700, before ultcli starts (~1800).
`play.mjs` logs the values it found.

## C. Where `u` stops (installer-faithful set, `run.sh intro`)

`u`'s main (0x1029d0) runs 0x1041b0 (ORIGIN, Lord British, intro_1 TV-room scene; skippable
through the per-frame input check 0x1039b8) and then the menu loop 0x1031b8. Observed on `adf/`:

- ORIGIN Presents at about frame 3587, "A Lord British Production" 3799-3862, then the TV room.
  One run (`shots/intro-try2`) shows it clean at frame 3912 and garbled at 3952; the final run
  shows only the garbled picture (frame 3925). It then stays unchanged (2,900 frames in the final
  run) while the music keeps playing. Keys and clicks change nothing.
- Breakpoint trace (u's hunk bases found by walking its seglist from the hunk-0 signature):
  load intro_1 (0x115132) → 0x113fce/0x113db2/0x113fb2 at frames 3806-3809, then 0x1039b8 once per
  frame for 12 frames, then no u breakpoint fires again and every sampled PC is outside u's
  hunks: u's task no longer runs. Its frame wait (0x115078) is a busy loop on a flag cleared by its VBlank server
  (0x1198f8), so a live u would show PC in u.
- One run caught an illegal instruction (vector 4) in supervisor mode at `$22e5f4`, after the CPU
  had executed data from `$22e5dc`; A6 = `$22e6a8`, the address `os tasks` listed as the input.device
  task in other boots of the same set. Other runs ended with the CPU filling memory with `0000 2608 …`.
- Not the cause: the floppy data (§A set, verified), the music (0x1036bc patched to `rts` at run
  time: no audio, same garbled frame), fast RAM (2 MB chip + 0 fast: crash; the A500-like
  configuration does not boot AROS at all, §3).
- [가설] Not established: an interrupt-level or input-handler call in u that goes through a
  register or structure AROS sets up differently from Kickstart 1.3 (A6 equal to a task pointer
  in the faulting run). Next step: a catchpoint run that keeps the instruction trace from the
  last u instruction before `$22e5dc`.

## D. Britannia (`run.sh play`, `adf-gamefirst/`)

`mkadf.py floppies OUT --game-first` = the §A set plus two patches, each checked against the
expected bytes: ultcli hunk 1 +8 `moveq #0,d7` → `moveq #1,d7` (file offset `0x3e4`; state 1 =
run `PORTRAIT:game`, u skipped), and objlist `0x1C71 = 1`, `0x1C72 = 1`. Why the objlist bytes:
with the stock objlist the game shows its parchment UI and "You must first create or transfer a
character." (main 0x1351d8 tests the byte at a4-0x7b03); the loader 0x105350 reads objlist
0x1BF1..0x1C72 to a4-0x7b84 (0x105578), so the byte is objlist 0x1C72 (avatar portrait), 0 in
DATA1. Values = Nuvie's new-game mapping for a male avatar with portrait 1 (formats.md §7), not
what `u` would write. Name stays "Avatar", stats are DATA1's.

Final run `shots/play-20261014-190647/` (lores 456×313 / hires 912×313, uncropped):

| shot | frame | lores sha256 | what is on screen |
|---|---|---|---|
| `ingame-first` | 7009 | `adb1a3a4…` | game UI: party panel Avatar/Dupre/Shamino/Iolo 90, date "7-04-0161 Wind:N", prompt "Avatar: >", command icons; map view still blank. Same bytes in 3 runs |
| `ingame` | 8313 | `187c90c1…` | throne room: red carpet up to the throne, the party and gargoyles on and beside it; animates from here on (NPCs, combat) |
| `step-1` | 9401 | `4a44de7c…` | DOWN/LEFT/RIGHT blocked (each a turn), UP scrolled the view one tile north; wall row visible at the top |
| `step-2` | 10104 | `9ea5ddd9…` | RIGHT scrolled east; "Avatar heavily wounded", Avatar 85 HP |
| `step-3` | 10505 | `238a488f…` | LEFT scrolled west; "Gargoyle critical!" |

Moves are confirmed by the scroll (`viewShift`: the map view compared after a 16-px shift beats
the unshifted comparison by more than 0.1; observed 0.92-0.97 equal pixels).
Where the party blocks the requested direction, play.mjs tries the other arrows; each press is a
game turn. Step outcomes, gargoyle moves and combat messages differ between runs.

## 1. Emulator, versions, sources

| part | version / id | source |
|---|---|---|
| core + UI | vAmigaWeb deployed build, commit `bd0d2e9519c8` (2026-09-10); RetroShell banner "RetroShell 4.3.6 (Sep 1 2026)" | `git clone --depth 1 https://github.com/vAmigaWeb/vAmigaWeb.github.io` (official site repo; GPL-3.0 source repo `vAmigaWeb/vAmigaWeb`) |
| `vAmiga.wasm` | sha256 `b5dec8b70fbb64a3…` | same clone |
| Kickstart | AROS "Kickstart replacement" Version 20260820: `roms/aros-rom-20260820.bin` sha256 `fe97599321ed9a66…`, `roms/aros-ext-20260820.bin` `5f078fa9be00f395…` | bundled in the clone; the UI fetches it itself (`js/vAmiga_ui.js:701-726`, `AROS:true` URL flag) |
| browser | Playwright 1.63.0 → chromium-headless-shell rev 1243 | npm (`tools/oracle/package-lock.json`); browser was already in `~/Library/Caches/ms-playwright` |

Machine as the core reports it (`/status`): Agnus rev 3 (ECS 2 MB), Denise rev 1 (ECS), 68000,
2048 KB chip + 2048 KB fast, 0 slow, PAL. Speed: 50 emulated frames per wall-second (250 frames in
5 s; realtime PAL). `warp on=1` did not speed it up (1003 frames in 20 s).

**FS-UAE 3.2.35** (`brew install fs-uae`; Homebrew also upgraded sdl3 3.4.16→3.4.18 and
sdl2-compat 2.32.72→2.32.74) was installed and read, but not used. It has the built-in AROS ROM
and `screenshots_output_dir`, but no scriptable key-input channel. Lua is not compiled in
(`strings` shows no lua symbols; `src/fs-uae/luascript.c` sits behind `WITH_LUA`, which configure
labels "unfinished"). Input recordings carry a per-frame RNG checksum (`src/fs-uae/recording.c`).
That leaves netplay as the only injection path. To remove it: `brew uninstall fs-uae`.

## 2. How the game launches (read from disk 1)

- `s/startup-sequence` = `installu6` (one line). Disk 1 is an installer disk, not a game disk.
- `start` = `ultima6` / `endcli`. `ultima6` (Workbench launcher) and `ultcli` (CLI launcher) both
  reference `PROGRAM:u`, `PORTRAIT:game`, `PORTRAIT:end`, `BOOK.DAT` (strings). Both carry the
  "REQUIRES A PAL MACHINE RUNNING AT 50HZ" text.
- `installu6` asks "(H)ard drive or (F)loppies". The floppy path builds four disks with the labels
  PROGRAM, POPULACE, PLAYER, PORTRAIT. The HD path asks for a directory (example `DH0:ULTIMA6`).
- The floppy path (measured, §3) writes `PROGRAM:s/startup-sequence` = `ultima6` / `endcli`
  (= disk 1's `start`), and `PROGRAM:ultima6` is the **CLI launcher**: byte-identical to the
  decrunched `ultcli`. The HD path installs the Workbench `ultima6` instead (9,212 bytes).
  Per-disk split: PROGRAM = launcher, `u`, intro/title/menu `.new`, `.MOD` music, `c/endcli`;
  POPULACE = `book.dat`, `converse.*`, `portrait.*`, `*.bmp`; PLAYER = map/tiles/`look.lzd`;
  PORTRAIT = `game`, `end*`, `tileflag`, `schedule`, `u6.ch`, `u6.set`, `basetile`, `anim*`, `paper.bmp`.

## 3. What boots (all under AROS)

| step | result | shot (lores sha256 / hires sha256) |
|---|---|---|
| disk1 in DF0 | the installer console window appears at about frame 2250 (45 s) | `shots/probe/t45.png` `9309e99d…`; `shots/install/01-prompt.hires.png` `6b6867d8…` |
| floppy install, blank OFS ADFs from `mkadf.py` | asks for and fills all four disks (PROGRAM, POPULACE, PLAYER, PORTRAIT), then keeps copying ("Copying PLAYER:maptiles.ega"). Not run to the end; disks not exported | `shots/install/07-disk4.hires.png` `86dd7c64…`, `09-later.hires.png` `8c467c5f…` |
| HD install (`h`, `DH0:ULTIMA6`, `y`) on a blank 16 MB FFS HDF | completes: "Uncompressing game", copies `savegame/*`, then the window closes (empty Workbench screen, texhash 34535989) | `shots/hd/02-path-prompt.hires.png` `204fe891…`, `04-after-copy` `99f12c12…`, `05-warp` `52cfb642…`, `06-done` `130e32ea…` |
| installed tree | 125 files, 2,461,018 bytes. 53 top-level files and all 70 `savegame/` files (OBJBLK* + objlist) are byte-identical to the PP20-unpacked set in `~/Games/ultima6/unpacked`. Extras: `gypsya.new`, `ULTIMA6.info`. Not installed: `installu6`, `ultcli`, `start`, `s/`, `c/` | manual run: `prev/u6-installed.hdf` `ca6c61b4…`; scripted run: `disks/u6-installed.hdf` `49fca0cd…` |
| **`gypsya.new` defect** | the installer writes it as 72,880 bytes that are all zero except one long at offset 4 (`$000925D0` in one run, `$00092668` in the other; looks like a memory address). `gypsy.new` (same size) has 70,751 nonzero bytes. Explained in §A: the source file does not exist and the installer writes an uninitialised buffer (not an AROS fault). Replacing it with `gypsy.new` does not change the crash below (`diag/gypsy`) | — |
| scripted `run.sh build` (exit 0) | the same prompts (installer prompt texhash 1482153914), install finished in 13,641 frames. The extracted tree is identical to the manual run's except bytes 4-7 of `gypsya.new` | `shots/build/01..03*.hires.png` |
| `ultima6` from a CLI startup-sequence | prints "Must be run from Workbench, with no other windows open" and returns (rc 0). The AROS ROM alone boots to a CLI, not a Workbench | `shots/game/diag-10.hires.png` `60121032…` |
| `ultcli` from `DH1:ULTIMA6` | "ultcli: object not found" (the HD install does not copy it) | `boot-tree/o2.txt` |
| `ultcli` with volumes PROGRAM/POPULACE/PLAYER/PORTRAIT on DH0-DH3 (`run.sh build` layout; `v4-*` shots from the manual-run volumes now in `prev/`, `boot-20261014-160059` from the scripted `disks/`) | grey AROS screen, then black from about frame 500 (texhash 2847054613), then a crash at about frame 1631 (§5). No picture is ever drawn; SPACE/RETURN/ESC change nothing | `shots/game/v4-10.png` = `v4-60.png` `cda9039b…`; `run.sh boot` default run: `shots/boot-20261014-160059/t10.png` = `t30.png` `cda9039b…`, `t60.png` `fb75b891…` (still black, shorter display window; [추정] from post-crash register writes) |
| floppy install, sources on HDFs (INSTALL/DATA1/DATA2 volume HDFs from `files/d*`, the four blanks in DF0, then moved to DF1-DF3 after each label) | completes. Copying takes 32,600 frames (about 11 min, real floppy speed), then the empty Workbench screen. Result has **defects**: `PLAYER:map`, `PORTRAIT:end`, `PORTRAIT:end.new` are 0 bytes; `POPULACE:converse.a` is 160,175 bytes and differs from the 162,615-byte original; PLAYER has no `savegame/`; `gypsya.new` is again near-zero (19 nonzero bytes). 48 other files are byte-identical to the HD install | `shots/floppy/01-prompt.hires.png` `6b6867d8…`, `02-program` `8e64fa44…` (same as the earlier floppy run), `04-copy-done` `130e32ea…`; ADFs `floppy/PROGRAM.adf` `1d496d94…`, `POPULACE.adf` `41e7d5f0…`, `PLAYER.adf` `a4e3b7c7…`, `PORTRAIT.adf` `6d3cf554…` |
| **boot of that floppy set** (DF0 PROGRAM, DF1 POPULACE, DF2 PLAYER, DF3 PORTRAIT) | AROS boot console (frame 1750); bitplane DMA on (0.52) from about frame 1828; heavy disk loading until about 3350; **"ORIGIN Presents"** at about frame 3606 (audio on); **first intro scene** at about 3859; from about 4114 a garbled composite that stays unchanged (texhash 3218773203 to frame 6159; music keeps going, blitter and disk idle). In a second run the picture was already garbled at about frame 3850, and SPACE/ESC/RETURN changed nothing up to frame 7158 | `diag/floppyboot2/f03500.png` `09bdf668…` (ORIGIN), `f03750.png` `3c21d33a…` (intro scene), `f04000.png` = `final.png` `97eb2956…` (garbled), `final.hires.png` `3bd4c185…`; keys: `shots/floppy-keys/boot-20261014-162550/*` |
| A500-like machine (OCS, 512 KB chip + 512 KB slow, no fast) with the same volumes | AROS stays on its grey boot screen for 2319 frames (texhash 3314420245, no disk DMA). The startup-sequence never runs | `diag/a500/final.png` `36075656…` |

All shots are under `~/Games/ultima6/oracle/`. Scripted volumes (`disks/`, from `run.sh build`):
PROGRAM `bc4f63c6…`, POPULACE `1d2e4e2c…`, PLAYER `85a4060a…`, PORTRAIT `f463326d…` (FFS
timestamps make these differ between builds; the extracted trees do not, except `gypsya.new`). The boot is reproducible screen by screen: the
installer prompt gave the same picture hash in independent runs (`9309e99d…` for probe/t45 and
install/01-prompt; texhash 1482153914 in three runs). The low-memory contents after the crash
differed between two runs, though. Host-side timing (when a command lands) leaks into emulated
state, so runs are not bit-identical end to end.

## 4. Harness and how to drive it

    tools/oracle/run.sh setup      # clone vAmigaWeb at the pinned commit (warns on drift) + npm ci
    tools/oracle/run.sh floppies   # adf/ + adf-gamefirst/ from installu6's table, amitools readback (§A)
    tools/oracle/run.sh play [KEY ...]    # adf-gamefirst/ -> Britannia shots in shots/play-<ts>/ (§D)
    tools/oracle/run.sh intro      # adf/ -> ORIGIN/intro shots until the picture stops, shots/intro-<ts>/ (§C)
    node tools/oracle/pace.mjs OUT [HOLD ...]   # adf-gamefirst/: held-button walk, step frames per hold (ui/target.rs HELD_WALK_MS)
    tools/oracle/run.sh build      # ADFs -> installer under AROS -> disks/{PROGRAM,POPULACE,PLAYER,PORTRAIT}.hdf
    tools/oracle/run.sh boot [step ...]   # boot those, run steps, PNGs into shots/boot-<ts>/
    tools/oracle/run.sh serve &    # control server 127.0.0.1:8765, then:
    tools/oracle/ctl.sh insert drive=0 path=/abs/x.hdf    # .adf -> DFn, .hdf -> DHn (power-cycles)
    tools/oracle/ctl.sh type $'text=DH0:ULTIMA6\n'          # \n = RETURN
    tools/oracle/ctl.sh key k=RETURN      # also: stable frames=150, until hash=N, status, quit
    tools/oracle/ctl.sh shot name=x modes=lores,hires,tex,page
    tools/oracle/ctl.sh palfix            # §B; also: configure spec=FAST_RAM=0 (powers off)

- **Input** = raw Amiga keycodes through `wasm_key(code, pressed)` (`main.cpp:1002`), so host
  keymaps play no part. Names are in `lib.mjs` `KEY` (US layout, shifted characters held with
  LSHIFT). `tap` holds a key for 4 frames, releases it, then waits 4 more. Mouse:
  `/mouse?dx&dy`, `/click?button=1|2|3`. Waits are counted in emulated frames
  (`wait`, `stable` = picture unchanged and disk DMA 0 for N frames, `until` = an exact texhash).
  The AROS console is line-buffered, so prompts need RETURN.
- **Capture** reads the core's RGBA texture from wasm memory (`wasm_pixel_buffer`, 1824×313 = 912
  hires positions × 2 texels, one PAL field), not the scaled canvas. `lores` = every 4th texel →
  456×313, `hires` = every 2nd → 912×313, `tex` = raw. Images are uncropped (border and blanking
  included). A shot is the last completed frame at poll time, not a specific frame number.
- **Debug**: `/shell?cmd=` drives vAmiga RetroShell (`debugger`, `catch vector N`, `watch at A`,
  `d`, `m`, `? blitter`). Commands sent while the CPU is paused have to go in as keystrokes
  (`lib.mjs shellKeys`, which uses `wasm_retro_shell_press_key`, `main.cpp:3140`). Scripted
  dumps: `node tools/oracle/diag.mjs OUTDIR FRAMES [cmd...]` with `U6_PRE`/`U6_PRE_AT`/`U6_MACHINE`.
  Memory: `lib.mjs peekL/peekW/peekStr` (`wasm_peek16`, no side effects) and `poke`
  (`wasm_mem_patch`). `os processes`/`os tasks`/`os libraries` work on AROS until something
  corrupts ExecBase's checksummed area ("Invalid ChkSum"). Vector 8 is AROS's own
  `Supervisor()` trap (`ori.w #$2000,SR` at `$F82EE4`): leave it out of catchpoint sets.
- **Play**: `play.mjs` finds the game UI by colours in the party panel (≥12 there; the AROS
  console and the empty parchment give ≤5) and the settled view by a map-view redraw plus 250
  frames of unchanged party panel and idle disk. The game reads raw keys through its
  input.device handler; its table (a4-0x3572) maps UP/DOWN/RIGHT/LEFT to 0x148/0x150/0x14d/0x14b.
- Without a Workbench, the volume names stand in for assigns. AROS's boot shell has no `assign`
  (`assign: object not found`), so each HDF is named PROGRAM/POPULACE/PLAYER/PORTRAIT and holds
  the full installed tree plus `ultcli` (PP20-decrunched by `tools/pp20.py`). PROGRAM's
  `s/startup-sequence` is `ultcli`. With DF0 and DH0 both bootable, DF0 boots first.

## 5. Blocking points

**Floppy set (the game's own layout):** the intro stops after its first scene with a garbled
picture and does not react to SPACE/ESC/RETURN (§3). WU-O's candidate (a), a defective floppy
set, is ruled out: the §A set, verified by two readers, stops the same way. What is known about
the stop is in §C; the cause is still open.

**HD-install layout (`run.sh build` volume HDFs, `ultcli`):** crash before any picture. The
timeline below comes from `diag.mjs`, a `$0-$7F` sampler, and RetroShell catch/watch points on
the default machine:

1. Frames 0-112: AROS boots. ExecBase (`$4`) settles at `$2007D8`.
2. From about frame 500: black screen. Bitplane DMA is 0 at every sample from frame 250 on
   (DMACON shows `.CBSDA`, i.e. bitplane DMA off). Copper activity is about 0.005.
3. About frame 1400: audio DMA turns on (0.29) [추정: the music starts].
4. Frame 1631: `$0`, `$4`, `$C`, `$10`, `$2C` all change in one sample (ExecBase pointer and
   exception vectors overwritten). The instruction trace at that moment:
   user mode `2f1b3a btst #$6,($2,A2)` / `2f1b40 bne $2f1b3a` (a blitter-busy wait on DMACONR,
   [추정] A2 = `$DFF000`), then **supervisor, IPL 6, PC `$000000`**. [추정] A level-6
   interrupt vectored through a clobbered autovector `$78`. `$78` itself was not sampled at the
   crash; a read watchpoint on it showed `$FE8F74` (AROS's level-6 handler) at about frame 1335.
5. After that the CPU runs garbage: illegal opcodes at `$0` loop through vector 4 = 0, it walks
   through unmapped `$A00xxx`, and memory gets filled with `0000 0000 2600` from about frame
   1799. [추정] The "Last exception: 11" seen in `? cpu` at frame 2069 comes from this phase. With no console filter, the
   wild CPU's per-access "Invalid addr e9xxxx" warnings (131,060 of them) once brought the page
   down.

Not established: which binary owns `$2F1B3A`. The code there had been overwritten before it
could be dumped. Also not established: what wrote low memory.
[가설] A blit or copy got a NULL or near-zero destination from an OS structure that AROS lays out
differently from Kickstart (BitMap `Planes[]`, View/ViewPort/RasInfo, GfxBase private fields).
That would fit "black screen, then clobbered low memory while waiting on the blitter".
Unverified.

AROS's own boot raises vectors 4 (MOVEC probe `$F802B0`), 8 (`ori.w #$2000,SR` at `$F82EE4`) and
11 (FPU probe `$F80294`). Catchpoints have to be armed after about frame 250
(`U6_PRE_AT=250`), or they fire inside the ROM.

## 6. Limits

- A real Kickstart (1.3 / 2.04) would answer whether AROS is the only blocker. None is available,
  and none may be fetched from unofficial sources (WU-O constraint).
- Realtime only (50 fps). Warp has no effect headless. Audio is not captured.
- `texhash` is FNV-1a over every 3rd texel. It is for equality checks, not a cryptographic hash.
- The 1 MB A500 configuration does not get past AROS boot in this setup.
- The Britannia route (§D) skips `u`, so the oracle has no intro, menu or character-creation
  screens, and its avatar is DATA1's default party with two objlist bytes set, not a created
  character.
- Runs are not repeatable frame for frame: the AROS boot differs between runs (§B), and the
  game's combat and NPC moves follow. The first in-game frame (`ingame-first`) was byte-identical
  in three runs; later frames are not.
- `floppy/` holds WU-O's AROS-installer floppies (defective, §3); use `adf/` (§A) instead.
