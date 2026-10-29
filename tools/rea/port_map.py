#!/usr/bin/env python3
"""Port map: every original `game` and `u` function -> Rust path -> status (re/notes/port-map.md).

Scans crates/u6-core/src/**/*.rs for the porting markers of spec 03 §3 and joins them with the
function inventory (milestone flags, subsystem):

    /// Original: 0xADDR (...)              ported ("[partial]" / "[placeholder]" in the line
                                            downgrade it to partial / placeholder)
    /// Replaces: 0xADDR (...), 0xADDR ...  replaced by a platform piece
    todo_stub!("0xADDR ...", fallback)      stubbed (calls in code only, the id on any line)
    //! - 0xADDR ...                        planned (the module listing it will own it)

The intro program `u` has its own table: markers written `u 0xADDR` (`/// Original: u 0x...`,
`/// Replaces: u 0x...` with its paragraph, `todo_stub!("u 0x...")`, and every `u 0x...` on a `//!` line = planned)
against the procedures of `u` (REA's u-procedures.json plus the code hunk bases of u.json, less
the `jmp abs.l` thunks read from u.bin).

The ending executable uses `Original: end 0xADDR` markers and has a separate address space.
Those markers are not part of either table; they must never mark a same-address game or u
function as ported. Its research and runtime validation are tracked in spec 16 E1.

A stub under a higher-ranked status shows beside it (`ported; stub at <site>`). A stub that names a
ported address can be a stale seam, a mode the port lacks, a guard inside the port, or a stub that
names the wrong address; the totals line counts those addresses as `also named by a stub`. Partial
and placeholder rows show their stubs the same way, uncounted. A `todo_stub!` call whose id names
no address stops the run.

Subsystems the spec replaces wholesale (03 §4: SAS/C runtime -> Rust std, dos.library file
layer -> Files, OS setup -> Engine/Io) count as replaced unless a marker says otherwise.
Addresses below the image base 0x100000 (doc examples, tests) are ignored; marker addresses
that are not function entries (data tables cited on fields) are listed separately.

    python3 -I -B tools/rea/port_map.py [--inventory PATH] [--u-procedures PATH] [--check]
"""
import argparse
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "crates", "u6-core", "src")
OUT = os.path.join(ROOT, "re", "notes", "port-map.md")
INVENTORY = os.path.expanduser("~/Games/ultima6/re/inventory.json")
U_PROCEDURES = os.path.expanduser("~/Games/ultima6/re/rea/u-procedures.json")
U_LAYOUT = os.path.expanduser("~/Games/ultima6/re/u.json")
U_IMAGE = os.path.expanduser("~/Games/ultima6/re/u.bin")
IMAGE_BASE = 0x100000

RANK = ["ported", "partial", "placeholder", "stubbed", "replaced", "planned"]
NAMED = "also named by a stub"
RULE_REPLACED = {
    "runtime": "Rust std (rule: spec 03 §4)",
    "fileio": "Files (rule: spec 03 §4)",
    "os": "Engine / Io (rule: spec 03 §4)",
}

RE_ORIGINAL = re.compile(r"^\s*///\s*Original:\s*0x([0-9a-fA-F]+)\b(.*)$")
RE_REPLACES = re.compile(r"^\s*///\s*Replaces:(.*)$")
RE_STUB = re.compile(r"0x([0-9a-fA-F]+)")
RE_PLANNED = re.compile(r"^\s*//!\s*-\s*0x([0-9a-fA-F]+)\b")
RE_ADDR = re.compile(r"0x([0-9a-fA-F]+)")
RE_U_ORIGINAL = re.compile(r"^\s*///\s*Original:\s*u\s+0x([0-9a-fA-F]+)\b(.*)$")
RE_U_ADDR = re.compile(r"\bu\s+0x([0-9a-fA-F]+)")
RE_U_STUB = re.compile(r"u\s+0x([0-9a-fA-F]+)")
RE_U_PLANNED = re.compile(r"^\s*//!")
RE_ITEM = re.compile(r"\b(?:fn|struct|enum|const|static|type|trait|mod)\s+([A-Za-z_][A-Za-z0-9_]*)")
RE_FIELD = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*)\s*:")
RE_LEX = re.compile(r'/[/*]|(?<![A-Za-z0-9_])[bc]?r(#*)"|(?<![A-Za-z0-9_])[bc]"|"|\'')
RE_STR_BODY = re.compile(r'(?:[^"\\]|\\.)*', re.S)
RE_BLOCK = re.compile(r"/\*|\*/")
RE_CALL = re.compile(r"\btodo_stub\s*!\s*[(\[{]\s*")


