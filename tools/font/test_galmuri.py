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

Run (Python 3.11 or later, as galmuri.py needs; an older one from 3.6 exits at once, saying so,
and one before 3.6 cannot parse this file):
     python3 -I -B tools/font/test_galmuri.py
     U6_FONT_REQUIRE_RELEASE=1 python3 -I -B tools/font/test_galmuri.py
"""
import builtins
import contextlib
import errno
import importlib.util
import io
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

if sys.version_info < (3, 11):
    sys.exit("test_galmuri: needs Python 3.11 or later")

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


class FailingFlush(Torn):
    """A file that writes all it is given, then fails to flush it."""

    def write(self, data):
        return self.f.write(data)

    def flush(self):
        raise OSError("simulated: the flush failed")


class FailingClose(Torn):
    """A file whose write (when `write_fails`) and close raise; the file is closed all the
    same."""

    write_fails = True

    def write(self, data):
        if self.write_fails:
            raise OSError("simulated: the write failed")
        return self.f.write(data)

    def close(self):
        self.f.close()
        raise OSError("simulated: the close failed")


@contextlib.contextmanager
def failing_close(write_fails):
    """While it lasts, galmuri.publish's temporary file fails to close (after a failed write
    when `write_fails`)."""

    def opener(file, mode="r", *args, **kwargs):
        f = FailingClose(builtins.open(file, mode, *args, **kwargs))
        f.write_fails = write_fails
        return f

    with mock.patch.object(galmuri, "open", opener, create=True):
        yield


@contextlib.contextmanager
def fail_at(step):
    """While it lasts, galmuri.publish fails at `step`: "flush", "fsync" or "replace"."""
    if step == "flush":

        def opener(file, mode="r", *args, **kwargs):
            return FailingFlush(builtins.open(file, mode, *args, **kwargs))

        patch = mock.patch.object(galmuri, "open", opener, create=True)
    else:
        failed = OSError(f"simulated: the {step} failed")
        patch = mock.patch.object(galmuri.os, step, side_effect=failed)
    with patch:
        yield


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


class Diagnostics(unittest.TestCase):
    """main reports an error on one line, whatever the paths in it hold."""

    def test_a_path_with_line_breaks_is_named_with_their_escapes_on_the_one_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "a\nb\rc.zip")
            with open(path, "wb") as f:
                f.write(b"not the release zip\n")
            err = io.StringIO()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
                code = galmuri.main(["--zip", path, "--out", os.path.join(tmp, "out")])
            self.assertFalse(os.path.exists(os.path.join(tmp, "out")), "nothing written")
        self.assertEqual(code, 1)
        self.assertEqual(len(err.getvalue().splitlines()), 1, err.getvalue())
        self.assertTrue(err.getvalue().endswith("\n"))
        self.assertIn(f"{tmp}/a\\nb\\rc.zip: sha256 ", err.getvalue())

    def test_every_line_break_in_an_error_and_its_notes_is_shown_as_its_escape(self):
        breaks = "\n\r\v\f\x1c\x1d\x1e\x85\u2028\u2029"
        e = galmuri.FontError(f"release zip /t{breaks}z: sha256 0, pinned 1")
        e.add_note(f"the directory /t{breaks}d made for it is left: refused")
        got = galmuri.message(e)
        shown = "\\n\\r\\x0b\\x0c\\x1c\\x1d\\x1e\\x85\\u2028\\u2029"
        self.assertEqual(
            got,
            f"release zip /t{shown}z: sha256 0, pinned 1; "
            f"the directory /t{shown}d made for it is left: refused",
        )
        self.assertEqual(got.splitlines(), [got])

    def test_a_line_break_and_the_text_of_its_escape_are_shown_apart(self):
        # A newline, and a backslash then `n`: the backslash is escaped too, so they differ.
        self.assertEqual(galmuri.one_line("/t/a\nb"), "/t/a\\nb")
        self.assertEqual(galmuri.one_line("/t/a\\nb"), "/t/a\\\\nb")
        self.assertEqual(galmuri.one_line("/t/a\\b"), "/t/a\\\\b")
        for path, shown in (("/t/a\nb", "/t/a\\nb"), ("/t/a\\nb", "/t/a\\\\nb")):
            e = galmuri.FontError(f"release zip {path}: sha256 0, pinned 1")
            e.add_note(f"the directory {path} made for it is left: refused")
            with self.subTest(path=path):
                self.assertEqual(
                    galmuri.message(e),
                    f"release zip {shown}: sha256 0, pinned 1; "
                    f"the directory {shown} made for it is left: refused",
                )


class Arguments(unittest.TestCase):
    """argparse's own output: an error on one line whatever the arguments hold, and the program
    named as the tool, not as argv[0] (which here holds line breaks)."""

    def run_main(self, argv):
        """galmuri.main(argv), argparse's exit caught: (exit code, stdout, stderr)."""
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "argv", ["/t/x\ny\rz.py"]):
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                try:
                    code = galmuri.main(argv)
                except SystemExit as e:
                    code = e.code
        return code, out.getvalue(), err.getvalue()

    def test_a_parser_error_from_an_argument_with_line_breaks_is_one_line(self):
        for argv, shown in ((["a\nb\rc"], "a\\nb\\rc"), (["--zap\n\rx"], "--zap\\n\\rx")):
            code, out, err = self.run_main(argv)
            with self.subTest(argv=argv):
                self.assertEqual(code, 2)
                self.assertEqual(out, "")
                self.assertEqual(err, f"galmuri: unrecognized arguments: {shown} (see --help)\n")

    def test_help_names_the_tool_not_argv_0(self):
        code, out, err = self.run_main(["--help"])
        self.assertEqual((code, err), (0, ""))
        self.assertIn("galmuri.py", out.splitlines()[0])
        self.assertNotIn("z.py", out)


