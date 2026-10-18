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

The release zip is kept in the cache, $XDG_CACHE_HOME/ultima6/ (~/.cache/ultima6/ without it),
after its sha256 is checked, and read from there the next time. It is published whole or not
at all: written to a temporary file beside it, synced, then renamed into place, so an
interrupted run leaves no partial file there. --zip reads that file instead (its sha256 is
checked all the same).

--check fails unless the files in --out are the ones the release gives, and writes nothing
anywhere, the cache included: with no cached zip it downloads the release into memory and
leaves the cache as it was (a run without --check fills it).

Errors (a refused zip, a malformed font, a failed download or write) are one line on stderr
and exit status 1.
"""
import argparse
import hashlib
import http.client
import io
import os
import re
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


def publish(path, data):
    """Writes `data` at `path` whole or not at all: a temporary file in the same directory,
    flushed, synced and closed, then renamed over `path`. On any error the temporary file is
    removed and `path` is as it was."""
    folder = os.path.dirname(path) or "."
    os.makedirs(folder, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=folder, prefix=f".{os.path.basename(path)}.", suffix=".part")
    try:
        with open(fd, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except FileNotFoundError:
            pass
        raise


def fetch(path, cache=CACHE, store=True):
    """The release zip's bytes: from `path`, else the cache, else downloaded (then published
    to the cache when `store`; --check does not store)."""
    source = path or (cache if cache and os.path.exists(cache) else None)
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
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
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
        print(f"galmuri: {e}", file=sys.stderr)
        return 1
    if a.check:
        stale = []
        for name, data in files.items():
            path = os.path.join(a.out, name)
            if not os.path.exists(path):
                stale.append(name)
                continue
            with open(path, "rb") as f:
                if f.read() != data:
                    stale.append(name)
        if stale:
            print(f"galmuri: differs from the release: {', '.join(stale)}", file=sys.stderr)
            return 1
        print(f"galmuri: {a.out} matches {VERSION}")
        return 0
    os.makedirs(a.out, exist_ok=True)
    for name, data in files.items():
        with open(os.path.join(a.out, name), "wb") as f:
            f.write(data)
    print(f"galmuri: wrote {', '.join(files)} to {a.out} (blob sha256 {BLOB_SHA256})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
