#!/usr/bin/env python3
"""Tests for galmuri.py (the Hangul table): synthetic BDF records only, no download.

Run: python3 -I -B tools/font/test_galmuri.py
"""
import importlib.util
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f"{name}.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


galmuri = load("galmuri")


def record(enc, bbx, rows):
    """One BDF record: ENCODING, BBX and its bitmap rows (hex strings)."""
    return "\n".join(
        [f"STARTCHAR U+{enc:04X}", f"ENCODING {enc}", "SWIDTH 533 0", "DWIDTH 8 0"]
        + ["BBX " + " ".join(str(v) for v in bbx), "BITMAP"]
        + rows
        + ["ENDCHAR"]
    )


def bdf(*records):
    return "STARTFONT 2.1\nCHARS %d\n%s\nENDFONT\n" % (len(records), "\n".join(records))


class Placement(unittest.TestCase):
    def test_a_7x7_glyph_on_the_baseline_fills_rows_0_to_6(self):
        rows = ["FE", "82", "82", "82", "82", "82", "FE"]
        out = galmuri.table(bdf(record(0x41, (7, 7, 0, 0), rows)), 0x41, 0x41)
        self.assertEqual(list(out), [0xFE, 0x82, 0x82, 0x82, 0x82, 0x82, 0xFE, 0x00])

    def test_the_y_offset_raises_the_glyph_and_the_x_offset_shifts_it_right(self):
        out = galmuri.table(bdf(record(0x41, (2, 2, 3, 1), ["C0", "40"])), 0x41, 0x41)
        # Top row = 7 - (1 + 2) = 4; columns 3 and 4.
        self.assertEqual(list(out), [0, 0, 0, 0, 0x18, 0x08, 0, 0])

    def test_bits_past_the_bbx_width_are_dropped(self):
        out = galmuri.table(bdf(record(0x41, (3, 1, 0, 0), ["FF"])), 0x41, 0x41)
        self.assertEqual(out[6], 0xE0)

    def test_a_negative_y_offset_reaches_the_row_below_the_baseline(self):
        out = galmuri.table(bdf(record(0x41, (1, 2, 0, -1), ["80", "80"])), 0x41, 0x41)
        self.assertEqual(list(out), [0, 0, 0, 0, 0, 0, 0x80, 0x80])

    def test_cells_come_in_code_point_order_and_others_are_ignored(self):
        f = bdf(
            record(0x43, (1, 1, 0, 0), ["80"]),
            record(0x20, (0, 0, 0, 0), []),
            record(0x42, (1, 1, 1, 0), ["80"]),
        )
        out = galmuri.table(f, 0x42, 0x43)
        self.assertEqual(out[6], 0x40)
        self.assertEqual(out[14], 0x80)


class Refusals(unittest.TestCase):
    def check(self, text, first=0x41, last=0x41):
        with self.assertRaises(galmuri.FontError):
            galmuri.table(text, first, last)

    def test_a_glyph_that_does_not_fit_the_cell(self):
        self.check(bdf(record(0x41, (9, 1, 0, 0), ["FF80"])))
        self.check(bdf(record(0x41, (2, 1, 7, 0), ["C0"])))
        self.check(bdf(record(0x41, (1, 8, 0, 0), ["80"] * 8)))
        self.check(bdf(record(0x41, (1, 1, 0, -2), ["80"])))
        self.check(bdf(record(0x41, (1, 1, -1, 0), ["80"])))

    def test_a_missing_or_repeated_code_point(self):
        self.check(bdf(record(0x41, (1, 1, 0, 0), ["80"])), 0x41, 0x42)
        self.check(bdf(record(0x41, (1, 1, 0, 0), ["80"]), record(0x41, (1, 1, 0, 0), ["80"])))

    def test_a_malformed_record(self):
        self.check(bdf(record(0x41, (1, 2, 0, 0), ["80"])))
        self.check(bdf(record(0x41, (1, 1, 0, 0), ["8"])))
        self.check(bdf(record(0x41, (1, 1, 0, 0), ["ZZ"])))
        self.check("STARTCHAR A\nENCODING 65\nBITMAP\n80\nENDCHAR\n")
        self.check("STARTCHAR A\nENCODING 65\nBBX 1 1 0 0\nBITMAP\n80\n")


class Release(unittest.TestCase):
    def test_a_zip_with_another_sha256_is_refused_before_it_is_read(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "not-the-release.zip")
            with open(path, "wb") as f:
                f.write(b"PK\x05\x06" + bytes(18))
            with self.assertRaises(galmuri.FontError):
                galmuri.fetch(path)

    def test_the_notice_names_the_source_the_version_and_both_sha256s(self):
        text = galmuri.notice(galmuri.BLOB_SHA256)
        for s in (galmuri.URL, galmuri.VERSION, galmuri.ZIP_SHA256, galmuri.BLOB_SHA256):
            self.assertIn(s, text)

    def test_the_committed_files_are_the_notice_and_a_blob_of_the_pinned_sha256(self):
        out = galmuri.OUT
        with open(os.path.join(out, galmuri.BLOB), "rb") as f:
            blob = f.read()
        self.assertEqual(len(blob), 11172 * 8)
        self.assertEqual(galmuri.sha256(blob), galmuri.BLOB_SHA256)
        with open(os.path.join(out, galmuri.NOTICE), "rb") as f:
            self.assertEqual(f.read(), galmuri.notice(galmuri.BLOB_SHA256).encode("utf-8"))
        with open(os.path.join(out, galmuri.OFL), "rb") as f:
            self.assertIn(b"SIL OPEN FONT LICENSE Version 1.1", f.read())


if __name__ == "__main__":
    unittest.main()
