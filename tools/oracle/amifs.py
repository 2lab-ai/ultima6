#!/usr/bin/env python3
"""AmigaDOS OFS/FFS reader and writer for ADF floppies and RDB-less HDF partitions.

    python3 -I amifs.py IMAGE ls             # tree with sizes
    python3 -I amifs.py IMAGE x OUTDIR       # extract every file

Block size 512. Root = (2 + blocks - 1) // 2 (880 on a DD floppy). OFS data blocks carry a
24-byte header; FFS data blocks are raw. Files are listed in name order.

`Writer` builds a volume from scratch (root, user dirs, file headers, T_LIST extension blocks,
OFS/FFS data blocks, one bitmap block, checksums). Block numbers are handed out upward from
root + 2 and wrap to 2; the result is deterministic for a given input and date.
"""
import os
import struct
import sys

BS = 512
T_HEADER, T_LIST, T_DATA = 2, 16, 8
ST_ROOT, ST_USERDIR, ST_FILE = 1, 2, -3
HT = BS // 4 - 56  # 72 hash slots
AMIGA_EPOCH = 252460800  # 1978-01-01 00:00:00 UTC in Unix seconds


def name_hash(name: str) -> int:
    """Directory hash slot of `name` (non-international mode: only ASCII a-z fold to upper)."""
    h = len(name)
    for c in name:
        o = ord(c) - 32 if 'a' <= c <= 'z' else ord(c)
        h = (h * 13 + o) & 0x7FF
    return h % HT


def checksum(block: bytearray, at: int) -> None:
    """Store the longword that makes the block's 128 longwords sum to 0 (mod 2^32)."""
    struct.pack_into('>I', block, at, 0)
    total = sum(struct.unpack('>128I', bytes(block))) & 0xFFFFFFFF
    struct.pack_into('>I', block, at, (-total) & 0xFFFFFFFF)


def boot_checksum(boot: bytearray) -> None:
    """Boot block checksum (longword 1): add with end-around carry over 256 longs, then invert."""
    struct.pack_into('>I', boot, 4, 0)
    total = 0
    for (v,) in struct.iter_unpack('>I', bytes(boot[:1024])):
        total += v
        if total > 0xFFFFFFFF:
            total = (total & 0xFFFFFFFF) + 1
    struct.pack_into('>I', boot, 4, (~total) & 0xFFFFFFFF)


def amiga_date(unix_seconds: int) -> tuple[int, int, int]:
    days, rem = divmod(int(unix_seconds) - AMIGA_EPOCH, 86400)
    return days, rem // 60, (rem % 60) * 50


class Volume:
    def __init__(self, data: bytes):
        self.d = data
        self.blocks = len(data) // BS
        if data[:3] != b'DOS':
            raise ValueError('no DOS boot block')
        self.ffs = data[3] & 1 == 1
        self.root = (2 + self.blocks - 1) // 2
        if self.long(self.root, 0) != T_HEADER or self.slong(self.root, BS - 4) != ST_ROOT:
            raise ValueError(f'no root block at {self.root}')
        self.name = self.bstr(self.root, BS - 80)

    def blk(self, n):
        return self.d[n * BS:(n + 1) * BS]

    def long(self, n, off):
        return struct.unpack_from('>I', self.d, n * BS + off)[0]

    def slong(self, n, off):
        return struct.unpack_from('>i', self.d, n * BS + off)[0]

    def bstr(self, n, off):
        ln = self.d[n * BS + off]
        return self.d[n * BS + off + 1:n * BS + off + 1 + ln].decode('latin-1')

    def entries(self, dirblk):
        for i in range(HT):
            b = self.long(dirblk, 24 + 4 * i)
            while b:
                yield b
                b = self.long(b, BS - 16)  # hash chain

    def file_bytes(self, hdr):
        size = self.long(hdr, BS - 188)
        ptrs, blk = [], hdr
        while blk:
            count = self.long(blk, 8)
            for i in range(count):
                ptrs.append(self.long(blk, BS - 204 - 4 * i))  # table filled from the end
            blk = self.long(blk, BS - 8)  # extension (T_LIST)
        out = bytearray()
        for p in ptrs:
            if self.ffs:
                out += self.blk(p)
            else:
                n = self.long(p, 12)
                out += self.d[p * BS + 24:p * BS + 24 + n]
        return bytes(out[:size])

    def walk(self, dirblk=None, prefix=''):
        dirblk = self.root if dirblk is None else dirblk
        for e in sorted(self.entries(dirblk), key=lambda b: self.bstr(b, BS - 80).lower()):
            name, st = self.bstr(e, BS - 80), self.slong(e, BS - 4)
            path = f'{prefix}{name}'
            if st == ST_USERDIR:
                yield path + '/', e, None
                yield from self.walk(e, path + '/')
            elif st == ST_FILE:
                yield path, e, self.long(e, BS - 188)