def rust_files(src):
    for dirpath, _, names in os.walk(src):
        for name in sorted(names):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def item_after(lines, i):
    """Name of the item a doc block starting at line i documents."""
    for line in lines[i + 1:i + 40]:
        s = line.strip()
        if s.startswith("///") or s.startswith("#[") or not s:
            continue
        m = RE_ITEM.search(s) or RE_FIELD.match(line)
        return m.group(1) if m else "?"
    return "?"


def lex(text):
    """Rust source with its comments (line, nested block) and the contents of its string, raw
    string and char literals blanked, line breaks kept; and each string literal's contents by
    the offset its token starts at."""
    out, literals, i, n = [], {}, 0, len(text)

    def keep(a, b):
        out.append(text[a:b])

    def blank(a, b):
        out.append("\n".join(" " * len(part) for part in text[a:b].split("\n")))

    while (m := RE_LEX.search(text, i)) is not None:
        s, tok = m.start(), m.group(0)
        keep(i, s)
        if tok == "//":
            i = text.find("\n", s)
            i = n if i < 0 else i
            blank(s, i)
        elif tok == "/*":
            depth, i = 1, n
            for b in RE_BLOCK.finditer(text, s + 2):
                depth += 1 if b.group(0) == "/*" else -1
                if not depth:
                    i = b.end()
                    break
            blank(s, i)
        elif tok == "'":
            if text.startswith("\\", s + 1):
                close = text.find("'", s + 3)
            elif s + 2 < n and text[s + 2] == "'" and text[s + 1] != "\n":
                close = s + 2
            else:
                close = -1
            if close < 0:
                keep(s, s + 1)
                i = s + 1
            else:
                keep(s, s + 1)
                blank(s + 1, close)
                keep(close, close + 1)
                i = close + 1
        else:
            body = m.end()
            if m.group(1) is not None:
                close = text.find('"' + m.group(1), body)
                close = n if close < 0 else close
                i = min(n, close + 1 + len(m.group(1)))
            else:
                close = RE_STR_BODY.match(text, body).end()
                i = min(n, close + 1)
            literals[s] = text[body:close]
            keep(s, body)
            blank(body, close)
            keep(close, i)
    keep(i, n)
    return "".join(out), literals


def stub_calls(text):
    """(line index of `todo_stub!`, its first argument if a string literal, the fn it is in) for
    each call in code."""
    if "todo_stub" not in text:
        return
    code, literals = lex(text)
    lines = code.split("\n")
    for m in RE_CALL.finditer(code):
        i = code.count("\n", 0, m.start())
        yield i, literals.get(m.end()), enclosing_fn(lines, i)


def stubs(text, pattern):
    """(address, line index, fn) of each `todo_stub!` call in code whose id starts with the pattern."""
    for i, ident, fn in stub_calls(text):
        m = pattern.match(ident or "")
        if m:
            yield int(m.group(1), 16), i, fn


def unnamed_stubs(src):
    """The sites of the `todo_stub!` calls whose id names no address, and the number of calls."""
    sites, calls = [], 0
    for path in rust_files(src):
        rel = os.path.relpath(path, src)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        for i, ident, _ in stub_calls(text):
            calls += 1
            if not (RE_STUB.match(ident or "") or RE_U_STUB.match(ident or "")):
                sites.append(f"{rel}:{i + 1}")
    return sites, calls


def site_key(site):
    """Sort key of a stub site `file:line in fn`: the file, then the line as a number."""
    file, line = site.split(" in ")[0].rsplit(":", 1)
    return file, int(line)