@contextlib.contextmanager
def umask(mask):
    """The process umask `mask` while it lasts."""
    old = os.umask(mask)
    try:
        yield
    finally:
        os.umask(old)


def mode(path):
    return stat.S_IMODE(os.stat(path).st_mode)


class Publish(unittest.TestCase):
    """publish(): one file whole or not at all, with the mode open() would give it."""

    def test_a_new_file_gets_0o666_less_the_umask_and_a_replaced_one_keeps_its_mode(self):
        with tempfile.TemporaryDirectory() as tmp:
            for mask, want in ((0o022, 0o644), (0o027, 0o640), (0o002, 0o664)):
                path = os.path.join(tmp, f"new-{mask:03o}")
                with umask(mask):
                    galmuri.publish(path, b"new bytes")
                self.assertEqual(mode(path), want, f"umask {mask:03o}")
            path = os.path.join(tmp, "older")
            with open(path, "wb") as f:
                f.write(b"older bytes")
            for want in (0o600, 0o640, 0o755):
                os.chmod(path, want)
                with umask(0o077):
                    galmuri.publish(path, b"new bytes")
                self.assertEqual(mode(path), want, f"replacing a {want:03o} file")
                with open(path, "rb") as f:
                    self.assertEqual(f.read(), b"new bytes")

    def older(self, tmp):
        """A file of older bytes in `tmp`: its path."""
        path = os.path.join(tmp, "older")
        with open(path, "wb") as f:
            f.write(b"older bytes\n")
        os.chmod(path, 0o640)
        return path

    def test_a_failure_at_flush_fsync_or_replace_leaves_the_file_there_as_it_was(self):
        for step in ("flush", "fsync", "replace"):
            with self.subTest(step=step), tempfile.TemporaryDirectory() as tmp:
                path = self.older(tmp)
                before = snapshot(tmp)
                with fail_at(step), self.assertRaisesRegex(OSError, f"the {step} failed"):
                    galmuri.publish(path, b"new bytes\n")
                self.assertEqual(snapshot(tmp), before, "its bytes and mtime")
                self.assertEqual(os.listdir(tmp), ["older"], "no temporary file left")
                self.assertEqual(mode(path), 0o640)

    def test_a_failure_removes_the_directories_made_for_the_file_and_no_other(self):
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "kept"))
            for step in ("flush", "fsync", "replace"):
                with self.subTest(step=step), fail_at(step), self.assertRaises(OSError):
                    galmuri.publish(os.path.join(tmp, "kept", "a", "b", "file"), b"new bytes\n")
                self.assertEqual(sorted(os.listdir(tmp)), ["kept"])
                self.assertEqual(os.listdir(os.path.join(tmp, "kept")), [], step)
            # A file made beside the temporary one keeps its directory, and the one around it.
            other = os.path.join(tmp, "new", "other")

            def beside(*args, **kwargs):
                with open(other, "wb") as f:
                    f.write(b"another's file\n")
                raise OSError("simulated: the fsync failed")

            with mock.patch.object(galmuri.os, "fsync", beside):
                with self.assertRaises(OSError) as raised:
                    galmuri.publish(os.path.join(tmp, "new", "file"), b"new bytes\n")
            self.assertEqual(os.listdir(os.path.join(tmp, "new")), ["other"])
            self.assertFalse(getattr(raised.exception, "__notes__", None), "not empty: no note")

    def test_a_directory_that_cannot_be_removed_is_a_note_on_the_first_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            made = os.path.join(tmp, "new")
            refused = PermissionError(errno.EACCES, "simulated: the rmdir was refused")
            with fail_at("fsync"), mock.patch.object(galmuri.os, "rmdir", side_effect=refused):
                with self.assertRaisesRegex(OSError, "the fsync failed") as raised:
                    galmuri.publish(os.path.join(made, "file"), b"new bytes\n")
            self.assertEqual(os.listdir(made), [], "the temporary file is removed")
            notes = raised.exception.__notes__
            self.assertEqual(len(notes), 1)
            self.assertIn(f"the directory {made} made for it is left", notes[0])
            self.assertIn("simulated: the rmdir was refused", notes[0])

    def test_a_directory_left_stops_the_walk_out_with_at_most_one_note(self):
        # publish made "a" and "a/b" and then failed, and removing "a/b" fails: "a" holds it, so
        # the walk stops there, one rmdir. An unexpected failure is one note; a directory not
        # empty (ENOTEMPTY, or EEXIST, which POSIX allows for it too) adds none.
        for err, noted in ((errno.EACCES, True), (errno.ENOTEMPTY, False), (errno.EEXIST, False)):
            with self.subTest(errno=errno.errorcode[err]), tempfile.TemporaryDirectory() as tmp:
                inner = os.path.join(tmp, "a", "b")
                rmdir = mock.Mock(side_effect=OSError(err, f"simulated: {os.strerror(err)}"))
                with fail_at("fsync"), mock.patch.object(galmuri.os, "rmdir", rmdir):
                    with self.assertRaisesRegex(OSError, "the fsync failed") as raised:
                        galmuri.publish(os.path.join(inner, "file"), b"new bytes\n")
                self.assertEqual(rmdir.call_args_list, [mock.call(inner)])
                self.assertEqual(os.listdir(inner), [], "the temporary file is removed")
                notes = getattr(raised.exception, "__notes__", [])
                if noted:
                    self.assertEqual(len(notes), 1, notes)
                    self.assertIn(f"the directory {inner} made for it is left", notes[0])
                else:
                    self.assertEqual(notes, [])

    def test_a_close_that_fails_after_a_failed_write_does_not_hide_the_write_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self.older(tmp)
            before = snapshot(tmp)
            with failing_close(True), self.assertRaisesRegex(OSError, "write failed") as raised:
                galmuri.publish(path, b"new bytes\n")
            self.assertEqual(len(raised.exception.__notes__), 1)
            self.assertRegex(raised.exception.__notes__[0], r"^closing \S+\.part failed too: ")
            self.assertIn("simulated: the close failed", raised.exception.__notes__[0])
            self.assertEqual(snapshot(tmp), before, "the file as it was, no temporary file")
            # A close that fails on its own is the error.
            with failing_close(False), self.assertRaisesRegex(OSError, "the close failed") as raised:
                galmuri.publish(path, b"new bytes\n")
            self.assertFalse(getattr(raised.exception, "__notes__", None))
            self.assertEqual(snapshot(tmp), before)

    def test_a_directory_another_process_makes_meanwhile_is_not_removed(self):
        # Between publish's look for missing directories and its mkdir of "a", another process
        # makes "a": publish makes only "b", and when the write fails it removes "b" only.
        with tempfile.TemporaryDirectory() as tmp:
            outer, inner = os.path.join(tmp, "a"), os.path.join(tmp, "a", "b")
            mkdir = os.mkdir

            def raced(d, *args, **kwargs):
                if d == outer and not os.path.exists(outer):
                    mkdir(outer)
                return mkdir(d, *args, **kwargs)

            with mock.patch.object(galmuri.os, "mkdir", raced), fail_at("fsync"):
                with self.assertRaisesRegex(OSError, "the fsync failed"):
                    galmuri.publish(os.path.join(inner, "file"), b"new bytes\n")
            self.assertEqual(os.listdir(tmp), ["a"], "another's directory stays")
            self.assertEqual(os.listdir(outer), [], "the one publish made is removed")

    def test_a_temporary_file_that_cannot_be_removed_does_not_hide_the_first_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self.older(tmp)
            refused = PermissionError("simulated: the unlink was refused")
            with fail_at("replace"), mock.patch.object(galmuri.os, "unlink", side_effect=refused):
                with self.assertRaisesRegex(OSError, "simulated: the replace failed") as raised:
                    galmuri.publish(path, b"new bytes\n")
            left = [name for name in os.listdir(tmp) if name != "older"]
            self.assertEqual(len(left), 1)
            self.assertRegex(left[0], r"^\.older\..*\.part$")
            notes = raised.exception.__notes__
            self.assertEqual(len(notes), 1)
            self.assertIn(left[0], notes[0])
            self.assertIn("simulated: the unlink was refused", notes[0])
            self.assertEqual(
                galmuri.message(raised.exception),
                f"simulated: the replace failed; {notes[0]}",
            )
            with open(path, "rb") as f:
                self.assertEqual(f.read(), b"older bytes\n")


