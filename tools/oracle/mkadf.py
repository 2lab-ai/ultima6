#!/usr/bin/env python3
"""Amiga DD floppy images for the oracle.

    python3 -I mkadf.py OUT.adf [LABEL] [--ffs]      # empty, formatted, non-bootable volume
    python3 -I mkadf.py floppies OUTDIR [GAMES]       # the four game disks, as installu6 lays them out
    python3 -I mkadf.py floppies OUTDIR [GAMES] --game-first   # same + the two oracle patches below
    python3 -I mkadf.py verify OUTDIR                 # ADFs vs manifest.json: amifs reader, and
                                                      # OUTDIR/xdf/<VOL>/ if amitools unpacked there

`floppies` writes OUTDIR/{PROGRAM,POPULACE,PLAYER,PORTRAIT}.adf + OUTDIR/manifest.json from
GAMES (default $U6_GAMES or ~/Games/ultima6): files/d{1,2,3}/<INSTALL|DATA1|DATA2>/ = the three
original disks as xdftool unpacked them. Nothing is guessed: the file list, target volume,
target name and decrunch flag of every file come from installu6's own copy table (read out of
the hunk file at run time), the PROGRAM boot block is the one installu6 writes with
trackdisk.device, and PP20 files are decrunched with tools/pp20.py where installu6 calls its
decruncher. Evidence (installu6 flattened by tools/hunk2raw.py, base 0x100000):

  0x102ee6, 0x102f00   the table is at a4+$24 with 20-byte entries (the lea there, the entry
                       size the moveq there loads for the multiply 0x1016d8)
  entry:  +0 src name  +4 dst volume (index into a4+0)  +5 flags  +6 dst name
          +$a buffer  +$e size  +$12 state  +$13 src disk (index into a4+$14)
  0x103000..0x103026   written iff flags < 2, or (floppy and flags & 2), or (HD and flags & 4)
  0x102f1e/0x102f48    flags & 1 -> decrunch (0x146748) before the write
  0x102f8a..0x102fe2   floppy target = "<VOLUME>:" + dst name (HD: "<dir>/" + dst name)
  0x101cde/0x101d02    PROGRAM:c and PROGRAM:s created;  0x101e3a  "savegame" created on PLAYER
  hunk 3 +$160         1024-byte boot block (DOS\\0, root 880, FindResident("dos.library"))
  0x1452f4             its checksum (add with carry, invert), written by CMD_WRITE at 0x145318

No game byte is kept here: the tool checks installu6 by its sha256 and the four code regions
above (0x102ee6, 0x102f00, 0x103000, the boot block's DOS type at 0x145368) by one sha256 over
their bytes, so a different build is refused before its table is read.

One table entry has no source on disk 1. installu6 ignores stat()'s result (0x102e26), so its
size is the previous entry's; it AllocMem's that much without MEMF_CLEAR (0x102e5c), read() on the
failed open() reads nothing, and the buffer is written out under the entry's name. Its content is
whatever was in that memory; the tool writes zeros of that size. No program reads it.

`--game-first` is NOT what the installer writes. It exists because `u` (intro, menu, character
creation) stops in its intro under the AROS ROM (re/notes/oracle.md), so no character can be
created there. Two patches, both checked first (the launcher's sha256; for the objlist, that
the two bytes do not mark a character yet):
  PROGRAM:ultima6 (= ultcli)  hunk 1 +8: its first state becomes 1 (the tool writes the word of
      `moveq #1,d7` there). ultcli's state machine (0x100440 jump table: 0 = run u, 1 = run game,
      2 = run end) then starts at state 1, so it launches PORTRAIT:game with its normal
      arguments and skips u.
  PLAYER:savegame/objlist  0x1C71 = 1, 0x1C72 = 1 (gender, hero portrait). game's main()
      (0x1351d8) prints its no-character line (0x1350a6) when the byte at a4-0x7b03 is 0;
      0x105578 reads objlist 0x1BF1..0x1C72 to a4-0x7b84, so that byte is objlist 0x1C72.
      Values = Nuvie's new-game mapping for a male hero with the first portrait
      (re/notes/formats.md section 7).
"""
import hashlib
import importlib.util
import json
import os
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


amifs = _load('amifs', os.path.join(HERE, 'amifs.py'))

INSTALLU6_SHA256 = '580ff939de83bea61a0bfc9a6efb2503d2f9e0156d3653ffd39befff78ea8077'
BASE = 0x100000
VOLUMES = ('PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT')  # installu6 prompt order
DIRS = {'PROGRAM': ('c', 's'), 'PLAYER': ('savegame',)}
# installu6 code regions the table layout is read against (address, length), and the sha256 of
# their bytes in that order: the check needs no byte of installu6 in the repo.
CODE_REGIONS = ((0x102ee6, 4), (0x102f00, 2), (0x103000, 6), (0x145368, 4))
CODE_SHA256 = 'b1155489f21264cef818ff1b3d9d68004391f3c4987584c6f28da6db59047682'

