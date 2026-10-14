#!/usr/bin/env python3
"""Port map: every original `game` function -> Rust path -> status (re/notes/port-map.md).

Scans crates/u6-core/src/**/*.rs for the porting markers of spec 03 §3 and joins them with the
function inventory (milestone flags, subsystem):

    /// Original: 0xADDR (...)              ported ("[partial]" / "[placeholder]" in the line
                                            downgrade it to partial / placeholder)
    /// Replaces: 0xADDR (...), 0xADDR ...  replaced by a platform piece
    todo_stub!("0xADDR ...", fallback)      stubbed
    //! - 0xADDR ...                        planned (the module listing it will own it)

Subsystems the spec replaces wholesale (03 §4: SAS/C runtime -> Rust std, dos.library file
layer -> Files, OS setup -> Engine/Io) count as replaced unless a marker says otherwise.
Addresses below the image base 0x100000 (doc examples, tests) are ignored; marker addresses
that are not function entries (data tables cited on fields) are listed separately.

    python3 -I -B tools/rea/port_map.py [--inventory PATH] [--check]
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
IMAGE_BASE = 0x100000

RANK = ["ported", "partial", "placeholder", "stubbed", "replaced", "planned"]
RULE_REPLACED = {
    "runtime": "Rust std (rule: spec 03 §4)",
    "fileio": "Files (rule: spec 03 §4)",
    "os": "Engine / Io (rule: spec 03 §4)",
}

RE_ORIGINAL = re.compile(r"^\s*///\s*Original:\s*0x([0-9a-fA-F]+)\b(.*)$")
RE_REPLACES = re.compile(r"^\s*///\s*Replaces:(.*)$")
RE_STUB = re.compile(r"todo_stub!\(\s*\"0x([0-9a-fA-F]+)")
RE_PLANNED = re.compile(r"^\s*//!\s*-\s*0x([0-9a-fA-F]+)\b")
RE_ADDR = re.compile(r"0x([0-9a-fA-F]+)")
RE_ITEM = re.compile(r"\b(?:fn|struct|enum|const|static|type|trait|mod)\s+([A-Za-z_][A-Za-z0-9_]*)")
RE_FIELD = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*)\s*:")


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


def enclosing_fn(lines, i):
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
            lines = f.read().split("\n")
        module = os.path.dirname(rel) + "/" if os.path.dirname(rel) else rel
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
            for a in RE_STUB.findall(line):
                add(int(a, 16), "stubbed", f"{rel}:{i + 1} in {enclosing_fn(lines, i)}")
            m = RE_PLANNED.match(line)
            if m:
                add(int(m.group(1), 16), "planned", module)
    return marks


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
            status = min((s for s, _ in entries), key=RANK.index)
            paths = sorted({path for s, path in entries if s == status})
            where = "; ".join(f"`{x}`" for x in paths)
        elif p["subsystem"] in RULE_REPLACED:
            status, where = "replaced", RULE_REPLACED[p["subsystem"]]
        else:
            status, where = "-", ""
        totals[status] = totals.get(status, 0) + 1
        if p["milestone1"] or p["milestone1_stub"]:
            m1_totals[status] = m1_totals.get(status, 0) + 1
        rows.append(f"| 0x{addr:06x} | {p['subsystem']} | {m1_flag(p)} | {where} | {status} |")
    data = sorted(a for a in marks if a not in by_addr)
    return rows, totals, m1_totals, len(procs), data


def summary(totals, n):
    parts = [f"{s} {totals[s]}" for s in RANK + ["-"] if totals.get(s)]
    return f"{n}: " + ", ".join(parts)


def render(rows, totals, m1_totals, n, data, marks):
    m1_n = sum(m1_totals.values())
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
    ap.add_argument("--src", default=SRC)
    ap.add_argument("--out", default=OUT)
    ap.add_argument("--check", action="store_true", help="exit 1 if --out is not up to date")
    args = ap.parse_args()
    with open(args.inventory, encoding="utf-8") as f:
        inventory = json.load(f)
    marks = scan(args.src)
    rows, totals, m1_totals, n, data = build(inventory, marks)
    text = render(rows, totals, m1_totals, n, data, marks)
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
    print(f"milestone 1 {summary(m1_totals, sum(m1_totals.values()))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
