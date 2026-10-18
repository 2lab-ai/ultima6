#!/usr/bin/env python3
"""Tests for galmuri.py (the Hangul table). Synthetic BDF records and the committed files,
no download (where a test needs the release it is handed the bytes, never the network); the
Release tests also read the pinned release zip from the cache galmuri.py keeps
($XDG_CACHE_HOME/ultima6/Galmuri-v2.40.4.zip, ~/.cache/ultima6/ without it) and are skipped
when it is not there. A run without --check fills it, for example
python3 -I -B tools/font/galmuri.py --out <a scratch directory> (--check writes nothing, the
cache included).

Release validation: with U6_FONT_REQUIRE_RELEASE=1 in the environment the Release tests fail,
instead of being skipped, when the cached release zip is not there, so a run that must check
the committed files against the release cannot pass without it.

Run: python3 -I -B tools/font/test_galmuri.py
     U6_FONT_REQUIRE_RELEASE=1 python3 -I -B tools/font/test_galmuri.py
"""
import builtins
import contextlib
import importlib.util
import io
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f"{name}.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


galmuri = load("galmuri")

# U6_FONT_REQUIRE_RELEASE=1: the Release tests fail without the cached release zip.
REQUIRE_RELEASE = os.environ.get("U6_FONT_REQUIRE_RELEASE") == "1"


def isolated(cache_home):
    """A fresh galmuri module whose cache lies under `cache_home` (its CACHE is read from
    $XDG_CACHE_HOME when the module loads)."""
    with mock.patch.dict(os.environ, {"XDG_CACHE_HOME": cache_home}):
        mod = load("galmuri")
    assert mod.CACHE.startswith(cache_home + os.sep), mod.CACHE
    return mod


def offline(mod, data):
    """Hands `mod` `data` where it would download the release, and counts the calls."""
    calls = []

    def download():
        calls.append(1)
        return data

    mod.download = download
    return calls