def make(label: str, ffs: bool) -> bytes:
    """Empty volume as AmigaDOS Format leaves it: root 880, bitmap 881, everything else free."""
    return amifs.Writer(label, ffs=ffs, when=int(time.time())).image()


def install_table(installu6: bytes):
    """Decode installu6's copy table. Returns (entries, boot block bytes)."""
    if hashlib.sha256(installu6).hexdigest() != INSTALLU6_SHA256:
        raise ValueError('installu6 is not the known build (sha256 mismatch)')
    hunk2raw = _load('hunk2raw', os.path.join(HERE, '..', 'hunk2raw.py'))
    img, info = hunk2raw.convert(installu6, BASE)
    regions = b''.join(img[a - BASE:a - BASE + n] for a, n in CODE_REGIONS)
    if hashlib.sha256(regions).hexdigest() != CODE_SHA256:
        raise ValueError('installu6 code regions ' + ', '.join(f'{a:#x}+{n}' for a, n in CODE_REGIONS)
                         + ' are not the known build (sha256 mismatch)')
    seg = {s['index']: s for s in info['segments']}
    a4 = int(seg[4]['base'], 16)   # startup: lea <hunk 4 + 0>,a4
    boot_at = int(seg[3]['base'], 16) + 0x160

    def cstr(addr):
        o = addr - BASE
        return img[o:img.index(b'\0', o)].decode('latin-1')

    vols = [cstr(struct.unpack_from('>I', img, a4 - BASE + 4 * i)[0]) for i in range(5)]
    srcs = [cstr(struct.unpack_from('>I', img, a4 + 0x14 - BASE + 4 * i)[0]) for i in range(4)]
    entries, at = [], a4 + 0x24
    while True:
        src, dvol, flags, dst, _buf, _size, _state, sdisk = struct.unpack_from('>IBBIIIBB', img, at - BASE)
        if dvol == 0:  # loop end test: tst.b 4(a3,d0)
            break
        entries.append({'src': cstr(src), 'src_disk': sdisk, 'src_volume': srcs[sdisk],
                        'volume': vols[dvol], 'dst': cstr(dst), 'flags': flags,
                        'floppy': flags < 2 or bool(flags & 2), 'hd': flags < 2 or bool(flags & 4),
                        'decrunch': bool(flags & 1)})
        at += 20
    boot = img[boot_at - BASE:boot_at - BASE + 1024]
    if boot[:4] != b'DOS\0' or struct.unpack_from('>I', boot, 8)[0] != 880:
        raise ValueError('installu6 boot block not where expected')
    return entries, boot


# ultcli (the floppy's PROGRAM:ultima6) as decrunched: its sha256, and where the first state
# word sits (hunk 1 offset, length). The patch is the tool's own instruction, `moveq #1,d7`
# (0111 rrr0 dddddddd with register 7 and data 1).
ULTCLI_SHA256 = 'dfd96f65b66fdccd9f98ea939f522e04fce1e92d83186aa4d685be0781125a1b'
ULTCLI_STATE = (8, 2)
ULTCLI_PATCH = (0x7000 | 7 << 9 | 1).to_bytes(2, 'big')
OBJLIST_CREATED = {0x1C71: 1, 0x1C72: 1}


def game_first_patches(path: str, data: bytes) -> tuple[bytes, str] | None:
    """The --game-first changes for one floppy file, or None if the file is untouched."""
    if path == 'ultima6':
        hunk2raw = _load('hunk2raw', os.path.join(HERE, '..', 'hunk2raw.py'))
        if hashlib.sha256(data).hexdigest() != ULTCLI_SHA256:
            raise ValueError('ultcli is not the known build (sha256 mismatch)')
        h1 = next(s for s in hunk2raw.convert(data, BASE)[1]['segments'] if s['index'] == 1)
        at = h1['file_offset'] + 8 + ULTCLI_STATE[0]  # skip the HUNK_CODE type + size longwords
        out = bytearray(data)
        out[at:at + ULTCLI_STATE[1]] = ULTCLI_PATCH
        return bytes(out), f'ultcli first state 0 -> 1 (file offset {at:#x}: moveq #1,d7)'
    if path == 'savegame/objlist':
        out = bytearray(data)
        if any(out[o] for o in OBJLIST_CREATED):
            raise ValueError('objlist already marks a created character')
        for o, v in OBJLIST_CREATED.items():
            out[o] = v
        return bytes(out), 'character-created bytes ' + ', '.join(f'{o:#x}={v}' for o, v in OBJLIST_CREATED.items())
    return None


