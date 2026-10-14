#!/usr/bin/env python3
"""Tests for classify.py on small synthetic pseudocode snippets and images.

Run: python3 -I -B tools/rea/test_classify.py
"""
import importlib.util
import os
import struct
import unittest

_spec = importlib.util.spec_from_file_location(
    "classify", os.path.join(os.path.dirname(os.path.abspath(__file__)), "classify.py"))
classify = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(classify)

A4 = classify.A4

THUNK_BODY = """
long thunk_FUN_00200010(void)

{
  int unaff_A4;
  p = *(long *)(unaff_A4 + 0x1230);
  thunk_FUN_00200800(s_hi_>_00201000, unaff_A4 + -0x4321);
  FUN_00200600();
  v = DAT_00202468;
  *(code **)(unaff_A4 + 0x10) = FUN_00200a00;
  (**(code **)(_DAT_00000004 + -0xd2))();
  (**(code **)(*(int *)(unaff_A4 + 0x67d4) + -0x1e))();
  (**(code **)(unaff_A4 + -0x2222))();
  _DAT_00dff096 = 1;
  return 0;
}
"""


class ScanTests(unittest.TestCase):
    def setUp(self):
        self.sc = classify.scan("thunk_FUN_00200010", THUNK_BODY)

    def test_calls_exclude_own_signature(self):
        self.assertEqual(self.sc["calls"], [0x200600, 0x200800])

    def test_function_pointer_is_not_a_call(self):
        self.assertEqual(self.sc["fn_ptrs"], [0x200A00])

    def test_string_label_with_punctuation(self):
        self.assertEqual(self.sc["slabels"], [0x201000])

    def test_small_data_offsets(self):
        for off in (0x1230, -0x4321, 0x67D4, 0x10):
            self.assertIn(A4 + off, self.sc["a4"])

    def test_absolute_data_and_hw(self):
        self.assertIn(0x202468, self.sc["dat"])
        self.assertEqual(self.sc["hw"], ["DMACON"])

    def test_os_calls_and_fnptr_call(self):
        got = [(o["lib"], o["name"], o["lvo"], o["checked"]) for o in self.sc["os"]]
        self.assertEqual(got, [("exec", "FreeMem", -0xD2, True), ("dos", "Open", -0x1E, True)])
        self.assertEqual(self.sc["fnptr_calls"], [A4 - 0x2222])

    def test_recursive_call_kept(self):
        body = "void FUN_00202000(void)\n{\n  FUN_00202000();\n}\n"
        self.assertEqual(classify.scan("FUN_00202000", body)["calls"], [0x202000])

    def test_indexed_longword_load(self):
        body = "x = *(undefined4 *)(unaff_A4 + -0x1110 + (int)i * 4);\n"
        self.assertEqual(classify.scan("FUN_00200000", body)["a4_indexed"], [A4 - 0x1110])


class BaseLibraryTests(unittest.TestCase):
    def test_exec_absolute(self):
        self.assertEqual(classify.base_library("_DAT_00000004", ""), "exec")

    def test_known_globals(self):
        self.assertEqual(classify.base_library("*(int *)(unaff_A4 + 0x67d4)", ""), "dos")
        self.assertEqual(classify.base_library("DAT_00161ec4", ""), "dos")
        self.assertEqual(classify.base_library("DAT_00123456", ""), "global?")

    def test_local_resolves_through_chain(self):
        text = "c = DAT_00161ec4;\nb = c;\n"
        self.assertEqual(classify.base_library("b", text), "dos")

    def test_reassigned_local_is_unknown(self):
        text = "b = _DAT_00000004;\nb = c;\nc = foo();\n"
        self.assertEqual(classify.base_library("b", text), "?")

    def test_registers(self):
        self.assertEqual(classify.base_library("unaff_A4", ""), "a4-fnptr")
        self.assertEqual(classify.base_library("unaff_A6", ""), "a6?")

    def test_unknown_vector_is_unchecked(self):
        self.assertEqual(classify.lvo_name("dos", 0x999), ("-0x999", "u"))
        self.assertEqual(classify.lvo_name("exec", 0x228), ("OpenLibrary", "v"))


class StringIndexTests(unittest.TestCase):
    def test_exact_and_containing(self):
        idx = classify.StringIndex({0x1000: "schedule", 0x2000: "x"})
        self.assertEqual(idx.find(0x1000), 0x1000)
        self.assertEqual(idx.find(0x1004), 0x1000)     # inside "schedule"
        self.assertEqual(idx.find(0x1008), 0x1000)     # its terminating NUL
        self.assertIsNone(idx.find(0x1009))
        self.assertIsNone(idx.find(0x0FFF))


SEGS = [{"index": 0, "type": "CODE", "base": 0x100000, "size": 0x40, "loaded": 0x40},
        {"index": 1, "type": "DATA", "base": 0x100040, "size": 0x20, "loaded": 0x20}]


