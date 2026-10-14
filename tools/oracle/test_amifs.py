#!/usr/bin/env python3
"""Tests for mkadf.py (blank floppy writer) and amifs.py (OFS/FFS reader) on synthetic images.

Run: python3 -I tools/oracle/test_amifs.py
"""
import importlib.util
import os
import struct
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f"{name}.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


mkadf = load("mkadf")
amifs = load("amifs")
BS = 512


def name_hash(name):
    h = len(name)
    for c in name.upper():
        h = (h * 13 + ord(c)) & 0x7FF
    return h % 72


def put_file(img, ffs, parent, hdr, name, data, first_data):
    """Write one file into a blank image: header block `hdr`, data blocks from `first_data`."""
    per = BS if ffs else BS - 24
    chunks = [data[i:i + per] for i in range(0, len(data), per)] or [b""]
    blocks = list(range(first_data, first_data + len(chunks)))
    h = bytearray(BS)
    struct.pack_into(">I", h, 0, 2)
    struct.pack_into(">I", h, 4, hdr)
    struct.pack_into(">I", h, 8, len(blocks))
    struct.pack_into(">I", h, 16, blocks[0])
    for i, b in enumerate(blocks):
        struct.pack_into(">I", h, BS - 204 - 4 * i, b)
    struct.pack_into(">I", h, BS - 188, len(data))
    nb = name.encode()
    h[BS - 80] = len(nb)
    h[BS - 79:BS - 79 + len(nb)] = nb
    struct.pack_into(">I", h, BS - 12, parent)
    struct.pack_into(">i", h, BS - 4, -3)
    img[hdr * BS:(hdr + 1) * BS] = h
    for seq, (b, c) in enumerate(zip(blocks, chunks), 1):
        d = bytearray(BS)
        if ffs:
            d[:len(c)] = c
        else:
            struct.pack_into(">5I", d, 0, 8, hdr, seq, len(c), blocks[seq] if seq < len(blocks) else 0)
            d[24:24 + len(c)] = c
        img[b * BS:(b + 1) * BS] = d
    root = parent * BS
    struct.pack_into(">I", img, root + 24 + 4 * name_hash(name), hdr)


class BlankDisk(unittest.TestCase):
    def test_layout(self):
        img = mkadf.make("U6TEST", ffs=False)
        self.assertEqual(len(img), 1760 * BS)
        self.assertEqual(img[:4], b"DOS\0")
        self.assertEqual(struct.unpack_from(">I", img, 8)[0], 880)
        # root and bitmap checksums: block longwords sum to 0
        for blk in (880, 881):
            self.assertEqual(sum(struct.unpack_from(">128I", img, blk * BS)) & 0xFFFFFFFF, 0)
        # bitmap: blocks 880/881 used, block 2 and 1759 free
        words = struct.unpack_from(">127I", img, 881 * BS + 4)
        used = lambda b: not (words[(b - 2) // 32] >> ((b - 2) % 32)) & 1
        self.assertTrue(used(880) and used(881))
        self.assertFalse(used(2) or used(1759))
        vol = amifs.Volume(img)
        self.assertEqual((vol.name, vol.ffs, vol.root), ("U6TEST", False, 880))
        self.assertEqual(list(vol.walk()), [])

    def test_ffs_flag(self):
        self.assertEqual(mkadf.make("X", ffs=True)[:4], b"DOS\1")


class ReadFiles(unittest.TestCase):
    def roundtrip(self, ffs):
        img = bytearray(mkadf.make("V", ffs=ffs))
        big = bytes(range(256)) * 5  # 1280 bytes: 3 OFS data blocks, 3 FFS blocks
        put_file(img, ffs, 880, 900, "big.dat", big, 901)
        put_file(img, ffs, 880, 910, "s", b"ultcli\n", 911)
        vol = amifs.Volume(bytes(img))
        listing = {p: (b, s) for p, b, s in vol.walk()}
        self.assertEqual({p: s for p, (_, s) in listing.items()}, {"big.dat": 1280, "s": 7})
        self.assertEqual(vol.file_bytes(listing["big.dat"][0]), big)
        self.assertEqual(vol.file_bytes(listing["s"][0]), b"ultcli\n")
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "v.adf")
            with open(path, "wb") as f:
                f.write(img)
            self.assertEqual(amifs.main([path, "x", os.path.join(d, "out")]), 0)
            with open(os.path.join(d, "out", "big.dat"), "rb") as f:
                self.assertEqual(f.read(), big)

    def test_ofs(self):
        self.roundtrip(ffs=False)

    def test_ffs(self):
        self.roundtrip(ffs=True)

    def test_rejects_non_dos(self):
        with self.assertRaises(ValueError):
            amifs.Volume(b"\0" * (1760 * BS))


if __name__ == "__main__":
    unittest.main()
