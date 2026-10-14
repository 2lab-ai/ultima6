#!/usr/bin/env python3
"""Flatten an AmigaOS hunk load file into a big-endian memory image.

Usage: hunk2raw.py <hunkfile> <out.bin> <out.json> [--base 0x00100000]

Segments of the root node are laid out contiguously from --base, each aligned
to 8 bytes, with RELOC32/RELOC32SHORT applied, so the image can be imported
into Ghidra as a raw 68000 binary at --base. The JSON map lists every segment,
symbol and (if present) overlay node. Addresses in the JSON are "0x%08x"
strings; sizes and counts are ints.

The input is untrusted: every read is bounds-checked and any unknown hunk type
or truncation aborts with a non-zero exit before an output file is written.
"""
import argparse
import hashlib
import json
import struct
import sys

HUNK_NAME = 0x3E8
HUNK_CODE = 0x3E9
HUNK_DATA = 0x3EA
HUNK_BSS = 0x3EB
HUNK_RELOC32 = 0x3EC
HUNK_EXT = 0x3EF
HUNK_SYMBOL = 0x3F0
HUNK_DEBUG = 0x3F1
HUNK_END = 0x3F2
HUNK_HEADER = 0x3F3
HUNK_OVERLAY = 0x3F5
HUNK_BREAK = 0x3F6
HUNK_DREL32 = 0x3F7  # V37 LoadSeg treats this as RELOC32SHORT in load files
HUNK_RELOC32SHORT = 0x3FC

TYPE_NAMES = {HUNK_CODE: "CODE", HUNK_DATA: "DATA", HUNK_BSS: "BSS"}
OTHER_NAMES = {
    0x3E7: "HUNK_UNIT", HUNK_HEADER: "HUNK_HEADER", HUNK_OVERLAY: "HUNK_OVERLAY", HUNK_BREAK: "HUNK_BREAK", 0x3ED: "HUNK_RELOC16", 0x3EE: "HUNK_RELOC8",
    0x3F8: "HUNK_DREL16", 0x3F9: "HUNK_DREL8", 0x3FA: "HUNK_LIB",
    0x3FB: "HUNK_INDEX", 0x3FD: "HUNK_RELRELOC32", 0x3FE: "HUNK_ABSRELOC16",
}
TYPE_MASK = 0x3FFFFFFF  # bits 30/31 of a type longword carry memory flags
MEMF_CHIP = 0x40000000
MEMF_FAST = 0x80000000
EXT_COMMON = 130
EXT_RELCOMMON = 137
MAX_IMAGE = 256 << 20  # header sizes are untrusted; no real Amiga program comes close


class HunkError(Exception):
    pass


def hx(v):
    return "0x%08x" % v


class Reader:
    def __init__(self, data):
        self.data = data
        self.pos = 0

    def eof(self):
        return self.pos >= len(self.data)

    def take(self, n, what):
        if n < 0 or self.pos + n > len(self.data):
            raise HunkError("truncated at file offset 0x%x reading %s (need %d bytes, %d left)"
                            % (self.pos, what, n, len(self.data) - self.pos))
        b = self.data[self.pos:self.pos + n]
        self.pos += n
        return b

    def long(self, what):
        return struct.unpack(">I", self.take(4, what))[0]

    def word(self, what):
        return struct.unpack(">H", self.take(2, what))[0]

    def name(self, nlongs, what):
        raw = self.take(nlongs * 4, what)
        return raw.rstrip(b"\0").decode("latin-1")

    def align4(self):
        if self.pos % 4:
            self.take(4 - self.pos % 4, "alignment padding")


def mem_label(flags, ext):
    if flags == MEMF_CHIP:
        return "CHIP"
    if flags == MEMF_FAST:
        return "FAST"
    if flags == MEMF_CHIP | MEMF_FAST:
        return "EXT:" + hx(ext)
    return "ANY"


def type_name(t):
    tt = t & TYPE_MASK
    return OTHER_NAMES.get(tt, "0x%x" % tt)


def read_header(r):
    """Read a HUNK_HEADER body (type longword already consumed)."""
    libs = []
    while True:
        n = r.long("resident library length")
        if n == 0:
            break
        libs.append(r.name(n, "resident library name"))
    table_size = r.long("table size")
    first = r.long("first hunk")
    last = r.long("last hunk")
    if not first <= last < table_size:
        raise HunkError("bad hunk header: first=%d last=%d table_size=%d" % (first, last, table_size))
    sizes = []
    for i in range(first, last + 1):
        s = r.long("hunk %d size" % i)
        flags = s & 0xC0000000
        ext = r.long("hunk %d extended memory flags" % i) if flags == 0xC0000000 else 0
        sizes.append({"index": i, "size": (s & TYPE_MASK) * 4, "mem_flags": mem_label(flags, ext)})
    return {"libs": libs, "table_size": table_size, "first": first, "last": last, "sizes": sizes}


