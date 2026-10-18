#!/usr/bin/env python3
"""The Hangul table of spec 08 (Design 1): Galmuri7's 11,172 syllables as 8 x 8 cells.

Downloads the upstream quiple/galmuri release (or reads a local copy of its zip), checks the
zip's sha256 pinned below, reads Galmuri7.bdf from it and writes site/fonts/:

    galmuri7-hangul.bin   U+AC00..=U+D7A3 in order, 8 row bytes each (bit 7 = leftmost),
                          89,376 bytes; its sha256 is pinned below too
    OFL.txt               the release's LICENSE.txt as it is (SIL Open Font License 1.1)
    NOTICE.txt            the source URL, the version and both sha256s

Each glyph is placed in its cell as its BDF BBX says, against a baseline under cell row 6:
rows 0..=6 lie above the baseline and row 7 below it, so a glyph's bitmap row 0 lands on cell
row 7 - (BBX y offset + BBX height) and its column 0 on cell column (BBX x offset). A glyph
that does not fit the cell, a missing or repeated syllable, or a malformed record is an
error; nothing is written then.

The font is not derived from the game (spec 08 Rights: the blob and its OFL text are
committed).

    python3 -I -B tools/font/galmuri.py [--zip PATH] [--out DIR] [--check]

It needs Python 3.11 or later (an error's notes, BaseException.add_note); an older one from 3.6
is told so on one line, exit status 1, before anything is read or written. Before 3.6 Python
cannot parse the file (its f-strings) and stops with a SyntaxError; there is no launcher for
those.

The release zip is kept in the cache, $XDG_CACHE_HOME/ultima6/ (~/.cache/ultima6/ without it),
after its sha256 is checked, and read from there the next time. --zip reads that file instead
(its sha256 is checked all the same).

Every file the tool writes (the cached zip, each file in --out) is published whole or not at
all, one file at a time: written to a temporary file beside it, synced, then renamed into
place, so an interrupted write leaves no partial file and the one there before stays as it
was. The three files in --out are not published as a set: a failure on the second leaves the
first new and the other two as they were (on the third, the first two new), and --check fails
on that mix until a run writes all three. A failed publication removes the empty directories
it made for its file (a new --out, the cache's), so a run that fails on the first file of a new
--out leaves no --out; a run that fails after a file was published leaves that file and its
directory, and a directory that is not empty (a temporary file that could not be removed) stays.
A process killed while it writes can leave the temporary file (.<name>.<random>.part) beside
its file.

--check fails unless the files in --out are the ones the release gives, and writes nothing
anywhere, the cache included: with no cached zip it downloads the release into memory and
leaves the cache as it was (a run without --check fills it).

Errors (a refused zip, a malformed font, a failed download or write) are one line on stderr
and exit status 1.
"""
import argparse
import errno
import hashlib
import http.client
import io
import os
import re
import stat
import sys
import tempfile
import urllib.request
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "site", "fonts")

VERSION = "v2.40.4"
URL = f"https://github.com/quiple/galmuri/releases/download/{VERSION}/Galmuri-{VERSION}.zip"
ZIP_SHA256 = "c8b3d9861a62ae73c8b1178091401cd79994812437ef386413f6dd54856e60e7"
BLOB_SHA256 = "cc35ff5cb931f05acf6990a5f3cb9ed05d56142a8b330955468de1c83754634b"
# The release's LICENSE.txt, written as OFL.txt byte for byte.
LICENSE_SHA256 = "86a3ee9495f942f0243f18c103da9faca27adb88142613edb8bb852e56c892c1"
CACHE = os.path.join(
    os.environ.get("XDG_CACHE_HOME") or os.path.expanduser("~/.cache"),
    "ultima6",
    f"Galmuri-{VERSION}.zip",
)
BDF_MEMBER = "Galmuri7.bdf"
LICENSE_MEMBER = "LICENSE.txt"

# The Python the tool needs.
PYTHON = (3, 11)

BLOB = "galmuri7-hangul.bin"
OFL = "OFL.txt"
NOTICE = "NOTICE.txt"

FIRST = 0xAC00
LAST = 0xD7A3
CELL = 8
# Cell rows above the baseline (rows 0..=6); row 7 is the one row below it.
ASCENT = 7


class FontError(Exception):
    pass


def sha256(data):
    return hashlib.sha256(data).hexdigest()


INTEGER = re.compile(r"-?[0-9]+")
HEX = re.compile(r"[0-9A-Fa-f]+")


