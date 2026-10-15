"""The port's timing constants from trace.mjs runs (trace.json: every traced call with its PAL
frames): what actors/turn.rs and combat/present.rs take, rebuilt from the oracle's data.

usage: python3 -I -B trace.py RUN/trace.json ...
Prints per constant n, median, mean and range in PAL frames, and the ms (20 a frame) the port
uses: the median where one call is one cost, the mean where the port pays one cost per call of
a varying routine (messages, names, NPC turns, picks).
"""

import json
import statistics
import sys

WORLD_TURN, NPC_TURN, AFTER_TURN = 0x1213DE, 0x1200B0, 0x102F2A
FLIGHT, HIT_FLASH, PRINTF, NAME = 0x132706, 0x1230CE, 0x108658, 0x10EEF8
# Called inside an NPC turn and paid as costs of their own.
OWN_COSTS = (FLIGHT, HIT_FLASH, PRINTF, NAME)
# A name read longer than this loaded look.lzd from the floppy (the boot's first): not a cost
# the port pays.
FLOPPY_NAME = 20.0


def stats(values):
    if not values:
        return {"n": 0}
    return {
        "n": len(values),
        "median": statistics.median(values),
        "mean": statistics.mean(values),
        "min": min(values),
        "max": max(values),
    }


def inside(outer, c):
    return outer["at"] <= c["at"] and c["at"] + c["frames"] <= outer["at"] + outer["frames"]


def constants(steps):
    """Per constant, the statistics of its traced calls over `steps` (trace.json's steps)."""
    out = {k: [] for k in ("refresh after a turn", "combat NPC turn", "schedule NPC turn",
                           "scheduler pick", "hit flash", "flight", "flight without a step",
                           "message", "object name")}
    pick_frames = picks = 0.0
    for s in steps:
        calls = s["calls"]
        for c in calls:
            if c["fn"] == AFTER_TURN and c["frames"] > 1:
                out["refresh after a turn"].append(c["frames"])
            elif c["fn"] == HIT_FLASH:
                out["hit flash"].append(c["frames"])
            elif c["fn"] == FLIGHT:
                out["flight"].append(c["frames"])
                if c["frames"] < 8:
                    out["flight without a step"].append(c["frames"])
            elif c["fn"] == PRINTF:
                out["message"].append(c["frames"])
            elif c["fn"] == NAME and c["frames"] < FLOPPY_NAME:
                out["object name"].append(c["frames"])
        for world in (c for c in calls if c["fn"] == WORLD_TURN):
            turns = [c for c in calls if c["fn"] == NPC_TURN and inside(world, c)]
            refreshes = [c for c in calls if c["fn"] == AFTER_TURN and inside(world, c)]
            for n in turns:
                own = sum(c["frames"] for c in calls if c is not n and c["fn"] in OWN_COSTS
                          and c["depth"] == n["depth"] + 1 and inside(n, c))
                key = "combat NPC turn" if n["worktype"] < 0x80 else "schedule NPC turn"
                out[key].append(n["frames"] - own)
            pick_frames += world["frames"] - sum(c["frames"] for c in turns + refreshes)
            picks += len(turns) + 1
    res = {k: stats(v) for k, v in out.items()}
    res["scheduler pick"] = {"n": int(picks), "mean": pick_frames / picks if picks else 0.0}
    return res


# The constant each line feeds, and whether the port pays the median or the mean.
USES = {
    "refresh after a turn": ("actors/turn.rs AFTER_TURN_MS", "median"),
    "scheduler pick": ("actors/turn.rs PICK_MS", "mean"),
    "combat NPC turn": ("actors/turn.rs COMBAT_NPC_MS", "mean"),
    "schedule NPC turn": ("(no wait)", "mean"),
    "hit flash": ("combat/present.rs FLASH_MS", "median"),
    "flight without a step": ("combat/present.rs RENDER_MS", "median"),
    "message": ("combat/present.rs PRINT_MS", "mean"),
    "object name": ("combat/present.rs NAME_MS", "mean"),
    "flight": ("(start + steps + end; check only)", "median"),
}


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    steps = []
    for path in sys.argv[1:]:
        steps += json.load(open(path))["steps"]
    for k, v in constants(steps).items():
        use, how = USES[k]
        if not v.get("n"):
            print(f"{k}: no calls")
            continue
        val = v[how]
        rng = f", {v['min']:.2f}..{v['max']:.2f}" if "min" in v else ""
        print(f"{k}: n {v['n']}, {how} {val:.3f} frames{rng} -> {round(val * 20)} ms ({use})")


if __name__ == "__main__":
    main()
