#!/usr/bin/env python3
"""Tests for hunk2raw.py on synthetic hunk files built in memory.

Run: python3 -I tools/test_hunk2raw.py
  or python3 -I -m unittest discover -s tools -p test_hunk2raw.py
"""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
import struct
import tempfile
import unittest

_spec = importlib.util.spec_from_file_location(
    "hunk2raw", os.path.join(os.path.dirname(os.path.abspath(__file__)), "hunk2raw.py"))
hunk2raw = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(hunk2raw)

BASE = 0x00100000


def L(*v):
    return b"".join(struct.pack(">I", x) for x in v)


def W(*v):
    return b"".join(struct.pack(">H", x) for x in v)


def header(sizes, first=0, last=None, table=None, libs=()):
    """sizes: raw size longwords, or (raw, ext) for the both-bits extended form."""
    last = first + len(sizes) - 1 if last is None else last
    table = last + 1 if table is None else table
    out = L(0x3F3)
    for name in libs:
        b = name.encode() + b"\0" * (-len(name) % 4 or 4)
        out += L(len(b) // 4) + b
    out += L(0, table, first, last)
    for s in sizes:
        out += L(*s) if isinstance(s, tuple) else L(s)
    return out


def code(payload, kind=0x3E9):
    assert len(payload) % 4 == 0
    return L(kind, len(payload) // 4) + payload


def data(payload):
    return code(payload, 0x3EA)


def bss(longs):
    return L(0x3EB, longs)


def reloc32(groups):
    out = L(0x3EC)
    for target, offs in groups:
        out += L(len(offs), target, *offs)
    return out + L(0)


def reloc32short(groups, kind=0x3FC):
    words = b""
    for target, offs in groups:
        words += W(len(offs), target, *offs)
    words += W(0)
    if len(words) % 4:
        words += W(0)
    return L(kind) + words


def symbols(pairs):
    out = L(0x3F0)
    for name, value in pairs:
        b = name.encode() + b"\0" * (-len(name) % 4)
        out += L(len(b) // 4) + b + L(value)
    return out + L(0)


END = L(0x3F2)
BREAK = L(0x3F6)


def convert(blob, base=BASE):
    return hunk2raw.convert(blob, base)


def be32(image, addr):
    return struct.unpack_from(">I", image, addr - BASE)[0]


class LayoutTest(unittest.TestCase):
    def test_code_data_bss_layout(self):
        c = bytes(range(1, 13))                  # 3 longs, header says 3
        d = b"\xAA" * 8                          # 2 longs loaded, header says 4
        blob = header([3, 4, 2]) + code(c) + END + data(d) + END + bss(2) + END
        image, info = convert(blob)
        self.assertEqual(info["base"], "0x00100000")
        self.assertEqual(info["entry"], "0x00100000")
        segs = info["segments"]
        self.assertEqual([s["type"] for s in segs], ["CODE", "DATA", "BSS"])
        self.assertEqual([s["base"] for s in segs], ["0x00100000", "0x00100010", "0x00100020"])
        self.assertEqual([s["size"] for s in segs], [12, 16, 8])
        self.assertEqual([s["loaded_bytes"] for s in segs], [12, 8, 0])
        self.assertEqual([s["reloc_count"] for s in segs], [0, 0, 0])
        self.assertEqual(info["image_size"], 0x28)
        expected = c + b"\0" * 4 + d + b"\0" * 8 + b"\0" * 8
        self.assertEqual(image, expected)
        self.assertIsNone(info["overlay"])
        self.assertEqual(info["warnings"], [])

    def test_mem_flags_including_extended_form(self):
        blob = (header([0x40000001, 0x80000001, (0xC0000001, 0x00010002), 1])
                + code(L(0)) + END + code(L(0)) + END + data(L(0)) + END + bss(1) + END)
        _, info = convert(blob)
        self.assertEqual([s["mem_flags"] for s in info["segments"]],
                         ["CHIP", "FAST", "EXT:0x00010002", "ANY"])
        self.assertEqual([s["size"] for s in info["segments"]], [4, 4, 4, 4])

    def test_memory_flags_on_type_longword(self):
        blob = header([0x40000001]) + code(L(0x4E754E75), kind=0x400003E9) + END
        image, info = convert(blob)
        self.assertEqual(info["segments"][0]["type"], "CODE")
        self.assertEqual(image, L(0x4E754E75))

    def test_resident_library_list(self):
        blob = header([1], libs=["foo.library"]) + code(L(0)) + END
        _, info = convert(blob)
        self.assertEqual(info["resident_libraries"], ["foo.library"])

    def test_base_option(self):
        blob = header([2, 1]) + code(L(0, 0)) + END + data(L(0)) + END
        _, info = convert(blob, base=0x00200000)
        self.assertEqual(info["entry"], "0x00200000")
        self.assertEqual(info["segments"][1]["base"], "0x00200008")


class RelocTest(unittest.TestCase):
    def test_reloc32_across_segments(self):
        c = L(0x00000004, 0x00000008, 0xDEADBEEF)
        d = L(0x00000000, 0x11111111)
        blob = (header([3, 2]) + code(c) + reloc32([(1, [0]), (0, [4])]) + END
                + data(d) + reloc32([(0, [0])]) + END)
        image, info = convert(blob)
        b1 = BASE + 0x10
        self.assertEqual(info["segments"][1]["base"], hunk2raw.hx(b1))
        self.assertEqual(be32(image, BASE), b1 + 4)
        self.assertEqual(be32(image, BASE + 4), BASE + 8)
        self.assertEqual(be32(image, BASE + 8), 0xDEADBEEF)
        self.assertEqual(be32(image, b1), BASE)
        self.assertEqual(be32(image, b1 + 4), 0x11111111)
        self.assertEqual([s["reloc_count"] for s in info["segments"]], [2, 1])
        self.assertEqual(info["warnings"], [])

    def test_reloc32short_odd_word_count_padding(self):
        # 5 words (count, target, off, off, terminator) -> 2 bytes of padding before HUNK_END.
        c = L(0x00000000, 0x00000004, 0x22222222)
        blob = header([3, 2]) + code(c) + reloc32short([(1, [0, 4])]) + END + data(L(1, 2)) + END
        self.assertEqual(len(reloc32short([(1, [0, 4])])), 4 + 12)
        image, info = convert(blob)
        b1 = BASE + 0x10
        self.assertEqual(be32(image, BASE), b1)
        self.assertEqual(be32(image, BASE + 4), b1 + 4)
        self.assertEqual(be32(image, BASE + 8), 0x22222222)
        self.assertEqual(info["segments"][0]["reloc_count"], 2)

    def test_drel32_is_reloc32short_alias(self):
        c = L(0x00000002)
        blob = (header([1, 1]) + code(c) + reloc32short([(1, [0])], kind=0x3F7) + END
                + data(L(0)) + END)
        image, info = convert(blob)
        self.assertEqual(be32(image, BASE), BASE + 8 + 2)
        self.assertEqual(info["segments"][0]["reloc_count"], 1)

    def test_reloc_out_of_range_offset_fails(self):
        blob = header([1]) + code(L(0)) + reloc32([(0, [4])]) + END
        with self.assertRaisesRegex(hunk2raw.HunkError, "outside segment size"):
            convert(blob)

    def test_reloc_to_non_root_hunk_fails(self):
        blob = header([1]) + code(L(0)) + reloc32([(5, [0])]) + END
        with self.assertRaisesRegex(hunk2raw.HunkError, "not a root hunk"):
            convert(blob)

    def test_stored_offset_beyond_target_warns(self):
        blob = header([1, 1]) + code(L(0x100)) + reloc32([(1, [0])]) + END + data(L(0)) + END
        _, info = convert(blob)
        self.assertTrue(any("exceeds the target segment size" in w for w in info["warnings"]))


class SymbolTest(unittest.TestCase):
    def test_symbol_hunk(self):
        blob = (header([2, 3]) + code(L(0, 0)) + symbols([("_start", 0), ("_main", 4)]) + END
                + data(L(0, 0, 0)) + symbols([("_table", 8)]) + END)
        _, info = convert(blob)
        self.assertEqual(info["symbols"], {"_start": "0x00100000", "_main": "0x00100004",
                                           "_table": "0x00100010"})
        self.assertEqual(info["symbol_list"][2], {"name": "_table", "hunk": 1, "offset": 8,
                                                  "address": "0x00100010"})

    def test_debug_and_name_hunks_skipped(self):
        dbg = L(0x3F1, 2, 0x12345678, 0x9ABCDEF0)
        nm = L(0x3E8, 1) + b"txt\0"
        blob = header([1]) + nm + code(L(0x4E75_0000)) + dbg + END
        image, info = convert(blob)
        self.assertEqual(image, L(0x4E750000))
        self.assertEqual(info["segments"][0]["name"], "txt")
        self.assertEqual(info["segments"][0]["debug_bytes"], 8)

    def test_ext_hunk_reported(self):
        ext = (L(0x3EF) + L((1 << 24) | 1) + b"_x\0\0" + L(4)           # EXT_DEF _x = 4
               + L((129 << 24) | 1) + b"_y\0\0" + L(1, 0)                # EXT_REF32 _y, 1 ref
               + L((130 << 24) | 1) + b"_z\0\0" + L(16, 0) + L(0))       # EXT_COMMON, 0 refs
        blob = header([2]) + code(L(0, 0)) + ext + END
        _, info = convert(blob)
        self.assertEqual(info["ext"], [
            {"name": "_x", "type": 1, "value": "0x00000004", "hunk": 0},
            {"name": "_y", "type": 129, "ref_count": 1, "hunk": 0},
            {"name": "_z", "type": 130, "common_size": 16, "ref_count": 0, "hunk": 0},
        ])


class OverlayTest(unittest.TestCase):
    def overlay_table(self, first_hunk):
        # M+2 = 2, M+1 = 1 zero longword, one 8-longword node entry.
        body = L(2, 0, 0x1234, 0, 0, 1, 1, first_hunk, first_hunk, 0)
        return L(0x3F5, len(body) // 4 - 1) + body

    def test_overlay_with_node_headers_listed_not_dropped(self):
        root = header([2], first=0, last=0, table=3) + code(L(0x4E75_0000, 0)) + END
        node = (header([1, 2], first=1, last=2, table=3)
                + code(L(0)) + reloc32([(0, [0])]) + END + data(L(7, 8)) + END)
        blob = root + self.overlay_table(1) + node + BREAK
        image, info = convert(blob)
        self.assertEqual(image, L(0x4E750000, 0))       # root only
        self.assertEqual(len(info["segments"]), 1)
        ov = info["overlay"]
        self.assertIsNotNone(ov)
        self.assertEqual(ov["max_level"], 0)
        self.assertEqual(ov["entries"][0]["first_hunk"], 1)
        self.assertEqual(ov["entries"][0]["seek_offset"], 0x1234)
        node0 = ov["nodes"][0]
        self.assertTrue(node0["has_header"])
        self.assertEqual([(h["index"], h["type"], h["size"], h["reloc_count"]) for h in node0["hunks"]],
                         [(1, "CODE", 4, 1), (2, "DATA", 8, 0)])
        self.assertTrue(any("HUNK_OVERLAY" in w for w in info["warnings"]))

    def test_overlay_nodes_without_headers(self):
        root = header([1], first=0, last=0, table=3) + code(L(0)) + END
        blob = (root + self.overlay_table(1) + code(L(0, 0)) + END + BREAK
                + bss(4) + END + BREAK)
        _, info = convert(blob)
        nodes = info["overlay"]["nodes"]
        self.assertEqual([[(h["index"], h["type"], h["size"]) for h in n["hunks"]] for n in nodes],
                         [[(1, "CODE", 8)], [(2, "BSS", 16)]])
        self.assertFalse(nodes[0]["has_header"])

    def test_break_in_root_fails(self):
        blob = header([1]) + BREAK + code(L(0)) + END
        with self.assertRaisesRegex(hunk2raw.HunkError, "HUNK_BREAK"):
            convert(blob)


class ErrorTest(unittest.TestCase):
    VALID = (header([3, 2, 1]) + code(L(4, 0, 0)) + reloc32([(1, [0])]) + END
             + data(L(0, 0)) + reloc32short([(0, [0])]) + symbols([("_d", 0)]) + END
             + bss(1) + END)

    def test_valid_fixture_parses(self):
        convert(self.VALID)

    def test_every_truncation_fails_loudly(self):
        for n in range(len(self.VALID)):
            with self.subTest(cut=n):
                with self.assertRaises(hunk2raw.HunkError):
                    convert(self.VALID[:n])

    def test_unknown_hunk_type_fails(self):
        for t, msg in ((0x3ED, "HUNK_RELOC16"), (0x12345, "0x12345")):
            blob = header([1]) + code(L(0)) + L(t, 0) + END
            with self.subTest(type=t):
                with self.assertRaisesRegex(hunk2raw.HunkError, msg):
                    convert(blob)

    def test_not_a_load_file(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "not an AmigaOS load file"):
            convert(L(0x3E7, 0))

    def test_bad_header_range(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "bad hunk header"):
            convert(L(0x3F3, 0, 1, 0, 1, 1))

    def test_data_larger_than_header_size_fails(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "exceeds header size"):
            convert(header([1]) + code(L(0, 0)) + END)

    def test_second_content_block_fails(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "second content block"):
            convert(header([1]) + code(L(0)) + data(L(0)) + END)

    def test_end_without_content_fails(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "without CODE/DATA/BSS"):
            convert(header([1]) + END)

    def test_trailing_data_fails(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "after last root hunk"):
            convert(header([1]) + code(L(0)) + END + L(0x3E9))

    def test_unaligned_base_fails(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "8-byte aligned"):
            convert(header([1]) + code(L(0)) + END, base=0x100004)

    def test_huge_header_size_fails(self):
        with self.assertRaisesRegex(hunk2raw.HunkError, "too large"):
            convert(header([0x3FFFFFFF]) + bss(1) + END)


class CliTest(unittest.TestCase):
    def run_main(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = hunk2raw.main(args)
        return rc, out.getvalue(), err.getvalue()

    def test_writes_bin_and_json_with_hashes(self):
        blob = header([2, 1]) + code(L(4, 0)) + reloc32([(1, [0])]) + END + bss(1) + END
        with tempfile.TemporaryDirectory() as d:
            src, ob, oj = (os.path.join(d, n) for n in ("prog", "prog.bin", "prog.json"))
            with open(src, "wb") as f:
                f.write(blob)
            rc, out, _ = self.run_main([src, ob, oj, "--base", "0x00200000"])
            self.assertEqual(rc, 0)
            with open(ob, "rb") as f:
                image = f.read()
            with open(oj) as f:
                info = json.load(f)
        self.assertEqual(info["input_sha256"], hashlib.sha256(blob).hexdigest())
        self.assertEqual(info["output_sha256"], hashlib.sha256(image).hexdigest())
        self.assertEqual(info["input_size"], len(blob))
        self.assertEqual(image[:4], L(0x00200008 + 4))
        self.assertIn("entry 0x00200000", out)

    def test_bad_input_exits_nonzero_without_outputs(self):
        with tempfile.TemporaryDirectory() as d:
            src, ob, oj = (os.path.join(d, n) for n in ("prog", "prog.bin", "prog.json"))
            with open(src, "wb") as f:
                f.write(header([2]) + code(L(0, 0))[:-4])
            rc, _, err = self.run_main([src, ob, oj])
            self.assertNotEqual(rc, 0)
            self.assertIn("truncated", err)
            self.assertFalse(os.path.exists(ob))
            self.assertFalse(os.path.exists(oj))


if __name__ == "__main__":
    unittest.main()
