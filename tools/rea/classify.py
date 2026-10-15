#!/usr/bin/env python3
"""Function inventory + subsystem classification of the Amiga Ultima VI `game` executable.

Reads the local REA decompile dumps (one JSON per procedure), the string list, the flattened
image and its segment table, and writes a JSON inventory outside the repo. Pseudocode is only
scanned for references (call names, string labels, small-data offsets, library vectors); none of
it is copied into the output.

    python3 -I -B tools/rea/classify.py [--re ~/Games/ultima6/re] [--out <inventory.json>]
"""
import argparse
import ast
import json
import os
import re
import struct
import sys
from collections import Counter, defaultdict

TABLE_CAP = 24          # slots read from a pointer table starting at the referenced offset
A4 = 0x0015B6F0          # SAS/C small-data base [likely]: hunk 33 base 0x1536F0 + 0x8000
EXEC_ABS = "_DAT_00000004"

# --- AmigaOS library vector offsets (public NDK values, negative offsets from the base) -----
# "v" = the call site context was checked by hand (argument shape / neighbouring calls agree);
# "u" = taken from the NDK table only.
EXEC_LVO = {
    0x1e: ("Supervisor", "u"), 0x78: ("Disable", "u"), 0x7e: ("Enable", "u"),
    0x84: ("Forbid", "v"), 0x8a: ("Permit", "v"), 0xa2: ("SetIntVector", "u"),
    0xa8: ("AddIntServer", "v"), 0xae: ("RemIntServer", "v"), 0xc6: ("AllocMem", "v"),
    0xd2: ("FreeMem", "v"), 0xd8: ("AvailMem", "v"), 0xfc: ("Remove", "u"),
    0x114: ("FindName", "u"), 0x126: ("FindTask", "v"), 0x132: ("SetSignal", "v"),
    0x13e: ("Wait", "u"), 0x144: ("Signal", "u"), 0x14a: ("AllocSignal", "v"),
    0x150: ("FreeSignal", "v"), 0x162: ("AddPort", "u"), 0x168: ("RemPort", "u"),
    0x16e: ("PutMsg", "u"), 0x174: ("GetMsg", "v"), 0x17a: ("ReplyMsg", "v"),
    0x180: ("WaitPort", "v"), 0x186: ("FindPort", "u"), 0x198: ("OldOpenLibrary", "u"),
    0x19e: ("CloseLibrary", "v"), 0x1bc: ("OpenDevice", "v"), 0x1c2: ("CloseDevice", "v"),
    0x1c8: ("DoIO", "v"), 0x1ce: ("SendIO", "u"), 0x1d4: ("CheckIO", "u"),
    0x1da: ("WaitIO", "u"), 0x1e0: ("AbortIO", "u"), 0x1f2: ("OpenResource", "v"),
    0x228: ("OpenLibrary", "v"),
}
DOS_LVO = {
    0x1e: ("Open", "v"), 0x24: ("Close", "v"), 0x2a: ("Read", "v"), 0x30: ("Write", "v"),
    0x36: ("Input", "u"), 0x3c: ("Output", "u"), 0x42: ("Seek", "v"),
    0x48: ("DeleteFile", "v"), 0x4e: ("Rename", "v"), 0x54: ("Lock", "v"),
    0x5a: ("UnLock", "v"), 0x60: ("DupLock", "v"), 0x66: ("Examine", "u"),
    0x6c: ("ExNext", "u"), 0x7e: ("CurrentDir", "v"), 0x84: ("IoErr", "v"),
    0x8a: ("CreateProc", "u"), 0x90: ("Exit", "u"), 0x96: ("LoadSeg", "u"),
    0x9c: ("UnLoadSeg", "u"), 0xae: ("DeviceProc", "u"), 0xc0: ("DateStamp", "v"),
    0xc6: ("Delay", "u"), 0xd8: ("IsInteractive", "u"), 0xde: ("Execute", "u"),
}
CIA_RES_LVO = {0x6: ("AddICRVector", "u"), 0xc: ("RemICRVector", "v"),
               0x12: ("AbleICR", "u"), 0x18: ("SetICR", "u")}
LIBS = {"exec": EXEC_LVO, "dos": DOS_LVO, "ciab": CIA_RES_LVO}
# Library base globals proven from the code: DOSBase is written by the startup's OpenLibrary
# result (0x161ec4 = A4 + 0x67d4); the ciab.resource base is kept at 0x101176 in hunk 0.
# The graphics-setup routine (0x14ef06) stores its two OpenLibrary results at A4 + 0x67cc
# (intuition, first name in its string pool) and A4 + 0x657a (graphics: it then reads
# GfxBase + 0x26, the system copper list). No call in `game` goes through either base.
BASE_GLOBALS = {0x161EC4: "dos", 0x101176: "ciab", 0x161EBC: "intuition", 0x161C6A: "graphics"}

HW = {0xDFF080: "COP1LC", 0xDFF088: "COPJMP1", 0xDFF096: "DMACON", 0xDFF1DC: "BEAMCON0",
      0xBFE001: "CIAA-PRA", 0xBFD400: "CIAB-TALO", 0xBFD500: "CIAB-TAHI",
      0xBFD600: "CIAB-TBLO", 0xBFD700: "CIAB-TBHI", 0xBFDE00: "CIAB-CRA",
      0xBFDF00: "CIAB-CRB"}

# --- pseudocode scanning ------------------------------------------------------------------
RE_CALL = re.compile(r"\b(thunk_)?FUN_([0-9a-f]{8})\b(\s*\()?")
RE_SLABEL = re.compile(r"\bs_\S*?_([0-9a-f]{8})\b")
RE_DAT = re.compile(r"\b_?DAT_([0-9a-f]{8})\b")
RE_A4 = re.compile(r"unaff_A4 \+ (-?)(0x[0-9a-f]+|\d+)\b")
RE_OSCALL = re.compile(r"\(\*\*?\(code \*\*\)\((?P<base>[^;{}]{1,60}?) \+ -0x(?P<lvo>[0-9a-f]+)\)\)")
RE_A4BASE = re.compile(r"^\*\(int \*\)\(unaff_A4 \+ (-?)(0x[0-9a-f]+)\)$")
RE_IDENT = re.compile(r"^[A-Za-z_]\w*$")
# longword load from an indexed small-data array: `*(undefined4 *)(unaff_A4 + OFF + i * 4)`
RE_A4IDX = re.compile(r"\*\((?:undefined4|int|char \*|byte \*|undefined \*) \*\)\s*\(unaff_A4 \+ (-?)(0x[0-9a-f]+)\s*\+")


