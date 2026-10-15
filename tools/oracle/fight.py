"""Where the frames of the original's fight turns go: reads fight.mjs runs (turns.json) and puts
every sampled frame down to what the game was doing, from the routine it was in and its
callers (function starts = the REA dump file names, read at run time).

usage: python3 -I -B fight.py [--dumps DIR] [--image-json game.json] RUN/turns.json ...
Prints per step the frames from the key taken to the prompt (and how many of them other tasks
ran: the game waiting on the floppy), then the per-operation costs over all steps.
"""

import argparse
import bisect
import collections
import json
import os
import statistics

DUMPS = "/Users/zhugehyuk/Games/ultima6/re/dumps/game"
GAME_JSON = "/Users/zhugehyuk/Games/ultima6/re/game.json"

# Image base of each of the 35 hunks (re/notes/game-layout.md). Their lengths come from the hunk
# headers at run time (tools/hunk2raw.py's JSON beside the image, `hunk_sizes`); code hunks only
# are mapped.
BASES = [
    0x100000, 0x101EB0, 0x104FB0, 0x1089D8, 0x109AB8, 0x10A420, 0x10F080,
    0x1107A0, 0x1109E0, 0x1125B8, 0x1128D0, 0x115228, 0x11B1C0, 0x121B08,
    0x127278, 0x1283B8, 0x12C8B8, 0x131138, 0x132338, 0x134570, 0x134668,
    0x1346A0, 0x134F08, 0x135588, 0x146A40, 0x14DF70, 0x14EE70, 0x14F2C8,
    0x1512C8, 0x151B08, 0x151EE0, 0x1535D0, 0x1535D8, 0x1536F0, 0x161EC8,
]
CODE = [i for i in range(len(BASES)) if i not in (6, 20, 23, 24, 25, 27, 28, 31, 33)]

# What a frame was spent on: the first of these found among the routine and its callers.
KINDS = [
    ("flight", {0x132706, 0x1331B4}),
    ("hit flash", {0x1230CE}),
    # 0x10eef8 reads a name out of look.lzd (0x10ee76, 0x161ec8) for a message to print.
    ("object name", {0x10EEF8}),
    ("message", {0x108658, 0x107E5A, 0x108376, 0x107DD8, 0x107E00}),
    ("refresh after a turn", {0x102F2A}),
    ("refresh", {0x102EF0, 0x102C90}),
    ("party step", {0x11D4EE}),
    ("NPC turn", {0x1200B0}),
    ("scheduler", {0x1213DE}),
    ("key wait", {0x106BBC, 0x10790A}),
]


def procedures(dumps):
    return sorted(int(f[:-5], 16) for f in os.listdir(dumps) if f.startswith("0x"))


def hunk_sizes(path):
    """Each hunk's length in memory, read from hunk2raw's JSON at `path`; refused unless its
    hunks start at BASES."""
    segs = json.load(open(path))["segments"]
    num = lambda v: v if isinstance(v, int) else int(v, 16)  # noqa: E731
    if [num(s["base"]) for s in segs] != BASES:
        raise SystemExit(f"{path}: the hunks do not start where game-layout.md has them")
    return [num(s["size"]) for s in segs]


class Image:
    def __init__(self, bases, procs, sizes):
        self.bases, self.procs, self.sizes = bases, procs, sizes

    def function(self, addr):
        """The image function containing runtime address `addr`, or None outside the code."""
        for i in CODE:
            base, size = self.bases[i], self.sizes[i]
            if base <= addr < base + size:
                a = BASES[i] + addr - base
                k = bisect.bisect_right(self.procs, a) - 1
                return self.procs[k] if k >= 0 else None
        return None

    def chain(self, sample):
        """The routine of a sample and its callers, innermost first, repeats dropped."""
        _, pc, _, _, words = sample
        out = []
        for f in [self.function(pc)] + [self.function(v) for _, v in words]:
            if f is not None and (not out or out[-1] != f):
                out.append(f)
        return out


def kind(chain):
    for name, funcs in KINDS:
        if any(f in funcs for f in chain):
            return name
    return "other"


class Incomplete(Exception):
    """A capture that does not hold every emulated frame of a step, or a row outside the
    contract of fight.mjs's rows."""


def is_int(v):
    return isinstance(v, int) and not isinstance(v, bool)


def row_ok(r):
    """A fight.mjs row as written since 2026-10-16 (spec 01 §Rights): [frame, reading, frames
    skipped, view changed, scroll changed, panel changed, actors moved, actors hurt, sample,
    centre changed]: integers, booleans and counts, the sample None or [kind, pc, usp, a5,
    [[offset, address], ...]] (the emulated CPU's addresses). Nothing else: no picture hash,
    no centre, no actor list."""
    if not isinstance(r, list) or len(r) != 10:
        return False
    frame, reading, gap, view, scroll, panel, moved, hurt, sample, centre = r
    if not all(is_int(v) for v in (frame, reading, gap, moved, hurt)):
        return False
    if not all(isinstance(v, bool) for v in (view, scroll, panel, centre)):
        return False
    if sample is None:
        return True
    if not isinstance(sample, list) or len(sample) != 5 or not isinstance(sample[0], str):
        return False
    words = sample[4]
    return (all(is_int(v) for v in sample[1:4]) and isinstance(words, list)
            and all(isinstance(w, list) and len(w) == 2 and all(is_int(v) for v in w)
                    for w in words))


