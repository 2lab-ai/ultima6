---
title: ultima6 oracle — original Amiga Ultima VI under AROS (vAmigaWeb, headless)
type: note
tags: [ultima6, oracle, amiga, aros, vamiga, emulator]
---

# Oracle: original Amiga Ultima VI in an emulator

Measured 2026-10-14 on macOS 26.6.2 arm64. Harness = `tools/oracle/`. Everything that touches game
data stays in `~/Games/ultima6/oracle/` (disks, extracted tree, shots, logs).

**Result in one line:** under the AROS ROM, the original disk 1 boots and the original installer
runs to completion (HD path), but the game itself crashes about 32 s into `ultcli`, before it
draws its first picture. No title screen or Britannia view can be captured with AROS. That is the
blocking point (details in §5).

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

## 3. What boots (all under AROS)

| step | result | shot (lores sha256 / hires sha256) |
|---|---|---|
| disk1 in DF0 | the installer console window appears at about frame 2250 (45 s) | `shots/probe/t45.png` `9309e99d…`; `shots/install/01-prompt.hires.png` `6b6867d8…` |
| floppy install, blank OFS ADFs from `mkadf.py` | asks for and fills all four disks (PROGRAM, POPULACE, PLAYER, PORTRAIT), then keeps copying ("Copying PLAYER:maptiles.ega"). Not run to the end; disks not exported | `shots/install/07-disk4.hires.png` `86dd7c64…`, `09-later.hires.png` `8c467c5f…` |
| HD install (`h`, `DH0:ULTIMA6`, `y`) on a blank 16 MB FFS HDF | completes: "Uncompressing game", copies `savegame/*`, then the window closes (empty Workbench screen, texhash 34535989) | `shots/hd/02-path-prompt.hires.png` `204fe891…`, `04-after-copy` `99f12c12…`, `05-warp` `52cfb642…`, `06-done` `130e32ea…` |
| installed tree | 125 files, 2,461,018 bytes. 53 top-level files and all 70 `savegame/` files (OBJBLK* + objlist) are byte-identical to the PP20-unpacked set in `~/Games/ultima6/unpacked`. Extras: `gypsya.new`, `ULTIMA6.info`. Not installed: `installu6`, `ultcli`, `start`, `s/`, `c/` | `disks/u6-installed.hdf` (manual run) `ca6c61b4…` |
| **`gypsya.new` defect** | the installer writes it as 72,880 bytes that are all zero except one long at offset 4 (`$000925D0` in one run, `$00092668` in the other; looks like a memory address). `gypsy.new` (same size) has 70,751 nonzero bytes. [추정] it should be a copy of `gypsy.new`, and AROS is what breaks it. Replacing it with `gypsy.new` does not change the crash below (`diag/gypsy`) | — |
| scripted `run.sh build` (exit 0) | the same prompts (installer prompt texhash 1482153914), install finished in 13,641 frames. The extracted tree is identical to the manual run's except bytes 4-7 of `gypsya.new` | `shots/build/01..03*.hires.png` |
| `ultima6` from a CLI startup-sequence | prints "Must be run from Workbench, with no other windows open" and returns (rc 0). The AROS ROM alone boots to a CLI, not a Workbench | `shots/game/diag-10.hires.png` `60121032…` |
| `ultcli` from `DH1:ULTIMA6` | "ultcli: object not found" (the HD install does not copy it) | `boot-tree/o2.txt` |
| `ultcli` with volumes PROGRAM/POPULACE/PLAYER/PORTRAIT on DH0-DH3 (`run.sh build` layout) | grey AROS screen, then black from about frame 500 (texhash 2847054613), then a crash at about frame 1631 (§5). No picture is ever drawn; SPACE/RETURN/ESC change nothing | `shots/game/v4-10.png` = `v4-60.png` `cda9039b…`; `run.sh boot` default run: `shots/boot-20261014-160059/t10.png` = `t30.png` `cda9039b…`, `t60.png` `fb75b891…` (still black, shorter display window; [추정] from post-crash register writes) |
| A500-like machine (OCS, 512 KB chip + 512 KB slow, no fast) with the same volumes | AROS stays on its grey boot screen for 2319 frames (texhash 3314420245, no disk DMA). The startup-sequence never runs | `diag/a500/final.png` `36075656…` |

All shots are under `~/Games/ultima6/oracle/`. The boot is reproducible screen by screen: the
installer prompt gave the same picture hash in independent runs (`9309e99d…` for probe/t45 and
install/01-prompt; texhash 1482153914 in three runs). The low-memory contents after the crash
differed between two runs, though. Host-side timing (when a command lands) leaks into emulated
state, so runs are not bit-identical end to end.

## 4. Harness and how to drive it

    tools/oracle/run.sh setup      # clone vAmigaWeb at the pinned commit (warns on drift) + npm ci
    tools/oracle/run.sh build      # ADFs -> installer under AROS -> disks/{PROGRAM,POPULACE,PLAYER,PORTRAIT}.hdf
    tools/oracle/run.sh boot [step ...]   # boot those, run steps, PNGs into shots/boot-<ts>/
    tools/oracle/run.sh serve &    # control server 127.0.0.1:8765, then:
    tools/oracle/ctl.sh insert drive=0 path=/abs/x.hdf    # .adf -> DFn, .hdf -> DHn (power-cycles)
    tools/oracle/ctl.sh key k=RETURN | type 'text=DH0:ULTIMA6
    ' | stable frames=150 | until hash=N | shot name=x modes=lores,hires,tex,page | status | quit

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
- Without a Workbench, the volume names stand in for assigns. AROS's boot shell has no `assign`
  (`assign: object not found`), so each HDF is named PROGRAM/POPULACE/PLAYER/PORTRAIT and holds
  the full installed tree plus `ultcli` (PP20-decrunched by `tools/pp20.py`). PROGRAM's
  `s/startup-sequence` is `ultcli`. With DF0 and DH0 both bootable, DF0 boots first.

## 5. The blocking point (ultcli under AROS)

The timeline below comes from `diag.mjs`, a `$0-$7F` sampler, and RetroShell catch/watch points
on the default machine:

1. Frames 0-112: AROS boots. ExecBase (`$4`) settles at `$2007D8`.
2. From about frame 500: black screen. Bitplane DMA is 0 at every sample from frame 250 on
   (DMACON shows `.CBSDA`, i.e. bitplane DMA off). Copper activity is about 0.005.
3. About frame 1400: audio DMA turns on (0.29) [추정: the music starts].
4. Frame 1631: `$0`, `$4`, `$C`, `$10`, `$2C` all change in one sample (ExecBase pointer and
   exception vectors overwritten). The instruction trace at that moment:
   user mode `2f1b3a btst #$6,($2,A2)` / `2f1b40 bne $2f1b3a` (a blitter-busy wait on DMACONR,
   [추정] A2 = `$DFF000`), then **supervisor, IPL 6, PC `$000000`**. A level-6 interrupt vectored
   through a clobbered `$78`.
5. After that the CPU runs garbage: illegal opcodes at `$0` loop through vector 4 = 0, it walks
   through unmapped `$A00xxx`, and memory gets filled with `0000 0000 2600` from about frame
   1799. "Last exception: 11" in `? cpu` comes from this phase. With no console filter, the
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