def read_reloc32(r, hunk):
    while True:
        count = r.long("hunk %d RELOC32 count" % hunk)
        if count == 0:
            return
        target = r.long("hunk %d RELOC32 target" % hunk)
        for _ in range(count):
            yield target, r.long("hunk %d RELOC32 offset" % hunk)


def read_reloc32short(r, hunk):
    while True:
        count = r.word("hunk %d RELOC32SHORT count" % hunk)
        if count == 0:
            break
        target = r.word("hunk %d RELOC32SHORT target" % hunk)
        for _ in range(count):
            yield target, r.word("hunk %d RELOC32SHORT offset" % hunk)
    r.align4()


def read_symbols(r, hunk):
    out = []
    while True:
        n = r.long("hunk %d symbol length" % hunk)
        if n == 0:
            return out
        nm = r.name(n & 0xFFFFFF, "hunk %d symbol name" % hunk)
        out.append({"name": nm, "type": n >> 24, "value": r.long("hunk %d symbol value" % hunk)})


def read_ext(r, hunk):
    out = []
    while True:
        n = r.long("hunk %d EXT entry" % hunk)
        if n == 0:
            return out
        etype = n >> 24
        nm = r.name(n & 0xFFFFFF, "hunk %d EXT name" % hunk)
        ent = {"name": nm, "type": etype}
        if etype < 128:
            ent["value"] = r.long("hunk %d EXT value" % hunk)
        else:
            if etype in (EXT_COMMON, EXT_RELCOMMON):
                ent["common_size"] = r.long("hunk %d EXT common size" % hunk)
            cnt = r.long("hunk %d EXT ref count" % hunk)
            r.take(cnt * 4, "hunk %d EXT ref offsets" % hunk)
            ent["ref_count"] = cnt
        out.append(ent)


def read_hunk(r, index, header_size):
    """Read one hunk (content + reloc/symbol/debug blocks) through HUNK_END.

    header_size is None for overlay nodes without their own header; the
    content length is then used as the size. Returns a dict, or None if a
    HUNK_BREAK/EOF ends the node before any block of this hunk.
    """
    h = {"index": index, "type": None, "name": None, "file_offset": None, "loaded": b"",
         "loaded_bytes": 0, "relocs": [], "symbols": [], "ext": [], "debug_bytes": 0,
         "header_size": header_size}
    started = False
    while True:
        if r.eof():
            if not started and header_size is None:
                return None
            raise HunkError("truncated: hunk %d has no HUNK_END (file ends at 0x%x)" % (index, r.pos))
        at = r.pos
        t = r.long("hunk %d block type" % index)
        tt = t & TYPE_MASK
        if tt == HUNK_BREAK and not started and header_size is None:
            r.pos = at
            return None
        started = True
        if tt in (HUNK_CODE, HUNK_DATA, HUNK_BSS):
            if h["type"] is not None:
                raise HunkError("hunk %d: second content block %s at 0x%x" % (index, TYPE_NAMES[tt], at))
            h["type"] = TYPE_NAMES[tt]
            h["file_offset"] = at
            n = (r.long("hunk %d length" % index) & TYPE_MASK) * 4
            if tt == HUNK_BSS:
                h["bss_bytes"] = n
            else:
                h["loaded"] = r.take(n, "hunk %d %s data" % (index, h["type"]))
                h["loaded_bytes"] = n
            limit = header_size if header_size is not None else n
            if tt != HUNK_BSS and n > limit:
                raise HunkError("hunk %d: %s data 0x%x bytes exceeds header size 0x%x"
                                % (index, h["type"], n, limit))
            if h["header_size"] is None:
                h["header_size"] = n
        elif tt == HUNK_RELOC32:
            h["relocs"].extend(read_reloc32(r, index))
        elif tt in (HUNK_RELOC32SHORT, HUNK_DREL32):
            h["relocs"].extend(read_reloc32short(r, index))
        elif tt == HUNK_SYMBOL:
            h["symbols"].extend(read_symbols(r, index))
        elif tt == HUNK_EXT:
            h["ext"].extend(read_ext(r, index))
        elif tt == HUNK_DEBUG:
            n = r.long("hunk %d debug length" % index) * 4
            r.take(n, "hunk %d debug data" % index)
            h["debug_bytes"] += n
        elif tt == HUNK_NAME:
            h["name"] = r.name(r.long("hunk %d name length" % index), "hunk %d name" % index)
        elif tt == HUNK_END:
            if h["type"] is None:
                raise HunkError("hunk %d: HUNK_END at 0x%x without CODE/DATA/BSS" % (index, at))
            return h
        else:
            raise HunkError("unknown or unsupported hunk type %s (raw 0x%08x) at file offset 0x%x in hunk %d"
                            % (type_name(t), t, at, index))