class Writer:
    """Build an AmigaDOS volume image in memory.

        w = Writer('PROGRAM', ffs=False)          # 1760 blocks = DD floppy
        w.mkdir('s'); w.add('s/startup-sequence', b'ultima6\\n')
        img = w.image()                           # bytes, len = blocks * 512
    """

    def __init__(self, label: str, ffs: bool = False, blocks: int = 1760, when: int = 690681600):
        self.label, self.ffs, self.blocks = label, ffs, blocks
        self.date = amiga_date(when)  # default 1991-11-21 00:00 UTC
        self.root = (2 + blocks - 1) // 2
        self.bitmap = self.root + 1
        if blocks - 2 > (BS - 4) * 8:
            raise ValueError('volume needs more than one bitmap block')
        self.img = bytearray(blocks * BS)
        self.used = {0, 1, self.root, self.bitmap}
        self.next = self.root + 2
        self.dirs = {'': self.root}
        self.files: set[str] = set()
        self.meta = {self.root}  # header/list blocks; checksummed in image() after linking
        self.boot = bytearray(b'DOS' + bytes([1 if ffs else 0]) + bytes(1020))
        struct.pack_into('>I', self.boot, 8, self.root)
        r = bytearray(BS)
        struct.pack_into('>I', r, 0, T_HEADER)
        struct.pack_into('>I', r, 12, HT)
        struct.pack_into('>i', r, 312, -1)            # bm_flag: bitmap valid
        struct.pack_into('>I', r, 316, self.bitmap)   # bm_pages[0]
        struct.pack_into('>3I', r, 420, *self.date)   # root alteration
        self._name(r, label)
        struct.pack_into('>3I', r, 472, *self.date)   # disk alteration
        struct.pack_into('>3I', r, 484, *self.date)   # creation
        struct.pack_into('>i', r, BS - 4, ST_ROOT)
        self._put(self.root, r)

    def _put(self, n: int, blk: bytearray) -> None:
        self.img[n * BS:(n + 1) * BS] = blk

    def _blk(self, n: int) -> bytearray:
        return bytearray(self.img[n * BS:(n + 1) * BS])

    @staticmethod
    def _name(blk: bytearray, name: str) -> None:
        b = name.encode('latin-1')
        if not 1 <= len(b) <= 30 or '/' in name or ':' in name:
            raise ValueError(f'bad AmigaDOS name {name!r}')
        blk[BS - 80] = len(b)
        blk[BS - 79:BS - 79 + len(b)] = b

    def alloc(self) -> int:
        for _ in range(self.blocks):
            n = self.next
            self.next = self.next + 1 if self.next + 1 < self.blocks else 2
            if n not in self.used:
                self.used.add(n)
                return n
        raise ValueError(f'volume {self.label!r} full')

    def free_blocks(self) -> int:
        return self.blocks - len(self.used)

    def _link(self, parent: int, name: str, hdr: int) -> None:
        """Insert header `hdr` at the head of its hash chain in directory block `parent`."""
        p = self._blk(parent)
        slot = 24 + 4 * name_hash(name)
        h = self._blk(hdr)
        struct.pack_into('>I', h, BS - 16, struct.unpack_from('>I', p, slot)[0])
        struct.pack_into('>I', h, BS - 12, parent)
        self._put(hdr, h)
        struct.pack_into('>I', p, slot, hdr)
        self._put(parent, p)

    def _parent(self, path: str) -> tuple[int, str]:
        head, _, name = path.rpartition('/')
        if head not in self.dirs:
            raise ValueError(f'no directory {head!r} for {path!r}')
        if path in self.dirs or path in self.files:
            raise ValueError(f'{path!r} exists')
        return self.dirs[head], name

    def mkdir(self, path: str) -> int:
        parent, name = self._parent(path)
        n = self.alloc()
        d = bytearray(BS)
        struct.pack_into('>I', d, 0, T_HEADER)
        struct.pack_into('>I', d, 4, n)
        struct.pack_into('>3I', d, 420, *self.date)
        self._name(d, name)
        struct.pack_into('>i', d, BS - 4, ST_USERDIR)
        self._put(n, d)
        self.meta.add(n)
        self._link(parent, name, n)
        self.dirs[path] = n
        return n

    def add(self, path: str, data: bytes) -> int:
        parent, name = self._parent(path)
        self.files.add(path)
        per = BS if self.ffs else BS - 24
        chunks = [data[i:i + per] for i in range(0, len(data), per)]
        hdr = self.alloc()
        data_blocks = [self.alloc() for _ in chunks]
        # header holds the first 72 pointers, each T_LIST extension block the next 72
        groups = [data_blocks[i:i + HT] for i in range(0, len(data_blocks), HT)] or [[]]
        owners = [hdr] + [self.alloc() for _ in groups[1:]]
        for gi, (owner, group) in enumerate(zip(owners, groups)):
            b = bytearray(BS)
            struct.pack_into('>I', b, 0, T_HEADER if gi == 0 else T_LIST)
            struct.pack_into('>I', b, 4, owner)
            struct.pack_into('>I', b, 8, len(group))
            if group:
                struct.pack_into('>I', b, 16, group[0] if gi == 0 else 0)
            for i, p in enumerate(group):
                struct.pack_into('>I', b, BS - 204 - 4 * i, p)
            if gi + 1 < len(owners):
                struct.pack_into('>I', b, BS - 8, owners[gi + 1])
            if gi == 0:
                struct.pack_into('>I', b, BS - 188, len(data))
                struct.pack_into('>3I', b, 420, *self.date)
                self._name(b, name)
            else:
                struct.pack_into('>I', b, BS - 12, hdr)
            struct.pack_into('>i', b, BS - 4, ST_FILE)
            self._put(owner, b)
            self.meta.add(owner)
        for seq, (n, c) in enumerate(zip(data_blocks, chunks), 1):
            b = bytearray(BS)
            if self.ffs:
                b[:len(c)] = c
            else:
                nxt = data_blocks[seq] if seq < len(data_blocks) else 0
                struct.pack_into('>5I', b, 0, T_DATA, hdr, seq, len(c), nxt)
                b[24:24 + len(c)] = c
                checksum(b, 20)
            self._put(n, b)
        self._link(parent, name, hdr)
        return hdr

    def set_boot(self, code: bytes) -> None:
        """Replace boot block bytes 12.. with `code` (DOS type and root pointer stay)."""
        if len(code) > 1024 - 12:
            raise ValueError('boot code longer than 1012 bytes')
        self.boot[12:] = bytes(code) + bytes(1024 - 12 - len(code))

    def image(self) -> bytes:
        boot = bytearray(self.boot)
        if any(boot[12:]):
            boot_checksum(boot)
        self.img[0:1024] = boot
        for n in self.meta:
            blk = self._blk(n)
            checksum(blk, 20)
            self._put(n, blk)
        bm = bytearray(BS)
        words = [0] * (BS // 4 - 1)
        for b in range(2, self.blocks):
            if b not in self.used:
                words[(b - 2) // 32] |= 1 << ((b - 2) % 32)
        struct.pack_into(f'>{len(words)}I', bm, 4, *words)
        checksum(bm, 0)
        self._put(self.bitmap, bm)
        return bytes(self.img)


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    with open(argv[0], 'rb') as f:
        vol = Volume(f.read())
    if argv[1] == 'ls':
        print(f'# volume {vol.name!r} {"FFS" if vol.ffs else "OFS"} blocks={vol.blocks} root={vol.root}')
        for path, _, size in vol.walk():
            print(f'{"" if size is None else size:>8}  {path}')
        return 0
    if argv[1] == 'x' and len(argv) == 3:
        for path, blk, size in vol.walk():
            dst = os.path.join(argv[2], path)
            if size is None:
                os.makedirs(dst, exist_ok=True)
            else:
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                with open(dst, 'wb') as f:
                    f.write(vol.file_bytes(blk))
        return 0
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