def frames_of(turn, image):
    """The frames of a step from the key taken (reading cleared) to the prompt: (frame, kind,
    chain, view changed, messages changed); kind 'other task' when another task's stack ran.

    Raises Incomplete unless the capture holds every emulated frame from before the key was
    taken (a row with the command loop still reading) through the turn to 60 frames of the
    prompt after it, one row each, none skipped, every row in fight.mjs's contract
    (`row_ok`: booleans and counts, no hash, centre or actor list; spec 01 §Rights)."""
    rows = turn["rows"]
    if not rows:
        raise Incomplete(f"step {turn['key']}: no rows")
    bad = sum(1 for r in rows if not row_ok(r))
    if bad:
        raise Incomplete(f"step {turn['key']}: {bad} rows outside fight.mjs's row contract")
    ids = [r[0] for r in rows]
    if ids != list(range(ids[0], ids[0] + len(ids))) or any(r[2] for r in rows):
        raise Incomplete(f"step {turn['key']}: frames {ids[0]}..{ids[-1]} not all captured")
    zero = [r[0] for r in rows if r[1] == 0]
    if not zero:
        raise Incomplete(f"step {turn['key']}: the game never took the key")
    lo, hi = zero[0], zero[-1]
    if rows[0][1] != 1 or rows[0][0] >= lo:
        raise Incomplete(f"step {turn['key']}: the capture starts after the key was taken")
    tail = [r for r in rows if r[0] > hi]
    if len(tail) < 60 or any(r[1] != 1 for r in tail):
        raise Incomplete(f"step {turn['key']}: the capture ends before the prompt settled")
    pages = collections.Counter(
        r[8][2] >> 12 for r in rows if r[8] and image.function(r[8][1]) is not None
    )
    page = pages.most_common(1)[0][0] if pages else None
    out = []
    for r in rows:
        # The view and the scroll changed against the frame before (fight.mjs compares them in
        # the emulator; the step's first row is false).
        vch, mch = r[3], r[4]
        if not lo <= r[0] <= hi:
            continue
        s = r[8]
        if not s:
            out.append((r[0], "no sample", [], vch, mch))
        elif abs((s[2] >> 12) - page) > 4:
            out.append((r[0], "other task", [], vch, mch))
        else:
            c = image.chain(s)
            out.append((r[0], kind(c), c, vch, mch))
    return out


def runs(frames, name):
    """Runs of consecutive frames (by frame id) of one kind."""
    out, cur = [], []
    for f in frames:
        if f[1] == name and (not cur or f[0] == cur[-1][0] + 1):
            cur.append(f)
            continue
        if cur:
            out.append(cur)
        cur = [f] if f[1] == name else []
    if cur:
        out.append(cur)
    return out


def length(run):
    """Frames a run spans, from its frame ids."""
    return run[-1][0] - run[0][0] + 1


def flight_phases(run):
    """(first render, steps, closing refresh, drawn steps) of one flight's frames."""
    i = 0
    while i < len(run) and 0x102C90 in run[i][2] and 0x102EF0 not in run[i][2]:
        i += 1
    j = i
    while j < len(run) and 0x102EF0 not in run[j][2]:
        j += 1
    return i, j - i, len(run) - j, sum(1 for f in run[i:j] if f[3])


def show(name, values):
    if values:
        print(
            f"  {name}: median {statistics.median(values):g}, n {len(values)}, "
            f"{min(values)}..{max(values)}"
        )


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dumps", default=DUMPS)
    ap.add_argument("--image-json", default=GAME_JSON)
    ap.add_argument("runs", nargs="+")
    a = ap.parse_args()
    procs = procedures(a.dumps)
    sizes = hunk_sizes(a.image_json)
    totals, per_step = collections.Counter(), []
    cost = collections.defaultdict(list)
    for path in a.runs:
        d = json.load(open(path))
        image = Image(d["bases"], procs, sizes)
        for n, turn in enumerate(d["turns"]):
            try:
                frames = frames_of(turn, image)
            except Incomplete as e:
                raise SystemExit(f"{path}: {e}") from None
            kinds = collections.Counter(f[1] for f in frames)
            totals.update(kinds)
            per_step.append(frames[-1][0] - frames[0][0] + 1)
            print(
                f"{path} step {n} {turn['key']}: {per_step[-1]} frames, other tasks "
                f"{kinds['other task']}: " + ", ".join(f"{k} {v}" for k, v in kinds.most_common())
            )
            cost["refresh after a turn"] += [length(r) for r in runs(frames, "refresh after a turn")]
            cost["hit flash"] += [length(r) for r in runs(frames, "hit flash")]
            for r in runs(frames, "flight"):
                first, steps, end, drawn = flight_phases(r)
                cost["flight start"].append(first)
                if end:
                    cost["flight end"].append(end)
                if drawn:
                    cost["flight step x100"].append(round(100 * steps / drawn))
                    cost["flight step frames"].append(steps)
                    cost["flight steps drawn"].append(drawn)
            cost["object name"] += [length(r) for r in runs(frames, "object name")]
            for r in runs(frames, "message"):
                cost["message frames"].append(length(r))
                cost["message changes"].append(sum(1 for f in r if f[4]))
            cost["NPC turns and scheduler per step"].append(
                kinds["NPC turn"] + kinds["scheduler"] + kinds["other"]
            )
    steps = len(per_step)
    print(f"\n{steps} steps: median {statistics.median(per_step):g} frames")
    print("frames per step by kind: " + ", ".join(
        f"{k} {v / steps:.1f}" for k, v in totals.most_common()))
    print("costs (PAL frames):")
    for k in sorted(cost):
        show(k, cost[k])
    for k in ("flight step", "message"):
        num = sum(cost[f"{k} frames"])
        den = sum(cost["flight steps drawn" if k == "flight step" else "message changes"])
        if den:
            print(f"  {k}: {num} frames / {den} = {num / den:.2f}")


if __name__ == "__main__":
    main()