class Torn:
    """A file that writes half of what it is given, then raises (an interrupted write)."""

    def __init__(self, f):
        self.f = f

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.f.close()
        return False

    def write(self, data):
        self.f.write(data[: len(data) // 2])
        self.f.flush()
        raise OSError("simulated: the write stopped half-way")

    def __getattr__(self, name):
        return getattr(self.f, name)


def torn_open(file, mode="r", *args, **kwargs):
    """open() as galmuri calls it, every file opened for writing a Torn one."""
    f = builtins.open(file, mode, *args, **kwargs)
    return f if set(mode) & set("wax+") == set() else Torn(f)


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

    def test_bitmap_rows_are_hex_digits_only(self):
        # Each is two characters that int(_, 16) reads.
        for row in ["-1", "+1", " 1", "1_", "0x"]:
            with self.subTest(row=row):
                self.check(bdf(record(0x41, (1, 1, 0, 0), [row])))

    def text(self, *lines):
        return "STARTCHAR A\n" + "\n".join(lines) + "\nBITMAP\n80\nENDCHAR\n"

    def test_a_field_twice(self):
        self.check(self.text("ENCODING 65", "ENCODING 65", "BBX 1 1 0 0"))
        self.check(self.text("ENCODING 65", "BBX 1 1 0 0", "BBX 1 1 0 0"))
        self.check(self.text("ENCODING 66", "BBX 1 1 0 0", "ENCODING 65"))

    def test_missing_or_non_numeric_operands(self):
        for lines in [
            ("ENCODING", "BBX 1 1 0 0"),
            ("ENCODING x", "BBX 1 1 0 0"),
            ("ENCODING +65", "BBX 1 1 0 0"),
            ("ENCODING 6_5", "BBX 1 1 0 0"),
            ("ENCODING 65 1 2", "BBX 1 1 0 0"),
            ("ENCODING 65", "BBX"),
            ("ENCODING 65", "BBX 1 1 0"),
            ("ENCODING 65", "BBX 1 1 0 0 0"),
            ("ENCODING 65", "BBX 1 one 0 0"),
            ("ENCODING 65", "BBX 1 1 0 0.5"),
            ("ENCODING 65", "BBX -1 1 0 0"),
        ]:
            with self.subTest(lines=lines):
                self.check(self.text(*lines))
        # The two-integer form of ENCODING is a record (its first integer the code point).
        out = galmuri.table(self.text("ENCODING 65 0", "BBX 1 1 0 0"), 0x41, 0x41)
        self.assertEqual(out[6], 0x80)

    def test_an_integer_of_more_digits_than_int_reads(self):
        # int() refuses a decimal string longer than sys.get_int_max_str_digits() (4300 by
        # default) with ValueError; to the tool it is a malformed record like any other.
        big = "9" * 5000
        for lines in [
            (f"ENCODING {big}", "BBX 1 1 0 0"),
            (f"ENCODING -{big}", "BBX 1 1 0 0"),
            ("ENCODING 65", f"BBX 1 1 0 {big}"),
            ("ENCODING 65", f"BBX {big} 1 0 0"),
        ]:
            with self.subTest(lines=[w[:16] for w in lines]):
                self.check(self.text(*lines))


def snapshot(path):
    """Every file under `path`: its bytes and its modification time."""
    out = {}
    for d, _, names in os.walk(path):
        for name in names:
            p = os.path.join(d, name)
            with open(p, "rb") as f:
                out[os.path.relpath(p, path)] = (f.read(), os.stat(p).st_mtime_ns)
    return out


def quiet(argv, mod=galmuri):
    """mod.main(argv)'s exit code, its output kept from the test log."""
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return mod.main(argv)


@contextlib.contextmanager
def network(data):
    """While it lasts, every urllib download gives `data` instead of reaching the network; the
    list it yields collects the URLs asked for."""
    calls = []

    def urlopen(url, *args, **kwargs):
        calls.append(url)
        return io.BytesIO(data)

    def urlretrieve(url, filename=None, *args, **kwargs):
        calls.append(url)
        with builtins.open(filename, "wb") as f:
            f.write(data)
        return filename, None

    with mock.patch("urllib.request.urlopen", urlopen), mock.patch(
        "urllib.request.urlretrieve", urlretrieve
    ):
        yield calls


def synthetic_zip(bdf_text, licence):
    """A zip holding `bdf_text` and `licence` under the release's member names."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr(galmuri.BDF_MEMBER, bdf_text)
        z.writestr(galmuri.LICENSE_MEMBER, licence)
    return buf.getvalue()


class Main(unittest.TestCase):
    def test_a_malformed_font_is_reported_and_not_raised(self):
        licence = b"a synthetic licence\n"
        big = "9" * 5000
        record = f"STARTCHAR A\nENCODING {big}\nBBX 1 1 0 0\nBITMAP\n80\nENDCHAR\n"
        data = synthetic_zip(record, licence)
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "synthetic.zip")
            with open(path, "wb") as f:
                f.write(data)
            out = os.path.join(tmp, "out")
            for check in ([], ["--check"]):
                err = io.StringIO()
                pins = (
                    mock.patch.object(galmuri, "ZIP_SHA256", galmuri.sha256(data)),
                    mock.patch.object(galmuri, "LICENSE_SHA256", galmuri.sha256(licence)),
                )
                with pins[0], pins[1], contextlib.redirect_stdout(io.StringIO()):
                    with contextlib.redirect_stderr(err):
                        code = galmuri.main(["--zip", path, "--out", out] + check)
                with self.subTest(check=check):
                    self.assertEqual(code, 1)
                    self.assertRegex(err.getvalue(), r"^galmuri: line 2: ENCODING ")
                    self.assertNotIn("Traceback", err.getvalue())
                    self.assertFalse(os.path.exists(out), "nothing written")


class Cache(unittest.TestCase):
    """The release zip's cache: published whole or not at all, and left alone by --check."""

    DATA = b"synthetic release bytes\n" * 64

    def pinned(self, mod=galmuri):
        """`mod` with DATA as the pinned release zip."""
        return mock.patch.object(mod, "ZIP_SHA256", galmuri.sha256(self.DATA))

    def test_an_interrupted_publication_leaves_no_file_and_a_later_fetch_fills_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = os.path.join(tmp, "ultima6", "release.zip")
            with self.pinned(), network(self.DATA) as calls:
                with mock.patch.object(galmuri, "open", torn_open, create=True):
                    with self.assertRaises(OSError):
                        galmuri.fetch(None, cache=cache)
                self.assertFalse(os.path.exists(cache), "a partial file at the cache path")
                self.assertEqual(snapshot(tmp), {}, "nothing left beside it")
                # The next fetch downloads again and publishes the whole zip.
                self.assertEqual(galmuri.fetch(None, cache=cache), self.DATA)
                self.assertEqual(len(calls), 2)
            self.assertEqual(snapshot(tmp).keys(), {os.path.join("ultima6", "release.zip")})
            with open(cache, "rb") as f:
                self.assertEqual(f.read(), self.DATA)
            # Then it is read from the cache, with no download.
            with self.pinned(), network(b"") as calls:
                self.assertEqual(galmuri.fetch(None, cache=cache), self.DATA)
            self.assertEqual(calls, [])

    def test_check_leaves_a_cold_cache_cold(self):
        # An isolated cold cache ($XDG_CACHE_HOME a new directory) and no --zip: --check takes
        # the release from the network (here: DATA, no network) and writes nothing anywhere.
        with tempfile.TemporaryDirectory() as home, tempfile.TemporaryDirectory() as out:
            mod = isolated(home)
            with self.pinned(mod), network(self.DATA) as calls:
                code = quiet(["--check", "--out", out], mod)
            self.assertEqual(calls, [mod.URL], "the cold cache was not read")
            self.assertEqual(code, 1, "DATA is not a zip")
            self.assertEqual(os.listdir(home), [], "the cache directory is untouched")
            self.assertEqual(os.listdir(out), [])


class ReleaseMode(unittest.TestCase):
    """U6_FONT_REQUIRE_RELEASE=1 turns the Release tests' skip into a failure."""

    def release_tests(self, require):
        """The Release tests run on their own with a cold cache: (exit code, their log)."""
        env = {k: v for k, v in os.environ.items() if k != "U6_FONT_REQUIRE_RELEASE"}
        if require:
            env["U6_FONT_REQUIRE_RELEASE"] = "1"
        with tempfile.TemporaryDirectory() as home:
            env["XDG_CACHE_HOME"] = home
            r = subprocess.run(
                [sys.executable, "-I", "-B", os.path.join(HERE, "test_galmuri.py"), "Release"],
                env=env,
                capture_output=True,
                text=True,
                timeout=120,
            )
            self.assertEqual(os.listdir(home), [])
        return r.returncode, r.stderr

    def test_without_the_release_zip_the_release_tests_skip_or_in_release_mode_fail(self):
        code, log = self.release_tests(require=False)
        self.assertEqual(code, 0, log)
        self.assertRegex(log, r"OK \(skipped=\d+\)")
        code, log = self.release_tests(require=True)
        self.assertNotEqual(code, 0, log)
        self.assertRegex(log, r"FAILED \(failures=\d+\)")
        self.assertIn("U6_FONT_REQUIRE_RELEASE=1 and no cached release zip", log)


class Committed(unittest.TestCase):
    def test_a_zip_with_another_sha256_is_refused_before_it_is_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "not-the-release.zip")
            with open(path, "wb") as f:
                f.write(b"PK\x05\x06" + bytes(18))
            with self.assertRaises(galmuri.FontError):
                galmuri.fetch(path)
            # A cached file is checked the same way, and a bad one is not replaced.
            with self.assertRaises(galmuri.FontError):
                galmuri.fetch(None, cache=path)
            with open(path, "rb") as f:
                self.assertEqual(f.read(), b"PK\x05\x06" + bytes(18))

    def test_ofl_txt_is_the_release_licence_byte_for_byte(self):
        with open(os.path.join(galmuri.OUT, galmuri.OFL), "rb") as f:
            self.assertEqual(galmuri.sha256(f.read()), galmuri.LICENSE_SHA256)

    def test_the_licence_reserves_no_font_name(self):
        # OFL 1.1: Reserved Font Names are the ones named as such after the copyright
        # statement, before the licence text; this release names none.
        with open(os.path.join(galmuri.OUT, galmuri.OFL), encoding="utf-8") as f:
            text = f.read()
        head = text.split("SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007")[0]
        self.assertIn("This font software is licensed under the SIL Open Font License", head)
        self.assertNotIn("Reserved Font Name", head)
        self.assertNotIn("Reserved", head)

    def test_the_notice_names_the_source_the_version_and_both_sha256s(self):
        text = galmuri.notice(galmuri.BLOB_SHA256)
        for s in (galmuri.URL, galmuri.VERSION, galmuri.ZIP_SHA256, galmuri.BLOB_SHA256):
            self.assertIn(s, text)

    def test_the_committed_files_are_the_notice_and_a_blob_of_the_pinned_sha256(self):
        self.assertEqual(sorted(os.listdir(galmuri.OUT)), sorted([galmuri.BLOB, galmuri.OFL, galmuri.NOTICE]))
        out = galmuri.OUT
        with open(os.path.join(out, galmuri.BLOB), "rb") as f:
            blob = f.read()
        self.assertEqual(len(blob), 11172 * 8)
        self.assertEqual(galmuri.sha256(blob), galmuri.BLOB_SHA256)
        with open(os.path.join(out, galmuri.NOTICE), "rb") as f:
            self.assertEqual(f.read(), galmuri.notice(galmuri.BLOB_SHA256).encode("utf-8"))
        with open(os.path.join(out, galmuri.OFL), "rb") as f:
            self.assertIn(b"SIL OPEN FONT LICENSE Version 1.1", f.read())


class Release(unittest.TestCase):
    """The pinned release zip from the cache: build, write and --check against it. Skipped
    without it, failed without it under U6_FONT_REQUIRE_RELEASE=1."""

    def setUp(self):
        if not os.path.exists(galmuri.CACHE):
            if REQUIRE_RELEASE:
                self.fail(f"U6_FONT_REQUIRE_RELEASE=1 and no cached release zip at {galmuri.CACHE}")
            self.skipTest(f"no cached release zip at {galmuri.CACHE}")

    def test_build_gives_the_committed_files(self):
        with open(galmuri.CACHE, "rb") as f:
            blob, licence, notice = galmuri.build(f.read())
        for name, data in [(galmuri.BLOB, blob), (galmuri.OFL, licence), (galmuri.NOTICE, notice)]:
            with open(os.path.join(galmuri.OUT, name), "rb") as f:
                self.assertEqual(f.read(), data, name)
        with zipfile.ZipFile(galmuri.CACHE) as z:
            self.assertEqual(licence, z.read(galmuri.LICENSE_MEMBER), "OFL.txt is LICENSE.txt")

    def test_check_passes_on_the_committed_files_and_writes_nothing(self):
        before = snapshot(galmuri.OUT)
        self.assertEqual(quiet(["--zip", galmuri.CACHE, "--check"]), 0)
        self.assertEqual(snapshot(galmuri.OUT), before)

    def test_check_fails_on_other_files_and_writes_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            # An empty directory: fails, stays empty.
            self.assertEqual(quiet(["--zip", galmuri.CACHE, "--check", "--out", tmp]), 1)
            self.assertEqual(snapshot(tmp), {})
            # The committed files with one bit of the blob changed, then with the NOTICE gone.
            for name in (galmuri.BLOB, galmuri.OFL, galmuri.NOTICE):
                shutil.copy(os.path.join(galmuri.OUT, name), tmp)
            with open(os.path.join(tmp, galmuri.BLOB), "r+b") as f:
                f.seek(100)
                b = f.read(1)
                f.seek(100)
                f.write(bytes([b[0] ^ 1]))
            before = snapshot(tmp)
            self.assertEqual(quiet(["--zip", galmuri.CACHE, "--check", "--out", tmp]), 1)
            self.assertEqual(snapshot(tmp), before)
            os.remove(os.path.join(tmp, galmuri.NOTICE))
            shutil.copy(os.path.join(galmuri.OUT, galmuri.BLOB), tmp)
            before = snapshot(tmp)
            self.assertEqual(quiet(["--zip", galmuri.CACHE, "--check", "--out", tmp]), 1)
            self.assertEqual(snapshot(tmp), before)

    def test_writing_gives_the_committed_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "fonts")
            self.assertEqual(quiet(["--zip", galmuri.CACHE, "--out", out]), 0)
            got = {k: v[0] for k, v in snapshot(out).items()}
            want = {k: v[0] for k, v in snapshot(galmuri.OUT).items()}
            self.assertEqual(got, want)
            self.assertEqual(quiet(["--zip", galmuri.CACHE, "--check", "--out", out]), 0)

    def test_check_with_a_cold_cache_compares_and_leaves_the_cache_cold(self):
        with open(galmuri.CACHE, "rb") as f:
            data = f.read()
        before = snapshot(galmuri.OUT)
        with tempfile.TemporaryDirectory() as home:
            mod = isolated(home)
            with network(data) as calls:
                self.assertEqual(quiet(["--check"], mod), 0)
            self.assertEqual(calls, [mod.URL])
            self.assertEqual(os.listdir(home), [], "the cache directory is untouched")
        self.assertEqual(snapshot(galmuri.OUT), before)

    def test_a_run_without_check_fills_a_cold_cache(self):
        with open(galmuri.CACHE, "rb") as f:
            data = f.read()
        with tempfile.TemporaryDirectory() as home, tempfile.TemporaryDirectory() as tmp:
            mod = isolated(home)
            with network(data) as calls:
                self.assertEqual(quiet(["--out", os.path.join(tmp, "fonts")], mod), 0)
            self.assertEqual(calls, [mod.URL])
            with open(mod.CACHE, "rb") as f:
                self.assertEqual(f.read(), data)
            cached = snapshot(os.path.dirname(mod.CACHE))
            self.assertEqual(cached.keys(), {os.path.basename(mod.CACHE)}, "the zip only")

    def test_a_damaged_zip_is_refused(self):
        with open(galmuri.CACHE, "rb") as f:
            data = bytearray(f.read())
        data[len(data) // 2] ^= 1
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "damaged.zip")
            with open(path, "wb") as f:
                f.write(data)
            out = os.path.join(tmp, "out")
            self.assertEqual(quiet(["--zip", path, "--out", out]), 1)
            self.assertFalse(os.path.exists(out), "nothing written")


if __name__ == "__main__":
    unittest.main()
