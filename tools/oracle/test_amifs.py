#!/usr/bin/env python3
"""Tests for mkadf.py (blank + game floppies) and amifs.py (OFS/FFS reader and writer).

Synthetic images only, except InstallTable, which reads installu6 from the local game copy
($U6_GAMES or ~/Games/ultima6) and is skipped when it is absent.

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


def load_path(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
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


def block_sums_ok(img, blk):
    return sum(struct.unpack_from(">128I", img, blk * BS)) & 0xFFFFFFFF == 0


class Writer(unittest.TestCase):
    def build(self, ffs):
        w = amifs.Writer("WTEST", ffs=ffs)
        w.mkdir("s")
        w.mkdir("savegame")
        w.add("s/startup-sequence", b"ultima6\nendcli\n")
        big = bytes((i * 7) & 0xFF for i in range(100_000))  # 205 OFS / 196 FFS blocks: 2 T_LIST blocks
        w.add("game", big)
        w.add("empty", b"")
        # same hash slot as "game" on purpose: chain of two
        other = next(n for n in (f"x{i}" for i in range(10000)) if amifs.name_hash(n) == amifs.name_hash("game"))
        w.add(other, b"chained")
        for i in range(70):
            w.add(f"savegame/OBJBLK{chr(65 + i // 8)}{chr(65 + i % 8)}", bytes([i]) * (i * 13))
        return w, w.image(), big, other

    def roundtrip(self, ffs):
        w, img, big, other = self.build(ffs)
        vol = amifs.Volume(img)
        self.assertEqual((vol.name, vol.ffs), ("WTEST", ffs))
        files = {p: (b, s) for p, b, s in vol.walk()}
        self.assertEqual(vol.file_bytes(files["game"][0]), big)
        self.assertEqual(vol.file_bytes(files["s/startup-sequence"][0]), b"ultima6\nendcli\n")
        self.assertEqual(vol.file_bytes(files["empty"][0]), b"")
        self.assertEqual(vol.file_bytes(files[other][0]), b"chained")
        self.assertEqual(vol.file_bytes(files["savegame/OBJBLKIF"][0]), bytes([69]) * (69 * 13))
        self.assertEqual(sum(1 for p in files if p.startswith("savegame/") and not p.endswith("/")), 70)
        # every header/list/root block and every OFS data block sums to 0; bitmap too
        for n in w.meta:
            self.assertTrue(block_sums_ok(img, n), n)
        self.assertTrue(block_sums_ok(img, w.bitmap))
        if not ffs:
            for n in w.used - w.meta - {0, 1, w.bitmap}:
                self.assertTrue(block_sums_ok(img, n), n)
        # bitmap free bits == blocks not handed out
        words = struct.unpack_from(">127I", img, w.bitmap * BS + 4)
        free = sum(1 for b in range(2, 1760) if (words[(b - 2) // 32] >> ((b - 2) % 32)) & 1)
        self.assertEqual(free, w.free_blocks())
        # extension chain: "game" header -> T_LIST -> T_LIST -> 0
        hdr = files["game"][0]
        ext, chain = struct.unpack_from(">I", img, hdr * BS + BS - 8)[0], 0
        while ext:
            self.assertEqual(struct.unpack_from(">I", img, ext * BS)[0], 16)
            self.assertEqual(struct.unpack_from(">I", img, ext * BS + BS - 12)[0], hdr)
            ext, chain = struct.unpack_from(">I", img, ext * BS + BS - 8)[0], chain + 1
        per = BS if ffs else BS - 24
        self.assertEqual(chain, (-(-len(big) // per) + 71) // 72 - 1)

    def test_ofs(self):
        self.roundtrip(ffs=False)

    def test_ffs(self):
        self.roundtrip(ffs=True)

    def test_deterministic(self):
        self.assertEqual(self.build(False)[1], self.build(False)[1])

    def test_boot_block(self):
        w = amifs.Writer("B")
        w.set_boot(bytes.fromhex("43fa001c4eaeffa04a806700000c20402068001670004e7570ff6000fffa") + b"dos.library\0")
        img = w.image()
        total = 0
        for (v,) in struct.iter_unpack(">I", img[:1024]):
            total += v
            if total > 0xFFFFFFFF:
                total = (total & 0xFFFFFFFF) + 1
        self.assertEqual(total, 0xFFFFFFFF)  # add-with-carry sum of a bootable block
        self.assertEqual(img[:4], b"DOS\0")
        self.assertEqual(amifs.Writer("C").image()[4:8], b"\0\0\0\0")  # no code: not bootable

    def test_full_and_bad_names(self):
        w = amifs.Writer("F")
        with self.assertRaises(ValueError):
            w.add("a" * 31, b"")
        with self.assertRaises(ValueError):
            w.add("nodir/x", b"")
        w.add("x", b"1")
        with self.assertRaises(ValueError):
            w.add("x", b"2")
        with self.assertRaises(ValueError):
            w.add("huge", bytes(900_000))

    def test_hash_matches_reference(self):
        # reference values from the loop the existing synthetic writer above uses
        for n in ("startup-sequence", "OBJBLKAA", "u6.set", "ULTIMA_Aa.MOD", "savegame"):
            self.assertEqual(amifs.name_hash(n), name_hash(n))


class Verify(unittest.TestCase):
    def test_manifest_roundtrip_and_mismatch(self):
        import hashlib
        import json
        with tempfile.TemporaryDirectory() as d:
            m = {"volumes": {}}
            for v in mkadf.VOLUMES:
                w = amifs.Writer(v)
                w.mkdir("s")
                files = {"s/startup-sequence": b"ultima6\n", f"{v.lower()}.dat": bytes(range(256)) * 9}
                for p, data in files.items():
                    w.add(p, data)
                with open(os.path.join(d, f"{v}.adf"), "wb") as f:
                    f.write(w.image())
                m["volumes"][v] = [{"path": p, "sha256": hashlib.sha256(x).hexdigest()} for p, x in files.items()]
            with open(os.path.join(d, "manifest.json"), "w") as f:
                json.dump(m, f)
            self.assertEqual(mkadf.verify(d), [])
            m["volumes"]["PLAYER"][1]["sha256"] = "0" * 64
            with open(os.path.join(d, "manifest.json"), "w") as f:
                json.dump(m, f)
            self.assertEqual(mkadf.verify(d), ["PLAYER:player.dat: amifs differs"])


GAMES = os.environ.get("U6_GAMES", os.path.expanduser("~/Games/ultima6"))
INSTALLU6 = os.path.join(GAMES, "files", "d1", "INSTALL", "installu6")


@unittest.skipUnless(os.path.exists(INSTALLU6), "no local game copy")
class InstallTable(unittest.TestCase):
    def test_table(self):
        with open(INSTALLU6, "rb") as f:
            entries, boot = mkadf.install_table(f.read())
        self.assertEqual(len(entries), 129)
        by = {(e["volume"], e["dst"]): e for e in entries if e["floppy"]}
        self.assertEqual(by[("PROGRAM", "ultima6")]["src"], "ultcli")  # flags 3: floppy-only, decrunched
        self.assertTrue(by[("PROGRAM", "ultima6")]["floppy"])
        self.assertEqual(by[("PROGRAM", "s/startup-sequence")]["src"], "start")
        self.assertFalse(any(e["floppy"] for e in entries if e["src"] in ("ultima6", "ultima6.info", "disk.info")))
        self.assertEqual(sum(1 for e in entries if e["dst"].startswith("savegame/")), 70)
        self.assertTrue(all(e["volume"] == "PLAYER" for e in entries if e["dst"].startswith("savegame/")))
        self.assertEqual({e["volume"] for e in entries if e["src_volume"] == "DATA2"}, {"POPULACE"})
        self.assertEqual(boot[:4], b"DOS\0")
        self.assertEqual(sum(e["floppy"] for e in entries), 126)

    def test_game_first_patches(self):
        pp20 = load_path("pp20", os.path.join(HERE, "..", "pp20.py"))
        with open(os.path.join(GAMES, "files", "d1", "INSTALL", "ultcli"), "rb") as f:
            ultcli = pp20.decrunch(f.read())
        out, what = mkadf.game_first_patches("ultima6", ultcli)
        self.assertEqual(out[0x3E4:0x3EA].hex(), "7e017c007a00")  # moveq #1,d7 ; moveq #0,d6 ; moveq #0,d5
        self.assertEqual(sum(a != b for a, b in zip(out, ultcli)), 1)
        with self.assertRaises(ValueError):
            mkadf.game_first_patches("ultima6", out)  # already patched: expected bytes absent
        with open(os.path.join(GAMES, "files", "d2", "DATA1", "objlist"), "rb") as f:
            objlist = pp20.decrunch(f.read())
        out, _ = mkadf.game_first_patches("savegame/objlist", objlist)
        self.assertEqual((out[0x1C71], out[0x1C72]), (1, 1))
        self.assertEqual(sum(a != b for a, b in zip(out, objlist)), 2)
        self.assertIsNone(mkadf.game_first_patches("game", b"x"))


if __name__ == "__main__":
    unittest.main()