def enclosing_fn(lines, i):
    """The last `fn` at or above line i of the lexed code (comments and literals blanked)."""
    for line in reversed(lines[:i + 1]):
        m = re.search(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", line)
        if m:
            return m.group(1)
    return "?"


def scan(src):
    """address -> list of (status, rust path)."""
    marks = {}

    def add(addr, status, path):
        if addr >= IMAGE_BASE:
            marks.setdefault(addr, []).append((status, path))

    for path in rust_files(src):
        rel = os.path.relpath(path, src)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        lines = text.split("\n")
        module = os.path.dirname(rel) + "/" if os.path.dirname(rel) else rel
        for a, i, fn in stubs(text, RE_STUB):
            add(a, "stubbed", f"{rel}:{i + 1} in {fn}")
        for i, line in enumerate(lines):
            m = RE_ORIGINAL.match(line)
            if m:
                tail = m.group(2)
                status = "partial" if "[partial]" in tail else (
                    "placeholder" if "[placeholder]" in tail else "ported")
                add(int(m.group(1), 16), status, f"{rel}::{item_after(lines, i)}")
                continue
            m = RE_REPLACES.match(line)
            if m:
                for a in RE_ADDR.findall(m.group(1)):
                    add(int(a, 16), "replaced", f"{rel}::{item_after(lines, i)}")
                continue
            m = RE_PLANNED.match(line)
            if m:
                add(int(m.group(1), 16), "planned", module)
    return marks


def scan_u(src):
    """`u` address -> list of (status, rust path), from the `u 0x...` markers."""
    marks = {}

    def add(addr, status, path):
        if addr >= IMAGE_BASE:
            marks.setdefault(addr, []).append((status, path))

    for path in rust_files(src):
        rel = os.path.relpath(path, src)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        lines = text.split("\n")
        module = os.path.dirname(rel) + "/" if os.path.dirname(rel) else rel
        for a, i, fn in stubs(text, RE_U_STUB):
            add(a, "stubbed", f"{rel}:{i + 1} in {fn}")
        for i, line in enumerate(lines):
            m = RE_U_ORIGINAL.match(line)
            if m:
                tail = m.group(2)
                status = "partial" if "[partial]" in tail else (
                    "placeholder" if "[placeholder]" in tail else "ported")
                add(int(m.group(1), 16), status, f"{rel}::{item_after(lines, i)}")
                continue
            m = RE_REPLACES.match(line)
            if m:
                para = m.group(1)
                for more in lines[i + 1:]:
                    rest = more.strip()
                    if not rest.startswith("///") or rest == "///":
                        break
                    para += " " + rest[3:]
                for a in RE_U_ADDR.findall(para):
                    add(int(a, 16), "replaced", f"{rel}::{item_after(lines, i)}")
                continue
            if RE_U_PLANNED.match(line):
                for a in RE_U_ADDR.findall(line):
                    add(int(a, 16), "planned", module)
    return marks


def u_procedures(proc_path, layout_path, image_path):
    """The function entries of `u`: REA's procedures and the code hunk bases, less the
    `jmp abs.l` thunks (first word 0x4ef9 in the image). address -> hunk index."""
    with open(proc_path, encoding="utf-8") as f:
        procs = {int(p["address"], 16) for p in json.load(f)["normalized_result"]}
    with open(layout_path, encoding="utf-8") as f:
        segments = json.load(f)["segments"]
    code = [(int(g["base"], 16), int(g["base"], 16) + g["size"], g["index"])
            for g in segments if g["type"] == "CODE"]
    procs |= {base for base, _, _ in code}
    image = b""
    if os.path.exists(image_path):
        with open(image_path, "rb") as f:
            image = f.read()

    def thunk(a):
        o = a - IMAGE_BASE
        return image[o:o + 2] == b"\x4e\xf9"

    def hunk(a):
        return next((i for lo, hi, i in code if lo <= a < hi), None)

    return {a: hunk(a) for a in sorted(procs) if not thunk(a) and hunk(a) is not None}


def status_of(entries):
    """(status, Rust path cell, status cell, named) of an address's markers: the highest-ranked
    status names the paths; stub sites under a higher rank join the status cell; named = the
    address is ported and a stub names it."""
    status = min((s for s, _ in entries), key=RANK.index)
    sites = sorted({path for s, path in entries if s == "stubbed"}, key=site_key)
    paths = sites if status == "stubbed" else sorted({path for s, path in entries if s == status})
    cell = status
    if sites and status != "stubbed":
        cell += "; stub at " + "; ".join(f"`{x}`" for x in sites)
    return status, "; ".join(f"`{x}`" for x in paths), cell, bool(sites) and status == "ported"


def count(totals, status, named):
    totals[status] = totals.get(status, 0) + 1
    if named:
        totals[NAMED] = totals.get(NAMED, 0) + 1


def functions(totals):
    return sum(totals.get(s, 0) for s in RANK + ["-"])


def build_u(procs, marks):
    rows, totals = [], {}
    for addr, hunk in procs.items():
        entries = marks.get(addr, [])
        if entries:
            status, where, cell, named = status_of(entries)
        else:
            status, where, cell, named = "-", "", "-", False
        count(totals, status, named)
        rows.append(f"| u 0x{addr:06x} | {hunk} | {where} | {cell} |")
    data = sorted(a for a in marks if a not in procs)
    return rows, totals, len(procs), data


def render_u(rows, totals, n, data, marks):
    out = [
        "",
        "## u (the intro program)",
        "",
        "Markers written `u 0x...` against the functions of `u` (REA's procedure list plus the code",
        "hunk bases, less the `jmp abs.l` thunks). Same status precedence as above.",
        "",
        f"Totals: u functions {summary(totals, n)}.",
        "",
        "| address | hunk | Rust path | status |",
        "|---|---|---|---|",
        *rows,
    ]
    if data:
        out += ["", "### u data addresses cited by markers", "",
                "Marker addresses of `u` that are not function entries (globals cited on fields):", ""]
        for a in data:
            paths = sorted({path for _, path in marks[a]})
            out.append(f"- u 0x{a:06x}: " + "; ".join(f"`{x}`" for x in paths))
    return "\n".join(out) + "\n"


def m1_flag(p):
    if p["milestone1_stub"]:
        return "S"
    return "M" if p["milestone1"] else ""


def build(inventory, marks):
    procs = [p for p in inventory["procedures"] if p["kind"] != "thunk"]
    by_addr = {int(p["address"], 16): p for p in procs}
    rows, totals, m1_totals = [], {}, {}
    for addr in sorted(by_addr):
        p = by_addr[addr]
        entries = marks.get(addr, [])
        if entries:
            status, where, cell, named = status_of(entries)
        elif p["subsystem"] in RULE_REPLACED:
            status, where, cell, named = "replaced", RULE_REPLACED[p["subsystem"]], "replaced", False
        else:
            status, where, cell, named = "-", "", "-", False
        count(totals, status, named)
        if p["milestone1"] or p["milestone1_stub"]:
            count(m1_totals, status, named)
        rows.append(f"| 0x{addr:06x} | {p['subsystem']} | {m1_flag(p)} | {where} | {cell} |")
    data = sorted(a for a in marks if a not in by_addr)
    return rows, totals, m1_totals, len(procs), data


def summary(totals, n):
    def part(s):
        named = f" ({NAMED} {totals[NAMED]})" if s == "ported" and totals.get(NAMED) else ""
        return f"{s} {totals[s]}{named}"

    return f"{n}: " + ", ".join(part(s) for s in RANK + ["-"] if totals.get(s))


def render(rows, totals, m1_totals, n, data, marks):
    m1_n = functions(m1_totals)
    out = [
        "---",
        "title: ultima6 - port map (generated)",
        "type: note",
        "tags: [ultima6, re, porting, generated]",
        "---",
        "",
        "# Port map: original function -> Rust",
        "",
        "Generated by `python3 -I -B tools/rea/port_map.py` from the markers in",
        "`crates/u6-core/src` (spec 03 §3) joined with the function inventory. Do not edit by hand.",
        "M1 column: `M` = milestone-1 function, `S` = milestone-1 stub point. Status precedence:",
        "ported > partial > placeholder > stubbed > replaced > planned; `-` = no marker yet.",
        "`ported; stub at <site>` = a stub that names a ported address: it can be a stale seam, a mode the",
        "port lacks, a guard inside the port, or a stub that names the wrong address (counted as",
        "`also named by a stub`); partial and placeholder rows show their stubs the same way.",
        "",
        f"Totals: functions {summary(totals, n)}. Milestone 1 (M + S) {summary(m1_totals, m1_n)}.",
        "",
        "| address | subsystem | M1 | Rust path | status |",
        "|---|---|---|---|---|",
        *rows,
    ]
    if data:
        out += ["", "## Data addresses cited by markers", "",
                "Marker addresses that are not function entries (tables cited on fields):", ""]
        for a in data:
            paths = sorted({path for _, path in marks[a]})
            out.append(f"- 0x{a:06x}: " + "; ".join(f"`{x}`" for x in paths))
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--inventory", default=INVENTORY)
    ap.add_argument("--u-procedures", default=U_PROCEDURES)
    ap.add_argument("--u-layout", default=U_LAYOUT)
    ap.add_argument("--u-image", default=U_IMAGE)
    ap.add_argument("--src", default=SRC)
    ap.add_argument("--out", default=OUT)
    ap.add_argument("--check", action="store_true", help="exit 1 if --out is not up to date")
    args = ap.parse_args()
    unnamed, calls = unnamed_stubs(args.src)
    if unnamed:
        print(f"port-map: {len(unnamed)} of {calls} todo_stub! calls name no address: " + ", ".join(unnamed),
              file=sys.stderr)
        return 1
    with open(args.inventory, encoding="utf-8") as f:
        inventory = json.load(f)
    marks = scan(args.src)
    rows, totals, m1_totals, n, data = build(inventory, marks)
    text = render(rows, totals, m1_totals, n, data, marks)
    u_line = ""
    if os.path.exists(args.u_procedures) and os.path.exists(args.u_layout):
        u_marks = scan_u(args.src)
        u_rows, u_totals, u_n, u_data = build_u(
            u_procedures(args.u_procedures, args.u_layout, args.u_image), u_marks)
        text += render_u(u_rows, u_totals, u_n, u_data, u_marks)
        u_line = f"u {summary(u_totals, u_n)}"
    if args.check:
        try:
            with open(args.out, encoding="utf-8") as f:
                same = f.read() == text
        except FileNotFoundError:
            same = False
        print("port-map: up to date" if same else "port-map: STALE (rerun without --check)")
        return 0 if same else 1
    with open(args.out, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {os.path.relpath(args.out, ROOT)}: functions {summary(totals, n)}")
    print(f"milestone 1 {summary(m1_totals, functions(m1_totals))}")
    if u_line:
        print(u_line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
