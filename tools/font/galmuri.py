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

--zip reads that file instead of downloading (its sha256 is checked all the same); --check
writes nothing and fails unless the files in --out are the ones the release gives.
"""
import argparse
import hashlib
import io
import os
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


def glyphs(bdf):
    """(encoding, (w, h, xoff, yoff), [row ints]) for every STARTCHAR..ENDCHAR record."""
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
                enc = int(words[1])
            elif words and words[0] == "BBX":
                if len(words) != 5:
                    raise FontError(f"line {i + 1}: BBX takes four numbers")
                bbx = tuple(int(v) for v in words[1:])
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
        try:
            yield enc, bbx, [int(r, 16) for r in rows]
        except ValueError as e:
            raise FontError(f"U+{enc:04X}: bitmap row is not hex") from e


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


def fetch(path):
    if path:
        with open(path, "rb") as f:
            data = f.read()
    else:
        with tempfile.TemporaryDirectory() as tmp:
            dst = os.path.join(tmp, "galmuri.zip")
            urllib.request.urlretrieve(URL, dst)
            with open(dst, "rb") as f:
                data = f.read()
    got = sha256(data)
    if got != ZIP_SHA256:
        raise FontError(f"release zip sha256 {got}, pinned {ZIP_SHA256}")
    return data


def build(zip_bytes):
    """(blob, OFL text, NOTICE text) from the release zip's bytes."""
    with zipfile.ZipFile(io.BytesIO(zip_bytes)) as z:
        bdf = z.read(BDF_MEMBER).decode("utf-8")
        licence = z.read(LICENSE_MEMBER)
    blob = table(bdf)
    if len(blob) != (LAST - FIRST + 1) * CELL:
        raise FontError(f"blob of {len(blob)} bytes")
    got = sha256(blob)
    if got != BLOB_SHA256:
        raise FontError(f"blob sha256 {got}, pinned {BLOB_SHA256}")
    return blob, licence, notice(got).encode("utf-8")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--zip", help="a local copy of the release zip (default: download it)")
    p.add_argument("--out", default=OUT, help="the directory written (default: site/fonts)")
    p.add_argument("--check", action="store_true", help="compare with --out, write nothing")
    a = p.parse_args(argv)
    try:
        files = dict(zip((BLOB, OFL, NOTICE), build(fetch(a.zip))))
    except (FontError, OSError, KeyError, zipfile.BadZipFile) as e:
        print(f"galmuri: {e}", file=sys.stderr)
        return 1
    if a.check:
        stale = []
        for name, data in files.items():
            path = os.path.join(a.out, name)
            if not os.path.exists(path) or open(path, "rb").read() != data:
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
