#!/usr/bin/env python3
"""Read-only AmigaDOS OFS/FFS reader for ADF floppies and RDB-less HDF partitions.

    python3 -I amifs.py IMAGE ls             # tree with sizes
    python3 -I amifs.py IMAGE x OUTDIR       # extract every file

Block size 512. Root = (2 + blocks - 1) // 2 (880 on a DD floppy). OFS data blocks carry a
24-byte header; FFS data blocks are raw. Files are listed in on-disk hash order.
"""
import os
import struct
import sys

BS = 512
T_HEADER, T_LIST, T_DATA = 2, 16, 8
ST_ROOT, ST_USERDIR, ST_FILE = 1, 2, -3
HT = BS // 4 - 56  # 72 hash slots


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