def numbers(words, counts, line):
    """The integer operands of a keyword line, `counts` the numbers of them it may have."""
    ops = words[1:]
    if len(ops) not in counts or not all(INTEGER.fullmatch(v) for v in ops):
        raise FontError(f"line {line}: {words[0]} takes {' or '.join(map(str, counts))} integers")
    try:
        return [int(v) for v in ops]
    except ValueError:
        # More digits than int() converts (sys.get_int_max_str_digits()).
        raise FontError(f"line {line}: {words[0]} has an integer too long to read") from None


def glyphs(bdf):
    """(encoding, (w, h, xoff, yoff), [row ints]) for every STARTCHAR..ENDCHAR record.

    Each record has exactly one ENCODING (one or two integers, the first the code point) and one
    BBX (four integers, width and height not negative) before its BITMAP, then exactly height
    rows of exactly 2 hex digits per 8 pixels of width.
    """
    lines = bdf.split("\n")
    i, n = 0, len(lines)
    while i < n:
        if not lines[i].startswith("STARTCHAR"):
            i += 1
            continue
        enc = bbx = None
        i += 1
        while i < n and lines[i].strip() != "BITMAP":
            words = lines[i].split()
            if words and words[0] == "ENCODING":
                if enc is not None:
                    raise FontError(f"line {i + 1}: a second ENCODING")
                enc = numbers(words, (1, 2), i + 1)[0]
            elif words and words[0] == "BBX":
                if bbx is not None:
                    raise FontError(f"line {i + 1}: a second BBX")
                bbx = tuple(numbers(words, (4,), i + 1))
                if bbx[0] < 0 or bbx[1] < 0:
                    raise FontError(f"line {i + 1}: BBX width and height are not negative")
            elif words and words[0] in ("ENDCHAR", "STARTCHAR"):
                raise FontError(f"line {i + 1}: a record without BITMAP")
            i += 1
        if enc is None or bbx is None or i >= n:
            raise FontError(f"line {i + 1}: a record without ENCODING, BBX or BITMAP")
        i += 1
        rows = []
        while i < n and lines[i].strip() != "ENDCHAR":
            rows.append(lines[i].strip())
            i += 1
        if i >= n:
            raise FontError("a record without ENDCHAR")
        i += 1
        w, h = bbx[0], bbx[1]
        digits = 2 * ((w + 7) // 8)
        if len(rows) != h or any(len(r) != digits for r in rows):
            raise FontError(f"U+{enc:04X}: {len(rows)} bitmap rows for BBX {w} x {h}")
        if not all(HEX.fullmatch(r) for r in rows):
            raise FontError(f"U+{enc:04X}: a bitmap row is not hex digits")
        yield enc, bbx, [int(r, 16) for r in rows]


def cell(enc, bbx, rows):
    """The 8 row bytes of one glyph placed in its cell (bit 7 = leftmost)."""
    w, h, xoff, yoff = bbx
    top = ASCENT - (yoff + h)
    if w < 0 or h < 0 or xoff < 0 or xoff + w > CELL or top < 0 or top + h > CELL:
        raise FontError(f"U+{enc:04X}: BBX {w} {h} {xoff} {yoff} does not fit an 8 x 8 cell")
    digits = 2 * ((w + 7) // 8)
    out = [0] * CELL
    for r, bits in enumerate(rows):
        # The row's leftmost pixel is the top bit of its hex digits; keep the BBX width only.
        left = bits >> (4 * digits - 8) if digits else 0
        mask = (0xFF << (8 - w)) & 0xFF
        out[top + r] = ((left & mask) >> xoff) & 0xFF
    return bytes(out)


def table(bdf, first=FIRST, last=LAST):
    """The blob: one cell per code point first..=last, in order."""
    found = {}
    for enc, bbx, rows in glyphs(bdf):
        if first <= enc <= last:
            if enc in found:
                raise FontError(f"U+{enc:04X} twice")
            found[enc] = cell(enc, bbx, rows)
    missing = [c for c in range(first, last + 1) if c not in found]
    if missing:
        raise FontError(f"{len(missing)} syllables missing, the first U+{missing[0]:04X}")
    return b"".join(found[c] for c in range(first, last + 1))


def notice(blob_sha):
    return (
        "Galmuri7 Hangul table (site/fonts/galmuri7-hangul.bin)\n"
        "\n"
        "  The 11,172 Hangul syllables U+AC00..U+D7A3 of Galmuri7 by Lee Minseo (quiple), each as an\n"
        "  8 x 8 cell of 8 row bytes (bit 7 = leftmost), in code point order, written by\n"
        "  tools/font/galmuri.py from the font's BDF. The font is licensed under the SIL Open Font\n"
        "  License, Version 1.1 (OFL.txt beside this file, the release's LICENSE.txt as it is).\n"
        "\n"
        f"  Source:  {URL}\n"
        f"  Version: {VERSION} (member {BDF_MEMBER})\n"
        f"  sha256 of the release zip: {ZIP_SHA256}\n"
        f"  sha256 of galmuri7-hangul.bin: {blob_sha}\n"
        "\n"
        "  Copyright (c) 2019-2025 Lee Minseo (quiple@quiple.dev)\n"
    )


def download():
    """The release zip's bytes from URL, read into memory (nothing is written)."""
    with urllib.request.urlopen(URL, timeout=120) as r:
        return r.read()


def umask():
    """The process umask (os.umask sets it, so it is set back at once)."""
    mask = os.umask(0o077)
    os.umask(mask)
    return mask


def publish(path, data):
    """Writes `data` at `path`, that one file whole or not at all: a temporary file in the same
    directory, flushed, synced and closed, then renamed over `path`. The file gets the mode of
    the one it replaces, or 0o666 less the umask when there was none (as open() would make it),
    not the temporary file's 0600. On an error `path` is as it was, the temporary file is
    removed, and so are the directories this call made for `path` (one made by another process
    meanwhile is not this call's and stays; a directory that is not empty stays), and the error
    is raised. The error raised is the first one: when closing the temporary file, removing it or
    removing a directory made for it fails too (other than a directory that is not empty), that
    is a note on it (a failed close alone is the error)."""
    folder = os.path.dirname(path) or "."
    # The directories missing, outermost first; `made` the ones this call's mkdir made.
    missing = []
    d = os.path.abspath(folder)
    while not present(d):
        missing.insert(0, d)
        d = os.path.dirname(d)
    made = []
    tmp = None
    try:
        for d in missing:
            try:
                os.mkdir(d)
            except FileExistsError:
                # Made meanwhile by another process: not this call's (mkstemp refuses a file).
                continue
            made.append(d)
        try:
            mode = stat.S_IMODE(os.stat(path).st_mode)
        except FileNotFoundError:
            mode = 0o666 & ~umask()
        fd, tmp = tempfile.mkstemp(dir=folder, prefix=f".{os.path.basename(path)}.", suffix=".part")
        # Not `with`: a close that fails after a failed write would replace its error.
        f = open(fd, "wb")
        try:
            os.fchmod(f.fileno(), mode)
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        except BaseException as first:
            try:
                f.close()
            except OSError as closing:
                first.add_note(f"closing {tmp} failed too: {closing}")
            raise
        f.close()
        os.replace(tmp, path)
    except BaseException as e:
        if tmp is not None:
            try:
                os.unlink(tmp)
            except FileNotFoundError:
                pass
            except OSError as left:
                e.add_note(f"the temporary file {tmp} is left: {left}")
        for d in reversed(made):
            try:
                os.rmdir(d)
            except FileNotFoundError:
                continue
            except OSError as kept:
                # Not empty (the temporary file left, or another's file): it and those around
                # it stay. Any other failure is a note on the first error, as unlink's is.
                if kept.errno not in (errno.ENOTEMPTY, errno.EEXIST):
                    e.add_note(f"the directory {d} made for it is left: {kept}")
                break
        raise


# What str.splitlines() breaks a line at, and the backslash, each to be shown as its escape
# (`\n`, `\x0b`, ..., `\\`): with the backslash escaped too, a line break and the text of its
# escape (a backslash, then `n`) are shown apart. translate() does not escape its own output.
ESCAPES = str.maketrans(
    {
        c: c.encode("unicode_escape").decode("ascii")
        for c in "\\\n\r\v\f\x1c\x1d\x1e\x85\u2028\u2029"
    }
)


def one_line(text):
    """`text` (a path given, or a message naming one) with each line break and backslash in it
    shown as its escape, so that it prints on one line and reads back as it was."""
    return text.translate(ESCAPES)


def message(e):
    """`e` on one line, with its notes (publish adds one when it cannot clean up). A line break
    or a backslash in them (a path's: a FontError and a note name the path as it is) is shown
    as its escape."""
    return one_line("; ".join([str(e), *getattr(e, "__notes__", ())]))


class Parser(argparse.ArgumentParser):
    """argparse's parser with its error on one line, through one_line() (an argument given can
    hold a line break), without the usage before it. Its help is argparse's."""

    def error(self, message):
        self.exit(2, f"galmuri: {one_line(message)} (see --help)\n")


def present(path):
    """Whether `path` is there. Only its absence reads as absent: another error (a directory on
    the way that cannot be searched, a file where a directory is) is raised, and main reports
    it on one line."""
    try:
        os.stat(path)
    except FileNotFoundError:
        return False
    return True


def fetch(path, cache=CACHE, store=True):
    """The release zip's bytes: from `path`, else the cache, else downloaded (then published
    to the cache when `store`; --check does not store). A cache that cannot be looked at is an
    error, not a reason to download."""
    source = path or (cache if cache and present(cache) else None)
    if source:
        with open(source, "rb") as f:
            data = f.read()
    else:
        data = download()
    got = sha256(data)
    if got != ZIP_SHA256:
        raise FontError(f"release zip {source or URL}: sha256 {got}, pinned {ZIP_SHA256}")
    if not source and cache and store:
        publish(cache, data)
    return data


def build(zip_bytes):
    """(blob, OFL text, NOTICE text) from the release zip's bytes."""
    with zipfile.ZipFile(io.BytesIO(zip_bytes)) as z:
        bdf = z.read(BDF_MEMBER).decode("utf-8")
        licence = z.read(LICENSE_MEMBER)
    if sha256(licence) != LICENSE_SHA256:
        raise FontError(f"{LICENSE_MEMBER} sha256 {sha256(licence)}, pinned {LICENSE_SHA256}")
    blob = table(bdf)
    if len(blob) != (LAST - FIRST + 1) * CELL:
        raise FontError(f"blob of {len(blob)} bytes")
    got = sha256(blob)
    if got != BLOB_SHA256:
        raise FontError(f"blob sha256 {got}, pinned {BLOB_SHA256}")
    return blob, licence, notice(got).encode("utf-8")


def main(argv=None):
    # The guard reaches Python 3.6 or later: an older one stops at parsing (the f-strings).
    if tuple(sys.version_info[:2]) < PYTHON:
        need, have = ".".join(map(str, PYTHON)), ".".join(map(str, sys.version_info[:2]))
        print(f"galmuri: needs Python {need} or later, this is {have}", file=sys.stderr)
        return 1
    # A fixed program name: argv[0] can hold a line break too.
    p = Parser(prog="galmuri.py", description=__doc__.split("\n")[0])
    p.add_argument("--zip", help="a local copy of the release zip (default: the cache, else download)")
    p.add_argument("--out", default=OUT, help="the directory written (default: site/fonts)")
    p.add_argument(
        "--check",
        action="store_true",
        help="compare with --out; write nothing anywhere, the cache included",
    )
    a = p.parse_args(argv)
    try:
        files = dict(zip((BLOB, OFL, NOTICE), build(fetch(a.zip, store=not a.check))))
    except (
        FontError,
        OSError,
        KeyError,
        UnicodeDecodeError,
        zipfile.BadZipFile,
        http.client.HTTPException,
    ) as e:
        print(f"galmuri: {message(e)}", file=sys.stderr)
        return 1
    if a.check:
        stale = []
        try:
            for name, data in files.items():
                path = os.path.join(a.out, name)
                if not present(path):
                    stale.append(name)
                    continue
                with open(path, "rb") as f:
                    if f.read() != data:
                        stale.append(name)
        except OSError as e:
            # A file there that cannot be looked at or read (no permission, a directory, an
            # --out that is a file): one line.
            print(f"galmuri: {message(e)}", file=sys.stderr)
            return 1
        if stale:
            print(f"galmuri: differs from the release: {', '.join(stale)}", file=sys.stderr)
            return 1
        print(f"galmuri: {one_line(a.out)} matches {VERSION}")
        return 0
    # One file at a time, each whole or not at all; the three are not one transaction (a failure
    # on the second leaves the first new and the other two as they were).
    try:
        for name, data in files.items():
            publish(os.path.join(a.out, name), data)
    except OSError as e:
        print(f"galmuri: {message(e)}", file=sys.stderr)
        return 1
    print(f"galmuri: wrote {', '.join(files)} to {one_line(a.out)} (blob sha256 {BLOB_SHA256})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