def floppies(outdir: str, games: str, game_first: bool = False) -> dict:
    pp20 = _load('pp20', os.path.join(HERE, '..', 'pp20.py'))
    with open(os.path.join(games, 'files', 'd1', 'INSTALL', 'installu6'), 'rb') as f:
        entries, boot = install_table(f.read())
    writers = {v: amifs.Writer(v) for v in VOLUMES}
    writers['PROGRAM'].set_boot(boot[12:])
    for vol, dirs in DIRS.items():
        for d in dirs:
            writers[vol].mkdir(d)
    manifest = {'installu6_sha256': INSTALLU6_SHA256, 'game_first': game_first, 'volumes': {v: [] for v in VOLUMES}}
    last_size = 0
    for e in entries:  # the read pass stats every entry, floppy or not, in table order
        src = os.path.join(games, 'files', f'd{e["src_disk"]}', e['src_volume'], e['src'])
        note = None
        if os.path.exists(src):
            with open(src, 'rb') as f:
                raw = f.read()
            last_size = len(raw)  # what stat() leaves in the shared stat buffer
            if e['decrunch']:
                if raw[:4] != b'PP20':
                    raise ValueError(f'{src}: decrunch flag set but not PP20')
                data = pp20.decrunch(raw)
            else:
                data = raw
        else:
            data = bytes(last_size)
            note = 'no source on the original disk: installu6 writes an uninitialised buffer of the previous file\'s size'
        if not e['floppy']:
            continue
        patch = game_first_patches(e['dst'], data) if game_first else None
        if patch:
            data, what = patch
            note = f'--game-first: {what}'
        writers[e['volume']].add(e['dst'], data)
        row = {'path': e['dst'], 'size': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
               'from': f'{e["src_volume"]}:{e["src"]}', 'decrunched': e['decrunch']}
        if note:
            row['note'] = note
        manifest['volumes'][e['volume']].append(row)
    os.makedirs(outdir, exist_ok=True)
    for v, w in writers.items():
        img = w.image()
        path = os.path.join(outdir, f'{v}.adf')
        with open(path, 'wb') as f:
            f.write(img)
        manifest[v] = {'adf': path, 'sha256': hashlib.sha256(img).hexdigest(), 'free_blocks': w.free_blocks(),
                       'bootable': v == 'PROGRAM'}
    with open(os.path.join(outdir, 'manifest.json'), 'w') as f:
        json.dump(manifest, f, indent=1)
    return manifest


def verify(outdir: str) -> list[str]:
    """Compare every ADF with manifest.json (amifs reader; amitools tree under xdf/ if present)."""
    with open(os.path.join(outdir, 'manifest.json')) as f:
        m = json.load(f)
    problems = []
    for v in VOLUMES:
        want = {r['path']: r['sha256'] for r in m['volumes'][v]}
        with open(os.path.join(outdir, f'{v}.adf'), 'rb') as f:
            vol = amifs.Volume(f.read())
        if vol.name != v:
            problems.append(f'{v}: volume name {vol.name!r}')
        got = {p: hashlib.sha256(vol.file_bytes(b)).hexdigest() for p, b, size in vol.walk() if size is not None}
        trees = {'amifs': got}
        xdf = os.path.join(outdir, 'xdf', v)
        if os.path.isdir(xdf):
            t = {}
            for dp, _, fns in os.walk(xdf):
                for fn in fns:
                    p = os.path.join(dp, fn)
                    with open(p, 'rb') as f:
                        t[os.path.relpath(p, xdf)] = hashlib.sha256(f.read()).hexdigest()
            trees['amitools'] = t
        for reader, tree in trees.items():
            for path in sorted(set(want) | set(tree)):
                if want.get(path) != tree.get(path):
                    problems.append(f'{v}:{path}: {reader} {"missing" if path not in tree else "differs"}')
        print(f'{v:9s} {len(want):3d} files  readers: {", ".join(trees)}  '
              f'{"ok" if not any(p.startswith(v + ":") for p in problems) else "MISMATCH"}')
    return problems


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith('--')]
    if not args:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    if args[0] == 'floppies':
        if len(args) < 2:
            print(__doc__.strip(), file=sys.stderr)
            return 2
        games = args[2] if len(args) > 2 else os.environ.get('U6_GAMES', os.path.expanduser('~/Games/ultima6'))
        m = floppies(args[1], games, game_first='--game-first' in argv)
        for v in VOLUMES:
            files = m['volumes'][v]
            print(f'{v:9s} {len(files):3d} files {sum(r["size"] for r in files):8d} bytes '
                  f'free {m[v]["free_blocks"]:4d} blocks  {m[v]["adf"]}  {m[v]["sha256"][:16]}')
        return 0
    if args[0] == 'verify' and len(args) == 2:
        problems = verify(args[1])
        for p in problems:
            print(p, file=sys.stderr)
        return 1 if problems else 0
    out, label = args[0], (args[1] if len(args) > 1 else 'Empty')
    with open(out, 'wb') as f:
        f.write(make(label, '--ffs' in argv))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