def _signed(sign, num):
    v = int(num, 16) if num.startswith("0x") else int(num)
    return -v if sign else v


def base_library(base, text, depth=0):
    """Map the base expression of `(**(code **)(base + -lvo))()` to a library name."""
    if base == EXEC_ABS:
        return "exec"
    if base == "unaff_A4":
        return "a4-fnptr"          # call through a small-data function-pointer variable
    if base == "unaff_A6":
        return "a6?"               # base register passed in; library not provable here
    m = RE_A4BASE.match(base)
    if m:
        return BASE_GLOBALS.get(A4 + _signed(m.group(1), m.group(2)), "global?")
    m = RE_DAT.fullmatch(base)
    if m:
        return BASE_GLOBALS.get(int(m.group(1), 16), "global?")
    if RE_IDENT.match(base) and depth < 3:
        # A local holding a base: every assignment must resolve to the same library; a local
        # that is reassigned from something else (e.g. ExecBase, then DOSBase) stays "?".
        libs = {base_library(rhs.strip(), text, depth + 1)
                for rhs in re.findall(r"\b" + re.escape(base) + r" = ([^;]+);", text)}
        if len(libs) == 1 and "?" not in libs:
            return libs.pop()
    return "?"


def lvo_name(lib, lvo):
    table = LIBS.get(lib)
    if table and lvo in table:
        return table[lvo]
    return ("-0x%x" % lvo, "u")


def scan(name, text):
    """Extract references from one procedure's pseudocode (pure; unit-tested)."""
    lines = text.splitlines()
    sig = next((i for i, l in enumerate(lines) if re.search(r"\b" + re.escape(name) + r"\s*\(", l)), -1)
    calls, ptrs = set(), set()
    for i, l in enumerate(lines):
        for m in RE_CALL.finditer(l):
            if i == sig and (m.group(1) or "") + "FUN_" + m.group(2) == name:
                continue
            (calls if m.group(3) else ptrs).add(int(m.group(2), 16))
    os_calls = []
    for m in RE_OSCALL.finditer(text):
        lib = base_library(m.group("base").strip(), text)
        lvo = int(m.group("lvo"), 16)
        if lib == "a4-fnptr":
            continue
        fn, ver = lvo_name(lib, lvo)
        os_calls.append({"lib": lib, "lvo": -lvo, "name": fn, "checked": ver == "v"})
    fnptr_calls = sorted({A4 - int(m.group("lvo"), 16) for m in RE_OSCALL.finditer(text)
                          if m.group("base").strip() == "unaff_A4"})
    a4 = sorted({A4 + _signed(m.group(1), m.group(2)) for m in RE_A4.finditer(text)})
    a4_indexed = sorted({A4 + _signed(m.group(1), m.group(2)) for m in RE_A4IDX.finditer(text)})
    dat = sorted({int(x, 16) for x in RE_DAT.findall(text)})
    hw = sorted({HW[a] for a in dat if a in HW} | {HW[a] for a in HW if ("0x%x" % a) in text})
    return {
        "lines": len(lines),
        "calls": sorted(calls),
        "fn_ptrs": sorted(ptrs - calls),
        "slabels": sorted({int(x, 16) for x in RE_SLABEL.findall(text)}),
        "a4": a4,
        "a4_indexed": a4_indexed,
        "dat": [a for a in dat if a >= 0x100000],
        "os": os_calls,
        "fnptr_calls": fnptr_calls,
        "hw": hw,
    }


# --- inputs --------------------------------------------------------------------------------
def load_segments(path):
    g = json.load(open(path))
    return [{"index": s["index"], "type": s["type"], "base": int(s["base"], 16),
             "size": s["size"], "loaded": s["loaded_bytes"]} for s in g["segments"]]


def hunk_of(segs, addr):
    for s in segs:
        if s["base"] <= addr < s["base"] + s["size"]:
            return s["index"]
    return None


def load_strings(path):
    out = {}
    for line in open(path, encoding="utf-8", errors="replace"):
        a, _, s = line.rstrip("\n").partition(" ")
        try:
            s = ast.literal_eval(s)
        except (ValueError, SyntaxError):
            pass
        out[int(a, 16)] = s
    return out


class StringIndex:
    """Exact-or-containing lookup over the string list."""

    def __init__(self, strings):
        self.strings = strings
        self.starts = sorted(strings)

    def find(self, addr):
        import bisect
        i = bisect.bisect_right(self.starts, addr) - 1
        if i < 0:
            return None
        s = self.starts[i]
        if addr == s or addr < s + len(self.strings[s]) + 1:
            return s
        return None


def reloc_sites(hunk_file, segs):
    """Absolute addresses of every RELOC32 longword (parsed with tools/hunk2raw.py)."""
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    import hunk2raw
    parsed = hunk2raw.parse(open(hunk_file, "rb").read())
    sites = set()
    for h, s in zip(parsed["hunks"], segs):
        for _target, off in h["relocs"]:
            sites.add(s["base"] + off)
    return sites


def pointer_runs(image, base, segs, sites):
    """Runs of consecutive relocated longwords: [(start, [values])]. DATA hunks keep every run;
    CODE hunks keep runs of >= 2 (lone relocs there are instruction operands, not tables)."""
    runs = []
    for s in segs:
        if s["type"] not in ("DATA", "CODE"):
            continue
        first = len(runs)
        cur = None
        for a in sorted(x for x in sites if s["base"] <= x < s["base"] + s["loaded"]):
            v = struct.unpack_from(">I", image, a - base)[0]
            if cur and a == cur[0] + 4 * len(cur[1]):
                cur[1].append(v)
            else:
                cur = (a, [v])
                runs.append(cur)
        if s["type"] == "CODE":
            runs[first:] = [r for r in runs[first:] if len(r[1]) >= 2]
    return runs


def run_containing(runs, starts, addr):
    import bisect
    i = bisect.bisect_right(starts, addr) - 1
    if i >= 0 and addr < runs[i][0] + 4 * len(runs[i][1]):
        return runs[i]
    return None