def image():
    img = bytearray(0x60)
    struct.pack_into(">HH", img, 0x00, 0x41FA, 0x0020 - 2)    # LEA d16(PC),A0 -> 0x100020
    struct.pack_into(">HH", img, 0x08, 0x487A, 0x0030 - 10)   # PEA d16(PC)     -> 0x100030
    struct.pack_into(">HI", img, 0x10, 0x4879, 0x100030)      # PEA abs.l (reloc at 0x12)
    img[0x20:0x26] = b"hello\0"
    img[0x30:0x34] = b"bye\0"
    struct.pack_into(">III", img, 0x40, 0x100020, 0x100030, 0x100000)   # pointer table
    struct.pack_into(">I", img, 0x50, 0x100030)                         # lone pointer
    return bytes(img)


class ImageTests(unittest.TestCase):
    sites = {0x100012, 0x100040, 0x100044, 0x100048, 0x100050}

    def test_pointer_runs(self):
        runs = classify.pointer_runs(image(), 0x100000, SEGS, self.sites)
        self.assertEqual(runs, [(0x100040, [0x100020, 0x100030, 0x100000]),
                                (0x100050, [0x100030])])
        starts = [r[0] for r in runs]
        self.assertEqual(classify.run_containing(runs, starts, 0x100048)[0], 0x100040)
        self.assertIsNone(classify.run_containing(runs, starts, 0x10004C))

    def test_code_run_needs_two_entries(self):
        segs = [dict(SEGS[0])]
        self.assertEqual(classify.pointer_runs(image(), 0x100000, segs, {0x100012}), [])

    def test_code_xrefs(self):
        idx = classify.StringIndex({0x100020: "hello", 0x100030: "bye"})
        got = classify.code_xrefs(image(), 0x100000, SEGS, self.sites, idx.find)
        self.assertEqual(sorted(got[0x100020]), [0x100000])
        self.assertEqual(sorted(got[0x100030]), [0x100008, 0x100012])

    def test_owner(self):
        self.assertEqual(classify.owner([0x100000, 0x100010], 0x10000F), 0x100000)
        self.assertIsNone(classify.owner([0x100010], 0x100000))


def proc(addr, hunk, calls=(), strings=(), kind="function", target=None):
    return {"address": addr, "name": "FUN_%08x" % addr, "kind": kind, "target": target,
            "hunk": hunk, "lines": 10, "calls": list(calls), "fn_ptrs": [], "os": [],
            "hw": [], "fnptr_calls": [], "strings": list(strings), "strings_img": [],
            "tables": {}, "callers": []}


class ClassifyTests(unittest.TestCase):
    def setUp(self):
        p = {
            0x200000: proc(0x200000, 11, calls=[0x200100, 0x200200]),
            0x200100: proc(0x200100, 17),                         # unknown hunk
            0x200200: proc(0x200200, 17, calls=[0x200300], strings=[0x300000]),
            0x200300: proc(0x200300, 13),
            0x200400: proc(0x200400, 1, kind="thunk", target=0x200000),
        }
        for a, r in p.items():
            r["callers"] = sorted(c for c, q in p.items()
                                  if q["kind"] != "thunk" and a in q["calls"])
        p[0x200400]["callers"] = p[0x200000]["callers"]
        self.data = {"procs": p, "strings": {0x300000: "\nNo response\n"}, "segments": SEGS}
        self.saved = classify.MILESTONE_SEEDS, classify.MILESTONE_STOP
        classify.MILESTONE_SEEDS, classify.MILESTONE_STOP = [0x200000], {0x200300: "x"}
        classify.classify(self.data)

    def tearDown(self):
        classify.MILESTONE_SEEDS, classify.MILESTONE_STOP = self.saved

    def test_hunk_default_and_strings(self):
        p = self.data["procs"]
        self.assertEqual((p[0x200000]["subsystem"], p[0x200000]["rule"]), ("magic", "hunk"))
        self.assertEqual((p[0x200200]["subsystem"], p[0x200200]["rule"]), ("converse", "strings"))

    def test_unknown_takes_caller_subsystem(self):
        r = self.data["procs"][0x200100]
        self.assertEqual((r["subsystem"], r["confidence"], r["rule"]), ("magic", "low", "callers"))

    def test_thunk_inherits_target(self):
        r = self.data["procs"][0x200400]
        self.assertEqual((r["class"], r["subsystem"], r["rule"]), ("game", "magic", "thunk"))
        self.assertTrue(r["role"].endswith("0x200000"))

    def test_milestone_cut(self):
        p = self.data["procs"]
        self.assertTrue(p[0x200200]["milestone1"])
        self.assertFalse(p[0x200300]["milestone1"])
        self.assertTrue(p[0x200300]["milestone1_stub"])


if __name__ == "__main__":
    unittest.main()