class Python(unittest.TestCase):
    def test_an_older_python_is_told_so_on_one_line_before_anything_is_done(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "out")
            # From 3.6, which parses the tool (an older Python stops at its f-strings).
            for version in ((3, 10, 14, "final", 0), (3, 9, 6, "final", 0), (3, 6, 15, "final", 0)):
                err = io.StringIO()
                with mock.patch.object(galmuri.sys, "version_info", version), mock.patch.object(
                    galmuri, "fetch", side_effect=AssertionError("fetched")
                ), contextlib.redirect_stderr(err):
                    code = galmuri.main(["--out", out])
                with self.subTest(version=version[:2]):
                    self.assertEqual(code, 1)
                    have = f"{version[0]}.{version[1]}"
                    want = f"galmuri: needs Python 3.11 or later, this is {have}\n"
                    self.assertEqual(err.getvalue(), want)
                    self.assertFalse(os.path.exists(out), "nothing written")
            # 3.11 itself passes the check: the release is fetched and built, and --check of an
            # empty directory reports the files that differ.
            os.makedirs(out)
            err = io.StringIO()
            with contextlib.ExitStack() as stack:
                stack.enter_context(mock.patch.object(galmuri.sys, "version_info", (3, 11, 0)))
                fetch = stack.enter_context(mock.patch.object(galmuri, "fetch", return_value=b"zip"))
                files = (b"a", b"b", b"c")
                build = stack.enter_context(mock.patch.object(galmuri, "build", return_value=files))
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                stack.enter_context(contextlib.redirect_stderr(err))
                code = galmuri.main(["--check", "--out", out])
            self.assertEqual(code, 1)
            fetch.assert_called_once()
            build.assert_called_once_with(b"zip")
            names = ", ".join((galmuri.BLOB, galmuri.OFL, galmuri.NOTICE))
            self.assertEqual(err.getvalue(), f"galmuri: differs from the release: {names}\n")

    def test_a_real_older_python_imports_the_tool_and_is_told_so(self):
        # /usr/bin/python3 where it is older than 3.11 (macOS ships 3.9): the module imports and
        # main refuses on one line, exit status 1.
        old = "/usr/bin/python3"
        if not os.path.exists(old):
            self.skipTest(f"{old} is not there")
        probe = [old, "-I", "-c", "import sys; print(*sys.version_info[:2], sep='.')"]
        have = subprocess.run(probe, capture_output=True, text=True, timeout=60).stdout.strip()
        if tuple(int(v) for v in have.split(".")) >= (3, 11):
            self.skipTest(f"{old} is {have}, not older than 3.11")
        tool = os.path.join(HERE, "galmuri.py")
        load = (
            "import importlib.util; "
            f"spec = importlib.util.spec_from_file_location('galmuri', {tool!r}); "
            "m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); print(m.PYTHON)"
        )
        r = subprocess.run([old, "-I", "-B", "-c", load], capture_output=True, text=True, timeout=60)
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "(3, 11)\n", ""), "it imports")
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "out")
            r = subprocess.run(
                [old, "-I", "-B", tool, "--out", out], capture_output=True, text=True, timeout=60
            )
            self.assertEqual(r.returncode, 1)
            self.assertEqual(r.stderr, f"galmuri: needs Python 3.11 or later, this is {have}\n")
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
                self.assertEqual(os.listdir(tmp), [], "no directory made for it left")
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

    def test_a_cache_that_cannot_be_looked_at_is_reported_not_downloaded_over(self):
        if os.geteuid() == 0:
            self.skipTest("as root every directory can be searched")
        with tempfile.TemporaryDirectory() as home, tempfile.TemporaryDirectory() as out:
            mod = isolated(home)
            folder = os.path.dirname(mod.CACHE)
            os.makedirs(folder)
            os.chmod(folder, 0)
            err = io.StringIO()
            try:
                with self.pinned(mod), network(self.DATA) as calls:
                    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
                        code = mod.main(["--check", "--out", out])
            finally:
                os.chmod(folder, 0o755)
            self.assertEqual(code, 1)
            self.assertRegex(err.getvalue(), r"^galmuri: \[Errno 13\] Permission denied: \S+\n$")
            self.assertEqual(calls, [], "no download")

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