def read_overlay(r, root_header):
    at = r.pos - 4
    n = r.long("overlay table size")
    table = [r.long("overlay table") for _ in range(n + 1)]
    ov = {"file_offset": at, "table_longs": n + 1, "entries": None, "nodes": []}
    levels = table[0]  # M+2, followed by M+1 zero longwords, then 8-long entries
    rest = table[levels:] if 1 <= levels <= len(table) else None
    if rest is not None and len(rest) % 8 == 0:
        ov["max_level"] = levels - 2
        ov["entries"] = [dict(zip(("seek_offset", "dummy1", "dummy2", "level", "ordinate",
                                   "first_hunk", "symbol_hunk", "symbol_offset"), rest[i:i + 8]))
                         for i in range(0, len(rest), 8)]
    else:
        ov["raw_table"] = table
    next_index = root_header["last"] + 1
    while not r.eof():
        at = r.pos
        t = r.long("overlay node block type")
        node = {"file_offset": at, "has_header": False, "hunks": []}
        if t & TYPE_MASK == HUNK_HEADER:
            hdr = read_header(r)
            node["has_header"] = True
            for s in hdr["sizes"]:
                node["hunks"].append(read_hunk(r, s["index"], s["size"]))
                node["hunks"][-1]["mem_flags"] = s["mem_flags"]
            next_index = hdr["last"] + 1
        else:
            # No node header: hunks follow directly; their own lengths are the sizes.
            r.pos = at
            while True:
                h = read_hunk(r, next_index, None)
                if h is None:
                    break
                h["mem_flags"] = "ANY"
                node["hunks"].append(h)
                next_index += 1
        if not r.eof():
            at = r.pos
            t = r.long("HUNK_BREAK")
            if t & TYPE_MASK != HUNK_BREAK:
                raise HunkError("overlay node at 0x%x: expected HUNK_BREAK, got %s at 0x%x"
                                % (node["file_offset"], type_name(t), at))
        ov["nodes"].append(node)
    return ov


def parse(data):
    r = Reader(data)
    t = r.long("HUNK_HEADER")
    if t & TYPE_MASK != HUNK_HEADER:
        raise HunkError("not an AmigaOS load file: first longword 0x%08x (expected 0x000003f3)" % t)
    hdr = read_header(r)
    hunks = []
    for s in hdr["sizes"]:
        h = read_hunk(r, s["index"], s["size"])
        h["mem_flags"] = s["mem_flags"]
        hunks.append(h)
    overlay = None
    warnings = []
    if not r.eof():
        at = r.pos
        rest = data[at:]
        if len(rest) >= 4 and struct.unpack(">I", rest[:4])[0] & TYPE_MASK == HUNK_OVERLAY:
            r.pos += 4
            overlay = read_overlay(r, hdr)
        elif rest.strip(b"\0") == b"" and len(rest) < 4:
            warnings.append("ignored %d zero padding bytes at end of file" % len(rest))
        else:
            t = struct.unpack(">I", rest[:4])[0] if len(rest) >= 4 else None
            raise HunkError("unexpected data after last root hunk at file offset 0x%x (%s)"
                            % (at, "0x%08x" % t if t is not None else "%d stray bytes" % len(rest)))
    return {"header": hdr, "hunks": hunks, "overlay": overlay, "warnings": warnings}


def align8(v):
    return (v + 7) & ~7