def code_xrefs(image, base, segs, sites, resolve):
    """Byte-level operand xrefs from CODE hunks to `targets` (strings the decompiler printed as
    register arguments, e.g. OpenLibrary names): absolute longwords at RELOC32 sites, and
    d16(PC) operands of LEA/PEA. `resolve(addr)` maps an operand target to a key or None.
    Returns {key: [operand address]}."""
    out = defaultdict(list)
    for a in sites:
        h = hunk_of(segs, a)
        if h is None or segs[h]["type"] != "CODE":
            continue
        k = resolve(struct.unpack_from(">I", image, a - base)[0])
        if k is not None:
            out[k].append(a)
    for s in segs:
        if s["type"] != "CODE":
            continue
        off = s["base"] - base
        for p in range(0, s["loaded"] - 3, 2):
            op = struct.unpack_from(">H", image, off + p)[0]
            if (op & 0xF1FF) == 0x41FA or op == 0x487A:      # LEA d16(PC),An / PEA d16(PC)
                d16 = struct.unpack_from(">h", image, off + p + 2)[0]
                k = resolve(s["base"] + p + 2 + d16)
                if k is not None:
                    out[k].append(s["base"] + p)
    return out


def owner(starts, addr):
    import bisect
    i = bisect.bisect_right(starts, addr) - 1
    return starts[i] if i >= 0 else None


def load_dumps(dump_dir):
    procs = {}
    for f in os.listdir(dump_dir):
        if not f.startswith("0x"):
            continue
        j = json.load(open(os.path.join(dump_dir, f)))
        addr = int(j["address"], 16)
        procs[addr] = {"name": j["procedure"]["name"], "status": j.get("status"),
                       "text": j.get("pseudocode") or ""}
    return procs


