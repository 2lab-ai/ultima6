#!/usr/bin/env python3
"""Write an empty, formatted (non-bootable) 880 KB Amiga DD floppy image.

    python3 -I mkadf.py OUT.adf [LABEL] [--ffs]

Layout = what AmigaDOS Format leaves on a DD disk: boot block "DOS\\0" (or "DOS\\1" for FFS)
with root pointer 880, root block at 880, one bitmap block at 881, all other blocks free.
"""
import struct
import sys
import time

BLOCKS, BSIZE, ROOT, BITMAP = 1760, 512, 880, 881


def checksum(block: bytearray, at: int) -> None:
    struct.pack_into('>I', block, at, 0)
    total = sum(struct.unpack('>128I', bytes(block))) & 0xFFFFFFFF
    struct.pack_into('>I', block, at, (-total) & 0xFFFFFFFF)


def amiga_date(t: float) -> tuple[int, int, int]:
    secs = int(t) - 252460800  # 1978-01-01
    days, rem = divmod(secs, 86400)
    return days, rem // 60, (rem % 60) * 50


def make(label: str, ffs: bool) -> bytes:
    img = bytearray(BLOCKS * BSIZE)
    img[0:4] = b'DOS' + bytes([1 if ffs else 0])
    struct.pack_into('>I', img, 8, ROOT)

    root = bytearray(BSIZE)
    struct.pack_into('>I', root, 0, 2)          # T_HEADER
    struct.pack_into('>I', root, 12, 72)        # hash table size
    struct.pack_into('>i', root, 312, -1)       # bm_flag: bitmap valid
    struct.pack_into('>I', root, 316, BITMAP)   # bm_pages[0]
    d = amiga_date(time.time())
    struct.pack_into('>3I', root, 420, *d)      # root alteration
    name = label.encode('latin-1')[:30]
    root[432] = len(name)
    root[433:433 + len(name)] = name
    struct.pack_into('>3I', root, 472, *d)      # disk alteration
    struct.pack_into('>3I', root, 484, *d)      # creation
    struct.pack_into('>I', root, 508, 1)        # ST_ROOT
    checksum(root, 20)
    img[ROOT * BSIZE:(ROOT + 1) * BSIZE] = root

    bm = bytearray(BSIZE)
    words = [0] * 127
    for blk in range(2, BLOCKS):
        if blk not in (ROOT, BITMAP):
            i = blk - 2
            words[i // 32] |= 1 << (i % 32)
    struct.pack_into('>127I', bm, 4, *words)
    checksum(bm, 0)
    img[BITMAP * BSIZE:(BITMAP + 1) * BSIZE] = bm
    return bytes(img)


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith('--')]
    if not args:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    out, label = args[0], (args[1] if len(args) > 1 else 'Empty')
    with open(out, 'wb') as f:
        f.write(make(label, '--ffs' in argv))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