def flatten(parsed, base):
    if base % 8:
        raise HunkError("--base 0x%x is not 8-byte aligned" % base)
    hdr = parsed["header"]
    hunks = parsed["hunks"]
    bases = {}
    cur = base
    for h in hunks:
        cur = align8(cur)
        bases[h["index"]] = cur
        cur += h["header_size"]
    if cur - base > MAX_IMAGE or cur > 0x100000000:
        raise HunkError("image of 0x%x bytes at base 0x%x is too large (header sizes corrupt?)"
                        % (cur - base, base))
    image = bytearray(cur - base)
    sizes = {h["index"]: h["header_size"] for h in hunks}
    warnings = list(parsed["warnings"])
    segments = []
    symbols = {}
    sym_list = []
    ext = []
    for h in hunks:
        i = h["index"]
        off = bases[i] - base
        image[off:off + h["loaded_bytes"]] = h["loaded"]
        if h["type"] == "BSS" and h.get("bss_bytes", 0) > h["header_size"]:
            warnings.append("hunk %d: BSS length 0x%x exceeds header size 0x%x (header size used)"
                            % (i, h["bss_bytes"], h["header_size"]))
        suspicious = 0
        for target, roff in h["relocs"]:
            if target not in bases:
                raise HunkError("hunk %d: RELOC32 target hunk %d is not a root hunk (%d..%d)"
                                % (i, target, hdr["first"], hdr["last"]))
            if roff + 4 > h["header_size"]:
                raise HunkError("hunk %d: relocation offset 0x%x outside segment size 0x%x"
                                % (i, roff, h["header_size"]))
            p = off + roff
            v = struct.unpack_from(">I", image, p)[0]
            if v > sizes[target]:  # stored value is an offset into the target hunk
                suspicious += 1
            struct.pack_into(">I", image, p, (v + bases[target]) & 0xFFFFFFFF)
        if suspicious:
            warnings.append("hunk %d: %d relocations whose stored offset exceeds the target segment size"
                            % (i, suspicious))
        for s in h["symbols"]:
            addr = bases[i] + s["value"]
            sym_list.append({"name": s["name"], "hunk": i, "offset": s["value"], "address": hx(addr)})
            if s["name"] in symbols and symbols[s["name"]] != hx(addr):
                warnings.append("duplicate symbol %r (kept first)" % s["name"])
            symbols.setdefault(s["name"], hx(addr))
        for e in h["ext"]:
            e = dict(e, hunk=i)
            if "value" in e:
                e["value"] = hx(e["value"])
            ext.append(e)
        seg = {"index": i, "type": h["type"], "mem_flags": h["mem_flags"], "base": hx(bases[i]),
               "size": h["header_size"], "loaded_bytes": h["loaded_bytes"],
               "reloc_count": len(h["relocs"]), "file_offset": h["file_offset"]}
        if h["name"]:
            seg["name"] = h["name"]
        if h["debug_bytes"]:
            seg["debug_bytes"] = h["debug_bytes"]
        segments.append(seg)
    overlay = None
    if parsed["overlay"] is not None:
        ov = parsed["overlay"]
        overlay = {k: v for k, v in ov.items() if k != "nodes"}
        overlay["note"] = "overlay hunks are NOT in the image; only the root node is flattened"
        overlay["nodes"] = [{"file_offset": n["file_offset"], "has_header": n["has_header"],
                             "hunks": [{"index": h["index"], "type": h["type"], "mem_flags": h["mem_flags"],
                                        "size": h["header_size"], "loaded_bytes": h["loaded_bytes"],
                                        "reloc_count": len(h["relocs"]), "symbol_count": len(h["symbols"]),
                                        "file_offset": h["file_offset"]} for h in n["hunks"]]}
                            for n in ov["nodes"]]
        warnings.append("file has HUNK_OVERLAY: %d overlay node(s) listed in 'overlay', not flattened"
                        % len(ov["nodes"]))
    info = {
        "base": hx(base),
        "entry": hx(bases[hdr["first"]]),
        "image_size": len(image),
        "resident_libraries": hdr["libs"],
        "table_size": hdr["table_size"],
        "first_hunk": hdr["first"],
        "last_hunk": hdr["last"],
        "segments": segments,
        "symbols": symbols,
        "symbol_list": sym_list,
        "ext": ext,
        "overlay": overlay,
        "warnings": warnings,
    }
    return bytes(image), info


def convert(data, base):
    image, info = flatten(parse(data), base)
    info["input_sha256"] = hashlib.sha256(data).hexdigest()
    info["input_size"] = len(data)
    info["output_sha256"] = hashlib.sha256(image).hexdigest()
    return image, info


def main(argv=None):
    ap = argparse.ArgumentParser(description="Flatten an AmigaOS hunk load file into a raw image.")
    ap.add_argument("hunkfile")
    ap.add_argument("out_bin")
    ap.add_argument("out_json")
    ap.add_argument("--base", default="0x00100000", help="load address of hunk 0 (default 0x00100000)")
    a = ap.parse_args(argv)
    try:
        base = int(a.base, 0)
    except ValueError:
        print("hunk2raw: error: bad --base %r" % a.base, file=sys.stderr)
        return 2
    try:
        with open(a.hunkfile, "rb") as f:
            data = f.read()
        image, info = convert(data, base)
    except (HunkError, OSError) as e:
        print("hunk2raw: error: %s: %s" % (a.hunkfile, e), file=sys.stderr)
        return 1
    try:
        with open(a.out_bin, "wb") as f:
            f.write(image)
        with open(a.out_json, "w") as f:
            json.dump(info, f, indent=1)
            f.write("\n")
    except OSError as e:
        print("hunk2raw: error: writing output: %s" % e, file=sys.stderr)
        return 1
    for w in info["warnings"]:
        print("hunk2raw: warning: %s" % w, file=sys.stderr)
    print("%s: %d segments, entry %s, image %d bytes, overlay %s"
          % (a.hunkfile, len(info["segments"]), info["entry"], info["image_size"],
             "yes" if info["overlay"] else "no"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