def build(re_dir, hunk_file):
    segs = load_segments(os.path.join(re_dir, "game.json"))
    image = open(os.path.join(re_dir, "game.bin"), "rb").read()
    strings = load_strings(os.path.join(re_dir, "rea", "strings.txt"))
    sidx = StringIndex(strings)
    procs = load_dumps(os.path.join(re_dir, "dumps", "game"))
    sites = reloc_sites(hunk_file, segs)
    runs = pointer_runs(image, 0x100000, segs, sites)
    starts = [r[0] for r in runs]
    runstart = set(starts)
    inv = {}
    for addr, p in sorted(procs.items()):
        name = p["name"]
        kind = "entry" if name == "entry" else "thunk" if name.startswith("thunk_") else "function"
        sc = scan(name, p["text"])
        refs = set(sc["slabels"]) | set(sc["a4"]) | set(sc["dat"])
        direct, tables = set(), {}
        for r in refs:
            s = sidx.find(r)
            if s is not None:
                direct.add(s)
                continue
            run = run_containing(runs, starts, r)
            if run is None and r in sc["a4_indexed"] and (r + 4) in runstart:
                run = run_containing(runs, starts, r + 4)      # one-based index into a table
            if run:
                # Adjacent arrays merge into one reloc run, so read only from the referenced
                # slot onward, capped: the table a function indexes starts at its base offset.
                k = max(0, (r - run[0]) // 4)
                vals = run[1][k:k + TABLE_CAP]
                strs = [sidx.find(v) for v in vals]
                tables["0x%x" % (run[0] + 4 * k)] = {
                    "entries": len(vals),
                    "strings": sorted({x for x in strs if x is not None}),
                    "functions": sorted({v for v in vals if v in procs}),
                }
        inv[addr] = {
            "address": addr, "name": name, "kind": kind,
            "target": int(name[-8:], 16) if kind == "thunk" else None,
            "hunk": hunk_of(segs, addr), "lines": sc["lines"],
            "calls": [c for c in sc["calls"] if c != addr],
            "fn_ptrs": sc["fn_ptrs"], "os": sc["os"], "hw": sc["hw"],
            "fnptr_calls": sc["fnptr_calls"],
            "strings": sorted(direct), "tables": tables,
        }
    starts = sorted(inv)
    runmap = {r[0]: r for r in runs}

    def resolve(v):
        if v in runmap:
            return ("run", v)
        t = sidx.find(v)
        return ("str", t) if t is not None else None

    for (k, t), ops in code_xrefs(image, 0x100000, segs, sites, resolve).items():
        for op in ops:
            o = owner(starts, op)
            if o is None or hunk_of(segs, o) != hunk_of(segs, op):
                continue
            got = inv[o].setdefault("strings_img", set())
            if k == "str":
                got.add(t)
            else:
                got.update(x for x in (sidx.find(v) for v in runmap[t][1]) if x is not None)
    for r in inv.values():
        r["strings_img"] = sorted(r.get("strings_img", ()))
    callers = defaultdict(set)
    for a, r in inv.items():
        if r["kind"] != "thunk":
            tf = [f for t in r["tables"].values() for f in t["functions"]]
            for c in set(r["calls"] + r["fn_ptrs"] + tf):
                callers[c].add(a)
    for a, r in inv.items():
        key = r["target"] if r["kind"] == "thunk" else a
        r["callers"] = sorted(callers.get(key, ()))
    return {"segments": segs, "strings": strings, "procs": inv, "runs": runs}


# --- classification ------------------------------------------------------------------------
# One SAS/C code hunk ~ one source module. Default (class, subsystem, module note) per hunk,
# chosen from the distinctive strings and OS calls each hunk's functions reference.
HUNK_MODULE = {
    0: ("crt", "runtime", "SAS/C startup + MED player + C library"),
    1: ("game", "main", "command loop, timed events, per-turn status effects"),
    2: ("game", "ui", "command input, text output, view refresh, save/load front end"),
    3: ("game", "map", "map chunks and tile lookup"),
    4: ("game", "map", "map-view visibility / light grid"),
    5: ("game", "objects", "object list, object blocks, object names"),
    7: ("game", "sound", "music start/stop"),
    8: ("game", "ui", "status panels, inventory and ready/unready"),
    9: ("game", "converse", "talk command, converse text substitutions"),
    10: ("game", "converse", "converse script interpreter"),
    11: ("game", "magic", "spellcasting and spell effects"),
    12: ("game", "actors", "actor turns, NPC AI, schedules, party movement"),
    13: ("game", "combat", "attacks, damage, monster specials"),
    14: ("game", "party", "rest/camp, ship repair, healing"),
    15: ("game", "objects", "look/get/drop/move/search, doors, traps, light"),
    16: ("game", "objects", "use command and per-object use handlers"),
    17: ("game", "unknown", "direction/line helpers (purpose not pinned)"),
    18: ("game", "ui", "portrait/converse file loaders, view effects"),
    21: ("file-io", "fileio", "game file layer over dos.library, memory"),
    22: ("game", "boot", "main(): initialisation and data loading"),
    26: ("os-wrapper", "os", "library open, memory, VBlank interrupt server"),
    29: ("input", "input", "input.device handler and raw-key mapping"),
    30: ("graphics", "graphics", "tile data loading and blitting"),
    32: ("crt", "runtime", "SAS/C __main"),
    34: ("graphics", "graphics", "copper list / display setup"),
}
MED_RANGE = (0x1001F8, 0x101388)   # [likely] MED player module inside hunk 0

# Distinctive-string votes: the strings of `game` whose text marks a subsystem, by address
# (image address in game.bin). Chosen once by reading the text; the text stays out of the
# repo, so a string counts by its address alone.
STRING_SUBSYSTEM = {
    "saveload": (
        0x101f4c, 0x104fd4, 0x1345e2, 0x13463c, 0x13464a, 0x1540a3,
    ),
    "converse": (
        0x112758, 0x112760, 0x1129a2, 0x112a4a, 0x112a5a, 0x112a68, 0x112aa2, 0x155338,
        0x155344,
    ),
    "magic": (
        0x115422, 0x115446, 0x115452, 0x115514, 0x11553c, 0x115876, 0x1159ca, 0x115a1e,
        0x115a66, 0x115ab2, 0x115b36, 0x128730, 0x154369,
    ),
    "combat": (
        0x101f02, 0x110b72, 0x110b7a, 0x110b86, 0x110b8c, 0x110b94, 0x110b9c, 0x153fa0,
        0x153fc2, 0x153fcd, 0x153fda, 0x154014, 0x154024, 0x154044, 0x154055, 0x154196,
        0x1541bc, 0x1541cb, 0x1543d8,
    ),
    "party": (
        0x112a1e, 0x1272a8, 0x1272b6, 0x1272d2, 0x127356, 0x12cd66, 0x12cfc0, 0x1543b0,
    ),
    "time": (
        0x15445d,
    ),
    "sound": (
        0x110806, 0x110816, 0x110824, 0x110832, 0x110840, 0x11084e, 0x11085c, 0x11086a,
        0x154475, 0x15447f,
    ),
    "objects": (
        0x10a490, 0x10a4bc, 0x1287f6, 0x128820, 0x12882a, 0x12883e, 0x128848, 0x128896,
        0x1288d6, 0x12c91c, 0x12c926, 0x12ca2a, 0x12ca66, 0x12ca7e, 0x12ca96, 0x12cab4,
        0x12cad2, 0x153ec6, 0x153ed8, 0x153eee,
    ),
    "inventory": (
        0x110c14, 0x110c24, 0x110c38, 0x110c46, 0x110c6a,
    ),
    "ui": (
        0x110bb2, 0x110bba, 0x110bc2, 0x110bea, 0x1544b4,
    ),
    "map": (
        0x104fca, 0x108ab8, 0x152df6,
    ),
    "schedules": (
        0x104fbc,
    ),
    "boot": (
        0x1350a0, 0x1350a6,
    ),
}
STRING_VOTES = {a: sub for sub, addrs in STRING_SUBSYSTEM.items() for a in addrs}
SHARED_TABLE_MIN_USERS = 6   # a pointer table read by this many functions is a message pool

# Hand-read functions: addr -> (class, subsystem, role). A role ending in "[likely]" is an
# inference from structure (callers/callees/globals), the rest was read off the decompile.
CURATED = {
    # SAS/C runtime
    0x100000: ("crt", "runtime", "startup: clear BSS, open dos.library, take Workbench msg, call __main"),
    0x100180: ("crt", "runtime", "startup exit: close/unlock, reply WB msg, close dos.library"),
    0x101390: ("crt", "runtime", "jump to __main"),
    0x1535d8: ("crt", "runtime", "__main: build argv and call main (0x1351d8)"),
    0x101934: ("crt", "runtime", "free every tracked allocation"),
    0x1019ac: ("crt", "runtime", "time() over DateStamp"),
    0x101ac8: ("crt", "runtime", "exit path used on fatal errors [likely]"),
    0x101b1c: ("crt", "runtime", "write() over dos Write"),
    0x101b70: ("crt", "runtime", "lseek() over dos Seek"),
    0x101be4: ("crt", "runtime", "delete a message port"),
    0x101c24: ("crt", "runtime", "close() over dos Close"),
    0x101d2c: ("crt", "runtime", "tracked AllocMem"),
    0x101d68: ("crt", "runtime", "create a message port"),
    0x101e34: ("crt", "runtime", "poll break signal, call abort hook [likely]"),
    0x101ea8: ("crt", "runtime", "empty stub"),
    # MED player
    0x10102a: ("med", "sound", "MED init: signal, audio.device, ciab.resource, CIA-B timer"),
    0x101126: ("med", "sound", "MED shutdown: remove CIA vector, close audio.device"),
    0x100234: ("med", "sound", "MED timer-interrupt play routine [likely]"),
    0x10038a: ("med", "sound", "MED note/pattern player core [likely]"),
    0x100fa4: ("med", "sound", "audio filter via CIA-A PRA [likely]"),
    0x101010: ("med", "sound", "stop current song [likely]"),
    # main loop, time
    0x1041d8: ("game", "main", "command loop + A-Z key dispatcher, end-of-turn"),
    0x102ef0: ("game", "main", "post-step refresh: rebuild view if dirty, redraw"),
    0x10202e: ("game", "map", "render map window: tiles then objects [likely]"),
    0x102c90: ("game", "map", "present/refresh map window [likely]"),
    0x102f3c: ("game", "party", "per-turn party status effects (poison, web)"),
    0x10368a: ("game", "time", "advance clock: light/status timers, date+wind line"),
    0x103f64: ("game", "ui", "redraw side panel for current panel mode"),
    # h2 ui / input / save
    0x10790a: ("game", "ui", "read a command key; handles panel/mouse events"),
    0x106bbc: ("game", "ui", "wait for next input event, map clicks to commands"),
    0x108658: ("game", "ui", "printf-style output to the message scroll"),
    0x108376: ("game", "ui", "word-wrapped text output [likely]"),
    0x1052c8: ("game", "saveload", "Restore: reload saved state"),
    0x105716: ("game", "saveload", "Save: write savegame/objlist + object blocks"),
    0x105350: ("game", "saveload", "load state: objlist, schedules, base tiles; place party"),
    # map
    0x1089d8: ("game", "map", "open the map chunk file"),
    0x108ac4: ("game", "map", "load map region around the party [likely]"),
    0x108f90: ("game", "map", "rebuild the map-window tile cache [likely]"),
    0x10996c: ("game", "map", "tile at (x,y): window cache, else from chunk"),
    0x109e74: ("game", "map", "view-window flags (blocking/light/visible) from objects"),
    # objects
    0x10ad1a: ("game", "objects", "begin iterating objects at a location"),
    0x10aaca: ("game", "objects", "next object of the iteration"),
    0x10ae52: ("game", "objects", "object stacked on / contained in an object"),
    0x10c1d6: ("game", "objects", "remove an object from the map"),
    0x10c9d2: ("game", "objects", "move an object/actor to a map location"),
    0x10ee76: ("game", "objects", "object name from look.lzd"),
    0x10eef8: ("game", "objects", "display name of an object"),
    0x10d376: ("game", "objects", "add object to the region list (fails when full)"),
    0x10e1ce: ("game", "objects", "load object blocks for a region [likely]"),
    0x1298b8: ("game", "objects", "Look command"),
    0x12a226: ("game", "objects", "Drop command"),
    0x12a7a4: ("game", "objects", "Get command"),
    0x12ae04: ("game", "objects", "Move (push) command"),
    0x129544: ("game", "objects", "search here: secret doors, hidden items"),
    0x12ba36: ("game", "objects", "trap effects (acid, poison, bomb, gas)"),
    0x12bbc0: ("game", "objects", "open/close/lock/unlock a door or chest"),
    0x12c326: ("game", "objects", "light a torch"),
    0x12c594: ("game", "objects", "douse a light source"),
    0x1305e0: ("game", "objects", "Use command: dispatch to per-object handlers"),
    # sound
    0x110878: ("game", "sound", "play song N (load its file named in the song table, start MED); 0 = stop"),
    0x110912: ("game", "sound", "pick a song for the current area and play it"),
    # ui / inventory
    0x110d2c: ("game", "ui", "party-member stats panel (STR/DEX/INT, HP/MP, level)"),
    0x1109e0: ("game", "ui", "party roster panel [likely]"),
    0x111c30: ("game", "inventory", "inventory panel [likely]"),
    0x110c76: ("game", "ui", "print version line"),
    0x112052: ("game", "inventory", "Ready an item"),
    0x1123d6: ("game", "inventory", "Unready an item"),
    # converse
    0x1127c4: ("game", "converse", "Talk command: target, shrine/statue cases, start talk"),
    0x1125b8: ("game", "converse", "converse text substitutions (forms of address, time of day)"),
    0x114afa: ("game", "converse", "start conversation: checks, load script, greeting"),
    0x1130a2: ("game", "converse", "converse interpreter main loop (input, keyword match)"),
    0x11404c: ("game", "converse", "converse opcode executor"),
    0x1148f0: ("game", "converse", "print converse text, control chars"),
    0x133e5e: ("game", "converse", "load an NPC's converse.a script block"),
    0x133d3e: ("game", "converse", "load an NPC portrait (portrait.a)"),
    # magic
    0x11af10: ("game", "magic", "Cast command: needs readied spellbook"),
    0x116f2c: ("game", "magic", "spell effect dispatcher [likely]"),
    0x11a586: ("game", "magic", "spell target selection [likely]"),
    # actors / schedules / party
    0x1213de: ("game", "actors", "turn scheduler: next actor by movement points; NPC turns; tick clock"),
    0x1200b0: ("game", "schedules", "NPC turn: walk to current schedule entry / do worktype"),
    0x121826: ("game", "schedules", "hourly: pick each NPC's schedule entry for hour+weekday"),
    0x11d4ee: ("game", "party", "player move step (direction, drunk wobble, blocking)"),
    0x11c910: ("game", "party", "party member follows the leader [likely]"),
    0x120b64: ("game", "party", "party centre / formation anchor [likely]"),
    0x121056: ("game", "party", "leader / solo-mode bookkeeping per turn [likely]"),
    0x120a1c: ("game", "actors", "scan nearby hostiles, combat-state toggle [likely]"),
    0x11d1d4: ("game", "map", "dungeon/level transition (dungeon names) [likely]"),
    # combat / party
    0x124caa: ("game", "combat", "Attack command"),
    0x121c64: ("game", "combat", "begin combat mode"),
    0x121d78: ("game", "combat", "break off combat"),
    0x123d86: ("game", "combat", "apply damage and report"),
    0x12395a: ("game", "combat", "print wound level"),
    0x124a98: ("game", "combat", "attack with weapon, missile ammo check"),
    0x124232: ("game", "combat", "hit side effects (charm, paralyse, special weapons)"),
    0x127b1a: ("game", "party", "Rest command: camp checks, heal, ship repair"),
    # file I/O, OS, input, graphics
    0x1348d0: ("file-io", "fileio", "open a game data file (disk-swap prompts)"),
    0x1349a0: ("file-io", "fileio", "check a disk volume is present, prompt to insert"),
    0x134a54: ("file-io", "fileio", "open a file for writing [likely]"),
    0x134ac4: ("file-io", "fileio", "read N bytes from the current file"),
    0x134afe: ("file-io", "fileio", "write N bytes to the current file"),
    0x134b32: ("file-io", "fileio", "close the current file"),
    0x134b48: ("file-io", "fileio", "delete a file"),
    0x134b9e: ("file-io", "fileio", "rename a file"),
    0x1346a0: ("file-io", "fileio", "seek in the current file"),
    0x134c1c: ("os-wrapper", "os", "AllocMem wrapper"),
    0x134c80: ("os-wrapper", "os", "FreeMem wrapper (no callers)"),
    0x134cd6: ("os-wrapper", "os", "FreeMem wrapper"),
    0x1351d8: ("game", "boot", "main(): setup, load data, character check, music, command loop"),
    0x134f08: ("game", "boot", "load animdata / tileflag tables"),
    0x14ef06: ("os-wrapper", "os", "system setup: memory, libraries, copper, VBlank server, input"),
    0x14ee70: ("os-wrapper", "os", "system shutdown: remove VBlank server, close libraries"),
    0x14f16c: ("graphics", "graphics", "load objtiles.ega"),
    0x151b08: ("input", "input", "input.device event handler (queue key/mouse events)"),
    0x151c60: ("input", "input", "open input.device, install the handler"),
    0x151cdc: ("input", "input", "remove the input handler"),
    0x151d00: ("input", "input", "raw key code to key/command, qualifier state"),
    0x152e50: ("graphics", "graphics", "load tile graphics (maptiles/tileindx/masktype/animmask)"),
    0x152ffa: ("graphics", "graphics", "tile blitter, entry A (shares body with 0x153002) [likely]"),
    0x153002: ("graphics", "graphics", "tile blitter, entry B [likely]"),
    0x151ee4: ("game", "util", "random number generator core"),
    0x151f06: ("game", "util", "random number in [lo, hi]"),
    0x16236a: ("graphics", "graphics", "install the game copper list (COP1LC/COPJMP1)"),
    0x162294: ("graphics", "graphics", "set BEAMCON0 display mode [likely]"),
    0x1623d6: ("graphics", "graphics", "load objtiles.ega [likely]"),
    0x131196: ("game", "unknown", "index of a value in a small-data word list"),
}


def classify(data):
    procs = data["procs"]
    real = {a: r for a, r in procs.items() if r["kind"] != "thunk"}
    users = Counter(k for r in real.values() for k in r["tables"])
    for r in real.values():
        ev = set(r["strings"]) | set(r["strings_img"])
        for k, t in r["tables"].items():
            if users[k] < SHARED_TABLE_MIN_USERS:
                ev.update(t["strings"])
        r["evidence_strings"] = sorted(ev)
    med = med_members(real)
    for a, r in real.items():
        cls, sub, _ = HUNK_MODULE.get(r["hunk"], ("game", "unknown", ""))
        votes = Counter()
        for s in r["evidence_strings"]:
            if s in STRING_VOTES:
                votes[STRING_VOTES[s]] += 1
        role = None
        if a in CURATED:
            cls, sub, role = CURATED[a]
            conf, rule = ("medium" if role.endswith("[likely]") else "high"), "curated"
        elif a in med:
            cls, sub, conf, rule = "med", "sound", "medium", "med-reach"
        elif MED_RANGE[0] <= a < MED_RANGE[1]:
            cls, sub, conf, rule = "med", "sound", "low", "med-range"
        elif cls == "game" and votes:
            sub = votes.most_common(1)[0][0]
            conf, rule = ("high" if votes[sub] >= 2 else "medium"), "strings"
        else:
            conf, rule = ("medium" if sub != "unknown" else "low"), "hunk"
        r.update({"class": cls, "subsystem": sub, "confidence": conf, "rule": rule, "role": role})
    propagate(real)
    for a, r in real.items():
        subs = {real[c]["subsystem"] for c in r["callers"] if c in real}
        r["caller_subsystems"] = len(subs)
        if r["role"] is None:
            r["role"] = auto_role(r)
        r["evidence"] = evidence(r)
    neighbour_roles(real)
    for a, r in procs.items():
        if r["kind"] == "thunk":
            t = real.get(r["target"], {})
            r.update({k: t.get(k) for k in ("class", "subsystem", "confidence", "caller_subsystems")})
            r.update({"rule": "thunk", "role": "jump stub -> 0x%x" % r["target"],
                      "evidence": ["JMP 0x%x" % r["target"]], "evidence_strings": []})
    mark_milestone(real)
    for r in procs.values():
        if r["kind"] == "thunk":
            r["milestone1"] = real.get(r["target"], {}).get("milestone1", False)
    return data


def med_members(real):
    """Functions inside MED_RANGE reachable (calls only) from the MED roots."""
    roots = [0x100234, 0x10102a, 0x101126]
    seen, todo = set(), list(roots)
    while todo:
        a = todo.pop()
        if a in seen or a not in real or not (MED_RANGE[0] <= a < MED_RANGE[1]):
            continue
        seen.add(a)
        todo.extend(real[a]["calls"])
    return seen


def propagate(real):
    """Unknown-subsystem functions take their callers' subsystem when the callers agree."""
    for _ in range(4):
        changed = False
        for a, r in real.items():
            if r["subsystem"] != "unknown" or r["rule"] == "curated":
                continue
            subs = Counter(real[c]["subsystem"] for c in r["callers"]
                           if c in real and real[c]["subsystem"] != "unknown")
            if not subs or len(subs) >= 4:
                continue
            top, n = subs.most_common(1)[0]
            if n * 3 >= sum(subs.values()) * 2:
                r.update({"subsystem": top, "confidence": "low", "rule": "callers"})
                changed = True
        if not changed:
            break


# Milestone 1 = boot into the world map, draw the map with objects and party, walk with the
# keyboard, NPCs follow schedules, talk to an NPC. Closure over direct calls from the startup,
# not entering the command handlers below (other commands), magic, or non-leaf combat code;
# the cut targets are recorded as stub points.
# Roots: the startup, plus the input.device handler (its address is handed to the OS in a
# register, so no direct call reaches it).
MILESTONE_SEEDS = [0x100000, 0x151B08]
MILESTONE_STOP = {
    0x124caa: "Attack", 0x11af10: "Cast", 0x1298b8: "Look", 0x12a7a4: "Get",
    0x12a226: "Drop", 0x12ae04: "Move/push", 0x1305e0: "Use", 0x105716: "Save",
    0x1052c8: "Restore", 0x127b1a: "Rest", 0x121c64: "begin combat", 0x121d78: "end combat",
    0x112052: "Ready", 0x1123d6: "Unready",
}
MILESTONE_STOP_SUBSYSTEMS = {"magic", "sound"}


def milestone_cut(r):
    """Spells and music are not needed; in the combat module only the leaf helpers (actor-stat getters
    that time, converse and the panels also call) are kept, every non-leaf is a stub point."""
    if r["subsystem"] in MILESTONE_STOP_SUBSYSTEMS:
        return True
    return r["subsystem"] == "combat" and bool(r["calls"])


def mark_milestone(real):
    seen, cut, todo = set(), defaultdict(set), [a for a in MILESTONE_SEEDS if a in real]
    while todo:
        a = todo.pop()
        if a in seen:
            continue
        seen.add(a)
        for c in real[a]["calls"] + real[a]["fn_ptrs"]:
            if c not in real:
                continue
            if c in MILESTONE_STOP or milestone_cut(real[c]):
                cut[c].add(a)
            else:
                todo.append(c)
    for a, r in real.items():
        r["milestone1"] = a in seen
        r["milestone1_stub"] = a in cut and a not in seen


def auto_role(r):
    ev = r["evidence_strings"]
    if r["class"] == "crt":
        return "C library routine (not identified)"
    if r["class"] == "med":
        return "MED player internals [likely]"
    if ev:
        return "uses string 0x%x" % ev[0]
    if r["os"]:
        return "calls " + ", ".join(sorted({o["lib"] + "." + o["name"] for o in r["os"]})[:3])
    return None                     # filled from neighbours in neighbour_roles()


def neighbour_roles(real):
    """Roles for functions with no string/OS evidence: describe them by position only."""
    named = {a: r["role"] for a, r in real.items() if r["role"]}
    for a, r in real.items():
        if r["role"]:
            continue
        callers = [c for c in r["callers"] if c in real]
        if len(callers) == 1 and callers[0] in named:
            r["role"] = "step of 0x%x (%s)" % (callers[0], short(named[callers[0]], 34))
        elif not r["calls"]:
            r["role"] = "leaf; callers %d, from %d subsystems" % (len(callers), r["caller_subsystems"])
        else:
            known = [c for c in r["calls"] if c in named and c in CURATED]
            r["role"] = ("unnamed; calls 0x%x (%s)" % (known[0], short(named[known[0]], 30))
                         if known else "unnamed; %d callees" % len(r["calls"]))


def short(s, n=18):
    s = s.replace("\n", " ").replace("|", "/").replace("`", "'").strip()
    return s if len(s) <= n else s[: n - 1] + "~"


def evidence(r):
    out = []
    for s in r["evidence_strings"][:2]:
        out.append("string 0x%x" % s)
    for o in r["os"]:
        tok = "%s.%s%s" % (o["lib"], o["name"], "" if o["checked"] else "?")
        if tok not in out:
            out.append(tok)
    out.extend(r["hw"])
    if len(r["callers"]) == 1:
        out.append("by 0x%x" % r["callers"][0])
    elif len(r["callers"]) >= 8:
        out.append("fan-in %d" % len(r["callers"]))
    return out[:3]


# --- output --------------------------------------------------------------------------------
FAMILIES = [
    ("platform", "C runtime, OS, file I/O, input, graphics, MED music",
     ["runtime", "os", "fileio", "input", "graphics", "sound"]),
    ("core", "boot, main loop, clock, save/load, shared helpers, unknown",
     ["boot", "main", "time", "saveload", "util", "unknown"]),
    ("world", "map and objects (incl. object commands)", ["map", "objects"]),
    ("actors", "actors, schedules, party, combat", ["actors", "schedules", "party", "combat"]),
    ("magic", "spells", ["magic"]),
    ("converse", "conversation", ["converse"]),
    ("ui", "panels, text output, input commands, inventory", ["ui", "inventory"]),
]


def to_json(data):
    out = []
    for a, r in sorted(data["procs"].items()):
        out.append({
            "address": "0x%x" % a, "name": r["name"], "kind": r["kind"],
            "target": "0x%x" % r["target"] if r["target"] else None,
            "hunk": r["hunk"], "lines": r["lines"], "class": r["class"],
            "subsystem": r["subsystem"], "confidence": r["confidence"], "rule": r["rule"],
            "role": r["role"], "evidence": r["evidence"],
            "milestone1": r["milestone1"], "milestone1_stub": r.get("milestone1_stub", False),
            "calls": ["0x%x" % c for c in r["calls"]], "fn_ptrs": ["0x%x" % c for c in r["fn_ptrs"]],
            "callers": ["0x%x" % c for c in r["callers"]],
            "strings": ["0x%x" % x for x in r["strings"]],
            "strings_img": ["0x%x" % x for x in r["strings_img"]],
            "tables": {k: {"entries": t["entries"],
                           "strings": ["0x%x" % x for x in t["strings"]],
                           "functions": ["0x%x" % x for x in t["functions"]]}
                       for k, t in r["tables"].items()},
            "os": r["os"], "hw": r["hw"],
            "fnptr_calls": ["0x%x" % x for x in r["fnptr_calls"]],
        })
    return {"a4": "0x%x" % A4, "count": len(out), "procedures": out}


def histogram(data, key="class", real_only=False):
    return Counter(r[key] for r in data["procs"].values()
                   if not real_only or r["kind"] != "thunk")


def md_row(r, stubs):
    m = "M" if r["milestone1"] else ("S" if r.get("milestone1_stub") else "")
    return "| 0x%x | %d | %d | %s | %s/%s | %s | %s | %s | %s |" % (
        r["address"], r["hunk"], r["lines"], r["class"], r["subsystem"], r["confidence"][0],
        r["role"].replace("|", "/"), "; ".join(e.replace("|", "/") for e in r["evidence"]), m,
        " ".join("%x" % t for t in stubs))


FRONT = "---\ntitle: %s\ntype: note\ntags: [ultima6, re, amiga, %s]\n---\n\n"
TABLE_HEAD = ("| address | hunk | lines | class | subsystem/conf | role | key evidence | M1 | stubs |\n"
              "|---|---|---|---|---|---|---|---|---|\n")


def write_notes(data, notes_dir):
    procs = data["procs"]
    stubs = defaultdict(list)
    for a, r in procs.items():
        if r["kind"] == "thunk":
            stubs[r["target"]].append(a)
    real = {a: r for a, r in procs.items() if r["kind"] != "thunk"}
    fam_of = {sub: f for f, _, subs in FAMILIES for sub in subs}
    by_fam = defaultdict(list)
    for a, r in sorted(real.items()):
        by_fam[fam_of.get(r["subsystem"], "core")].append(r)
    for fam, desc, subs in FAMILIES:
        rows = by_fam[fam]
        nstub = sum(len(stubs[r["address"]]) for r in rows)
        with open(os.path.join(notes_dir, "functions-%s.md" % fam), "w") as f:
            f.write(FRONT % ("ultima6 game functions - %s" % fam, "functions"))
            f.write("# Functions: %s (%s)\n\n" % (fam, desc))
            f.write("Generated by `tools/rea/classify.py --notes re/notes`; legend and index in "
                    "[functions.md](functions.md). %d functions, %d jump stubs.\n\n"
                    % (len(rows), nstub))
            for sub in subs:
                part = [r for r in rows if r["subsystem"] == sub]
                if not part:
                    continue
                f.write("## %s (%d)\n\n" % (sub, len(part)))
                f.write(TABLE_HEAD)
                for r in part:
                    f.write(md_row(r, sorted(stubs[r["address"]])) + "\n")
                f.write("\n")
    write_index(data, notes_dir, real, stubs, by_fam)


def string_coverage(data):
    used = set()
    for r in data["procs"].values():
        used |= set(r["strings"]) | set(r["strings_img"])
        for t in r["tables"].values():
            used |= set(t["strings"])
    return len(used & set(data["strings"])), len(data["strings"])


def write_index(data, notes_dir, real, stubs, by_fam):
    procs, segs = data["procs"], data["segments"]
    allh, realh = histogram(data), histogram(data, real_only=True)
    w = []
    w.append(FRONT % ("ultima6 game functions - index", "functions"))
    w.append("# Amiga `game` executable: function inventory (index)\n\n")
    w.append("All %d REA procedures of `game` (base 0x00100000, A4 = 0x%06X [likely]) = %d "
             "functions + %d jump stubs (`thunk_FUN_*`: a 6-byte `JMP abs.l` at the end of a "
             "calling hunk, one per target that hunk calls). "
             "Generated from the local decompile dumps; no decompiler output is copied here, "
             "roles are one-line descriptions.\n\n" % (len(procs), A4, len(real),
                                                       len(procs) - len(real)))
    w.append("    python3 -I -B tools/rea/classify.py --notes re/notes   "
             "# also writes ~/Games/ultima6/re/inventory.json\n\n")
    w.append("Legend. `class`: crt = SAS/C runtime, os-wrapper, med = MED music player, file-io, "
             "graphics, input, game. `subsystem/conf`: conf h = curated or two distinctive "
             "strings, m = module default, one string, or a `[likely]` curated role, l = "
             "inferred from callers or an unreached MED-range function. `M1` = M in the "
             "milestone-1 set, S = stub point at its boundary (see subsystems.md). `stubs` = "
             "jump-stub addresses that jump to the function. `key evidence`: the address of a "
             "string the function uses (its text is not reproduced), library call (`?` = vector not checked by hand), custom-chip/CIA "
             "register, sole caller, or fan-in.\n\n")
    w.append("## Class histogram\n\n| class | all procedures | functions only |\n|---|---|---|\n")
    for k in sorted(allh, key=lambda k: -allh[k]):
        w.append("| %s | %d | %d |\n" % (k, allh[k], realh.get(k, 0)))
    w.append("| total | %d | %d |\n\n" % (sum(allh.values()), sum(realh.values())))
    hits, total = string_coverage(data)
    w.append("Strings: %d of %d listed strings are tied to a function (decompiler labels, "
             "small-data offsets, reloc pointer tables, LEA/PEA d16(PC) and absolute operands "
             "in the image).\n\n" % (hits, total))
    w.append("## Hunks (one SAS/C module each)\n\n| hunk | base | functions | stubs | default "
             "class/subsystem | module |\n|---|---|---|---|---|---|\n")
    for sg in segs:
        n = sum(1 for r in real.values() if r["hunk"] == sg["index"])
        t = sum(1 for r in procs.values() if r["kind"] == "thunk" and r["hunk"] == sg["index"])
        if not n and not t:
            continue
        cls, sub, mod = HUNK_MODULE.get(sg["index"], ("?", "?", "-"))
        w.append("| %d | 0x%06x | %d | %d | %s/%s | %s |\n"
                 % (sg["index"], sg["base"], n, t, cls, sub, mod))
    w.append("\n## Files by subsystem\n\n| file | subsystems (functions) |\n|---|---|\n")
    for fam, desc, subs in FAMILIES:
        c = Counter(r["subsystem"] for r in by_fam[fam])
        w.append("| [functions-%s.md](functions-%s.md) | %s |\n"
                 % (fam, fam, ", ".join("%s %d" % (s, c[s]) for s in subs if c[s])))
    w.append("\nSubsystem map, main loop, dispatcher, porting order and the milestone-1 set: "
             "[subsystems.md](subsystems.md).\n")
    with open(os.path.join(notes_dir, "functions.md"), "w") as f:
        f.write("".join(w))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    home = os.path.expanduser("~/Games/ultima6")
    ap.add_argument("--re", default=os.path.join(home, "re"))
    ap.add_argument("--hunk-file", default=os.path.join(home, "unpacked/d1/INSTALL/game"))
    ap.add_argument("--out", default=None, help="inventory JSON (default <re>/inventory.json)")
    ap.add_argument("--notes", default=None, help="write functions*.md into this directory")
    a = ap.parse_args(argv)
    data = classify(build(a.re, a.hunk_file))
    n = len(data["procs"])
    out = a.out or os.path.join(a.re, "inventory.json")
    with open(out, "w") as f:
        json.dump(to_json(data), f, indent=1)
    if a.notes:
        write_notes(data, a.notes)
    print("procedures %d -> %s" % (n, out))
    for k, v in sorted(histogram(data).items(), key=lambda kv: -kv[1]):
        print("  class %-10s %4d" % (k, v))
    m1 = [r for r in data["procs"].values() if r["kind"] != "thunk" and r["milestone1"]]
    print("milestone1 functions %d; stub points %d" % (
        len(m1), sum(1 for r in data["procs"].values() if r.get("milestone1_stub"))))
    hits, total = string_coverage(data)
    print("strings tied to functions %d/%d" % (hits, total))
    return 0 if n == 1383 else 1


if __name__ == "__main__":
    sys.exit(main())
