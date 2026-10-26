#!/usr/bin/env python3
"""Tests for port_map.py on a small synthetic source tree and inventory.

Run: python3 -I -B tools/rea/test_port_map.py
"""
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "port_map.py")
_spec = importlib.util.spec_from_file_location("port_map", SCRIPT)
port_map = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(port_map)

GAME_RS = r"""/// Original: 0x200000 (the port)
pub fn port() {}

fn seam() {
    todo_stub!(
        "0x200000 call the port (fallback: none)",
        ()
    )
}

/// Original: 0x200200 [partial] (part of it)
fn half() {
    todo_stub!("0x200200 the rest", ())
}

fn other() {
    todo_stub!("0x200100 one-line stub", "todo_stub!(\"0x200300 in the fallback string\", ())")
}

fn late<'a>(x: &'a str) -> char {
    let q = '"';
    todo_stub!("0x200000 the port again", '\'')
}

// todo_stub!(
// "0x200300 in a line comment", ())
/* todo_stub!(
   "0x200300 in a block comment", ()) /* nested */ todo_stub!("0x200300 after the nested one", ()) */
const RAW: &str = r#"todo_stub!(
    "0x200300 in a raw string", ())"#;
fn unnamed() {
    todo_stub!("no address here", ())
}
fn named_in_code() {
    // fn decoy_in_a_comment
    todo_stub!( /* why */
        "0x200400 after a comment", ())
}

fn also_in_code() {
    let s = "fn decoy_in_a_string";
    todo_stub!("0x200500 after a string", ())
}
"""

INTRO_RS = r"""/// Original: u 0x200000 (the port)
fn port() {}

fn seam() {
    crate::todo_stub!(
        "u 0x200100 the next line",
        ()
    )
}

fn guard() {
    crate::todo_stub!("u 0x200000 a guard inside the port", ())
}
/* crate::todo_stub!(
   "u 0x200300 in a block comment", ()) */
// crate::todo_stub!("u 0x200300 in a line comment", ())
const RAW: &str = r#"crate::todo_stub!(
    "u 0x200300 in a raw string", ())"#;
"""


def proc(addr, m1=True):
    return {"address": "0x%x" % addr, "kind": "function", "subsystem": "objects",
            "milestone1": m1, "milestone1_stub": False}


INVENTORY = {"procedures": [proc(0x200000), proc(0x200100, m1=False), proc(0x200200),
                            proc(0x200300)]}
U_PROCS = {0x200000: 0, 0x200100: 0, 0x200300: 1}


class PortMapTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        for name, text in (("game.rs", GAME_RS), ("intro.rs", INTRO_RS)):
            with open(os.path.join(self.dir.name, name), "w", encoding="utf-8") as f:
                f.write(text)
        self.marks = port_map.scan(self.dir.name)
        self.u_marks = port_map.scan_u(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def rows(self):
        rows, totals, m1_totals, n, data = port_map.build(INVENTORY, self.marks)
        return {r.split(" | ")[0][2:]: r for r in rows}, port_map.render(
            rows, totals, m1_totals, n, data, self.marks)

    def u_rows(self):
        rows, totals, n, data = port_map.build_u(U_PROCS, self.u_marks)
        return {r.split(" | ")[0][2:]: r for r in rows}, port_map.render_u(
            rows, totals, n, data, self.u_marks)

    def test_stub_with_its_id_on_the_next_line(self):
        self.assertIn(("stubbed", "game.rs:5 in seam"), self.marks[0x200000])
        self.assertEqual(self.u_marks[0x200100], [("stubbed", "intro.rs:5 in seam")])

    def test_one_line_stub(self):
        rows, _ = self.rows()
        self.assertEqual(rows["0x200100"], "| 0x200100 | objects |  | `game.rs:17 in other` | stubbed |")

    def test_comment_before_the_id(self):
        self.assertEqual(self.marks[0x200400], [("stubbed", "game.rs:36 in named_in_code")])

    def test_site_names_the_fn_in_code(self):
        self.assertEqual(self.marks[0x200500], [("stubbed", "game.rs:42 in also_in_code")])

    def test_comments_and_literals_hold_no_stub(self):
        self.assertNotIn(0x200300, self.marks)
        self.assertNotIn(0x200300, self.u_marks)
        rows, _ = self.rows()
        self.assertEqual(rows["0x200300"], "| 0x200300 | objects | M |  | - |")

    def test_ported_and_stubbed_shows_both(self):
        rows, _ = self.rows()
        self.assertEqual(rows["0x200000"],
                         "| 0x200000 | objects | M | `game.rs::port` | ported; stub at `game.rs:5 in seam`; "
                         "`game.rs:22 in late` |")
        self.assertEqual(rows["0x200200"],
                         "| 0x200200 | objects | M | `game.rs::half` | partial; stub at `game.rs:13 in half` |")

    def test_totals_count_ported_addresses_named_by_a_stub(self):
        _, text = self.rows()
        totals = next(line for line in text.split("\n") if line.startswith("Totals:"))
        self.assertEqual(totals, "Totals: functions 4: ported 1 (also named by a stub 1), partial 1, stubbed 1, "
                                 "- 1. Milestone 1 (M + S) 3: ported 1 (also named by a stub 1), partial 1, - 1.")

    def test_u_table(self):
        rows, text = self.u_rows()
        self.assertEqual(rows["u 0x200000"],
                         "| u 0x200000 | 0 | `intro.rs::port` | ported; stub at `intro.rs:12 in guard` |")
        self.assertEqual(rows["u 0x200100"], "| u 0x200100 | 0 | `intro.rs:5 in seam` | stubbed |")
        self.assertIn("Totals: u functions 3: ported 1 (also named by a stub 1), stubbed 1, - 1.", text)

    def test_stub_naming_no_address_is_listed(self):
        self.assertEqual(port_map.unnamed_stubs(self.dir.name), (["game.rs:32"], 9))

    def test_stub_naming_no_address_stops_the_run(self):
        inventory = os.path.join(self.dir.name, "inventory.json")
        out = os.path.join(self.dir.name, "port-map.md")
        with open(inventory, "w", encoding="utf-8") as f:
            json.dump(INVENTORY, f)
        run = subprocess.run(
            [sys.executable, "-I", "-B", SCRIPT, "--src", self.dir.name, "--inventory", inventory,
             "--u-procedures", os.path.join(self.dir.name, "none.json"), "--out", out],
            capture_output=True, text=True, check=False)
        self.assertEqual(run.returncode, 1)
        self.assertEqual(run.stderr.strip(), "port-map: 1 of 9 todo_stub! calls name no address: game.rs:32")
        self.assertFalse(os.path.exists(out))


if __name__ == "__main__":
    unittest.main()