class Output(unittest.TestCase):
    """The files written to --out: each published whole or not at all, as the cache is."""

    ZIP = b"synthetic release zip\n" * 32
    FILES = (b"\x5a" * 512, b"a synthetic licence\n", b"a synthetic notice\n")

    def main(self, argv, torn=False, stdout=None):
        """galmuri.main(argv) with ZIP as the pinned release and FILES what it builds, its
        files written half and then stopped when `torn`, what it prints to `stdout` (a
        StringIO) when given: (exit code, stderr)."""
        err = io.StringIO()
        with contextlib.ExitStack() as stack:
            stack.enter_context(mock.patch.object(galmuri, "ZIP_SHA256", galmuri.sha256(self.ZIP)))
            stack.enter_context(mock.patch.object(galmuri, "build", lambda data: self.FILES))
            if torn:
                stack.enter_context(mock.patch.object(galmuri, "open", torn_open, create=True))
            stack.enter_context(contextlib.redirect_stdout(stdout or io.StringIO()))
            stack.enter_context(contextlib.redirect_stderr(err))
            code = galmuri.main(argv)
        return code, err.getvalue()

    def test_an_out_with_line_breaks_or_the_text_of_their_escapes_is_named_apart_on_one_line(self):
        names = ", ".join((galmuri.BLOB, galmuri.OFL, galmuri.NOTICE))
        sha = galmuri.BLOB_SHA256
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "synthetic.zip")
            with open(path, "wb") as f:
                f.write(self.ZIP)
            # An --out holding a newline and a carriage return, and one holding the text of their
            # escapes (a backslash, then `n`; a backslash, then `r`): each named on one line,
            # and the two apart.
            for leaf, shown in (("a\nb\rc", "a\\nb\\rc"), ("a\\nb\\rc", "a\\\\nb\\\\rc")):
                out = os.path.join(tmp, leaf)
                # Written, then checked: each run's one message names --out with the escapes.
                for argv, said in (
                    ([], f"galmuri: wrote {names} to {tmp}/{shown} (blob sha256 {sha})\n"),
                    (["--check"], f"galmuri: {tmp}/{shown} matches {galmuri.VERSION}\n"),
                ):
                    got = io.StringIO()
                    code, err = self.main(["--zip", path, "--out", out, *argv], stdout=got)
                    with self.subTest(out=leaf, argv=argv):
                        self.assertEqual((code, err), (0, ""))
                        self.assertEqual(got.getvalue(), said)

    def test_an_interrupted_write_leaves_the_files_as_they_were_and_a_later_run_writes_them(self):
        names = (galmuri.BLOB, galmuri.OFL, galmuri.NOTICE)
        self.enterContext(umask(0o022))
        for existing in (True, False):
            with self.subTest(existing=existing), tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, "synthetic.zip")
                with open(path, "wb") as f:
                    f.write(self.ZIP)
                out = os.path.join(tmp, "fonts")
                if existing:
                    os.makedirs(out)
                    for name in names:
                        with open(os.path.join(out, name), "wb") as f:
                            f.write(b"an older " + name.encode() + b"\n")
                before = snapshot(out)
                code, err = self.main(["--zip", path, "--out", out], torn=True)
                self.assertEqual(code, 1)
                self.assertRegex(err, r"^galmuri: .*half-way")
                self.assertNotIn("Traceback", err)
                self.assertEqual(snapshot(out), before, "no partial file, nothing beside them")
                # A new --out is not left behind, even empty (snapshot() cannot tell).
                self.assertEqual(os.path.exists(out), existing)
                # A run that is not stopped writes all three whole, with the mode open() gives
                # them (the older files' where it replaced them; 0644 under umask 022 either way).
                self.assertEqual(self.main(["--zip", path, "--out", out])[0], 0)
                got = {k: v[0] for k, v in snapshot(out).items()}
                self.assertEqual(got, dict(zip(names, self.FILES)))
                for name in names:
                    self.assertEqual(mode(os.path.join(out, name)), 0o644, name)
                self.assertEqual(self.main(["--zip", path, "--out", out, "--check"])[0], 0)


    def older_files(self, tmp):
        """--out in `tmp` holding older files of the three names: (its path, the zip's path)."""
        path = os.path.join(tmp, "synthetic.zip")
        with open(path, "wb") as f:
            f.write(self.ZIP)
        out = os.path.join(tmp, "fonts")
        os.makedirs(out)
        for name in (galmuri.BLOB, galmuri.OFL, galmuri.NOTICE):
            with open(os.path.join(out, name), "wb") as f:
                f.write(b"an older " + name.encode() + b"\n")
        return out, path

    def test_a_failure_on_the_second_file_leaves_the_first_new_and_the_others_as_they_were(self):
        with tempfile.TemporaryDirectory() as tmp:
            out, path = self.older_files(tmp)
            before = snapshot(out)
            replace = os.replace

            def second_fails(src, dst):
                if os.path.basename(dst) == galmuri.OFL:
                    raise OSError("simulated: the replace failed")
                return replace(src, dst)

            with mock.patch.object(galmuri.os, "replace", second_fails):
                code, err = self.main(["--zip", path, "--out", out])
            self.assertEqual(code, 1)
            self.assertEqual(err, "galmuri: simulated: the replace failed\n")
            after = snapshot(out)
            self.assertEqual(sorted(after), sorted(before), "no temporary file left")
            self.assertEqual(after[galmuri.BLOB][0], self.FILES[0], "the first is new")
            for name in (galmuri.OFL, galmuri.NOTICE):
                self.assertEqual(after[name], before[name], name)
            # --check fails on that mix.
            self.assertEqual(self.main(["--zip", path, "--out", out, "--check"])[0], 1)
        # Into a new --out: the first file is published, so it and its directory stay.
        with tempfile.TemporaryDirectory() as tmp:
            _, path = self.older_files(tmp)
            out = os.path.join(tmp, "new")
            with mock.patch.object(galmuri.os, "replace", second_fails):
                self.assertEqual(self.main(["--zip", path, "--out", out])[0], 1)
            self.assertEqual(sorted(os.listdir(out)), [galmuri.BLOB])

    def test_a_write_and_a_close_that_fail_are_reported_write_first(self):
        with tempfile.TemporaryDirectory() as tmp:
            out, path = self.older_files(tmp)
            before = snapshot(out)
            with failing_close(True):
                code, err = self.main(["--zip", path, "--out", out])
            self.assertEqual(code, 1)
            self.assertRegex(
                err,
                r"^galmuri: simulated: the write failed; closing \S+\.part failed too: "
                r"simulated: the close failed\n$",
            )
            self.assertEqual(snapshot(out), before)

    def test_check_reports_a_file_it_cannot_read_on_one_line(self):
        # A file of --out that is a directory, and one that cannot be read: --check fails with
        # one line, no traceback, and leaves --out as it was.
        def directory(path):
            os.remove(path)
            os.mkdir(path)

        def unreadable(path):
            os.chmod(path, 0)

        cases = [(directory, galmuri.NOTICE, "Is a directory")]
        if os.geteuid() != 0:
            cases.append((unreadable, galmuri.OFL, "Permission denied"))
        for spoil, name, reason in cases:
            with self.subTest(case=spoil.__name__), tempfile.TemporaryDirectory() as tmp:
                out, path = self.older_files(tmp)
                target = os.path.join(out, name)
                spoil(target)
                names = sorted(os.listdir(out))
                try:
                    code, err = self.main(["--zip", path, "--out", out, "--check"])
                finally:
                    os.chmod(target, 0o755)
                self.assertEqual(code, 1)
                self.assertRegex(err, rf"^galmuri: \[Errno \d+\] {reason}: \S+\n$")
                self.assertNotIn("Traceback", err)
                self.assertEqual(sorted(os.listdir(out)), names)

    def test_check_reports_an_out_it_cannot_look_into_on_one_line(self):
        # --out a file, and (not as root) an --out that cannot be searched: each file's stat
        # fails with an error other than absence, reported on one line, not "differs".
        def a_file(out):
            shutil.rmtree(out)
            with open(out, "wb") as f:
                f.write(b"not a directory\n")

        def unsearchable(out):
            os.chmod(out, 0o600)

        cases = [(a_file, "Not a directory")]
        if os.geteuid() != 0:
            cases.append((unsearchable, "Permission denied"))
        for spoil, reason in cases:
            with self.subTest(case=spoil.__name__), tempfile.TemporaryDirectory() as tmp:
                out, path = self.older_files(tmp)
                spoil(out)
                try:
                    code, err = self.main(["--zip", path, "--out", out, "--check"])
                finally:
                    if os.path.isdir(out):
                        os.chmod(out, 0o755)
                self.assertEqual(code, 1)
                self.assertRegex(err, rf"^galmuri: \[Errno \d+\] {reason}: \S+\n$")
                self.assertNotIn("differs", err)

    def test_an_out_whose_parent_cannot_be_looked_into_is_named_exactly(self):
        # --out two levels below a file, and (not as root) below a directory that cannot be
        # searched: publish's look for missing directories fails on --out itself, and the one
        # line names it, not a directory above it.
        def a_file(parent):
            with open(parent, "wb") as f:
                f.write(b"not a directory\n")

        def unsearchable(parent):
            os.mkdir(parent)
            os.chmod(parent, 0o600)

        cases = [(a_file, errno.ENOTDIR)]
        if os.geteuid() != 0:
            cases.append((unsearchable, errno.EACCES))
        for spoil, err in cases:
            with self.subTest(case=spoil.__name__), tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, "synthetic.zip")
                with open(path, "wb") as f:
                    f.write(self.ZIP)
                parent = os.path.join(tmp, "parent")
                out = os.path.join(parent, "a", "b")
                spoil(parent)
                try:
                    code, err_text = self.main(["--zip", path, "--out", out])
                finally:
                    if os.path.isdir(parent):
                        os.chmod(parent, 0o755)
                self.assertEqual(code, 1)
                self.assertEqual(err_text, f"galmuri: [Errno {err}] {os.strerror(err)}: '{out}'\n")

    def test_a_cleanup_that_fails_is_reported_on_the_one_line_with_the_first_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            out, path = self.older_files(tmp)
            refused = PermissionError("simulated: the unlink was refused")
            with fail_at("replace"), mock.patch.object(galmuri.os, "unlink", side_effect=refused):
                code, err = self.main(["--zip", path, "--out", out])
            self.assertEqual(code, 1)
            self.assertRegex(
                err,
                r"^galmuri: simulated: the replace failed; the temporary file \S+\.part is left: "
                r"simulated: the unlink was refused\n$",
            )


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
