"""The port's timing constants from trace.mjs runs (trace.json: every traced call with its PAL
frames): what actors/turn.rs, actors/walk.rs and combat/present.rs take, rebuilt from the
oracle's data.

usage: python3 -I -B trace.py RUN/trace.json ...
Refuses a file of a run that did not finish (trace.mjs writes after every step and marks the
file complete only once every key's step is in), a followers' pass without its count of
followers, a followers' pass outside a party step without its D1 (Pass's is strict), and a
blocked step without the frame its prompt came back. Prints per constant n, median, mean and
range in PAL frames, and the ms (20 a frame) the port uses: the median where one call is one
cost, the mean where the port pays one cost per call of a varying routine (messages, names, NPC
turns, picks, steps, followers' passes). A party step that returned before its followers' pass
(blocked: no followers' pass inside it) is counted apart from the steps that reached it, with the
key-to-prompt frames of its command and the leader's points spent in it.
"""

import json
import statistics
import sys

WORLD_TURN, NPC_TURN, AFTER_TURN = 0x1213DE, 0x1200B0, 0x102F2A
FLIGHT, HIT_FLASH, PRINTF, NAME = 0x132706, 0x1230CE, 0x108658, 0x10EEF8
# The party step and the followers' pass it ends in.
STEP, FOLLOW = 0x11D4EE, 0x11C910
# The worktypes 0x1200b0 runs through a combat routine (its dispatch 0x120184..0x120248; the
# port's actors/npc.rs runs_combat_routine): the NPC turns COMBAT_NPC_MS is the time of.
COMBAT_ROUTINES = frozenset([*range(3, 12), 13, 14, 0x13, 0x9A, 0x9B])
# Called inside an NPC turn and paid as costs of their own.
OWN_COSTS = (FLIGHT, HIT_FLASH, PRINTF, NAME)
# A name read or a step longer than this waited on the floppy (the boot's first name read loads
# look.lzd, its first step a map region): not a cost the port pays.
FLOPPY = 20.0


class Incomplete(Exception):
    """A trace.json of a run that did not trace every one of its keys, or that lacks a field a
    constant needs."""


def steps_of(doc):
    """The steps of a trace.json `doc`, refused (Incomplete) unless the run marked it complete
    with one step per key, in the keys' order."""
    if doc.get("complete") is not True:
        raise Incomplete("not marked complete: the run stopped before its last step")
    keys, steps = doc.get("keys"), doc.get("steps", [])
    if not isinstance(keys, list) or [s.get("key") for s in steps] != keys:
        raise Incomplete(f"{len(steps)} steps for {len(keys or [])} keys: not one step per key")
    return steps


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


def nested(outer, calls, fn):
    """The calls of `fn` one level inside `outer`."""
    return [c for c in calls if c["fn"] == fn and c["depth"] == outer["depth"] + 1
            and inside(outer, c)]


def strict_pass(c, calls):
    """Whether followers' pass `c` is Pass's strict one (D1 = 1, 0x104c0a) rather than a party
    step's (D1 = 0). A trace without D1 (before trace.mjs recorded it) only had the steps' passes:
    one inside a party step is a step's; one outside, there, is refused."""
    if c.get("strict") is not None:
        return bool(c["strict"])
    if any(s["fn"] == STEP and s["depth"] == c["depth"] - 1 and inside(s, c) for s in calls):
        return False
    raise Incomplete("a followers' pass outside a party step without its D1")


def constants(steps):
    """Per constant, the statistics of its traced calls over `steps` (trace.json's steps), and
    under "blocked steps" what the steps whose party step was blocked spent and ran."""
    out = {k: [] for k in ("refresh after a turn", "combat NPC turn", "other NPC turn",
                           "scheduler pick", "hit flash", "flight", "flight without a step",
                           "message", "object name", "party step", "party step, blocked",
                           "follow", "follow, no one to move", "follow, Pass",
                           "follow, Pass, no one to move", "blocked step, world turn",
                           "blocked step, key to prompt")}
    blocked = {"steps": 0, "points spent": [], "NPC turns": 0}
    pick_frames = picks = 0.0
    for s in steps:
        calls = s["calls"]
        party_steps = [c for c in calls if c["fn"] == STEP]
        if party_steps and not any(nested(c, calls, FOLLOW) for c in party_steps):
            # Every party step of the command returned before its followers' pass.
            if s.get("back") is None:
                raise Incomplete("a blocked step without the frame its prompt came back")
            blocked["steps"] += 1
            out["blocked step, key to prompt"].append(s["back"] - s["read"])
            for c in party_steps:
                if c.get("mp"):
                    blocked["points spent"].append(c["mp"][0] - c["mp"][-1])
            for w in (c for c in calls if c["fn"] == WORLD_TURN):
                out["blocked step, world turn"].append(w["frames"])
                blocked["NPC turns"] += len(nested(w, calls, NPC_TURN))
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
            elif c["fn"] == NAME and c["frames"] < FLOPPY:
                out["object name"].append(c["frames"])
            elif c["fn"] == STEP and c["frames"] < FLOPPY:
                # Its own time: the followers' pass and what it prints and names are paid apart.
                own = sum(x["frames"] for x in calls if x is not c
                          and x["fn"] in (FOLLOW, *OWN_COSTS)
                          and x["depth"] == c["depth"] + 1 and inside(c, x))
                key = "party step" if nested(c, calls, FOLLOW) else "party step, blocked"
                out[key].append(c["frames"] - own)
            elif c["fn"] == FOLLOW:
                if c.get("followers") is None:
                    raise Incomplete("a followers' pass without its count of followers")
                key = "follow, Pass" if strict_pass(c, calls) else "follow"
                out[key if c["followers"] else f"{key}, no one to move"].append(c["frames"])
        for world in (c for c in calls if c["fn"] == WORLD_TURN):
            turns = [c for c in calls if c["fn"] == NPC_TURN and inside(world, c)]
            refreshes = [c for c in calls if c["fn"] == AFTER_TURN and inside(world, c)]
            for n in turns:
                own = sum(c["frames"] for c in calls if c is not n and c["fn"] in OWN_COSTS
                          and c["depth"] == n["depth"] + 1 and inside(n, c))
                key = "combat NPC turn" if n["worktype"] in COMBAT_ROUTINES else "other NPC turn"
                out[key].append(n["frames"] - own)
            pick_frames += world["frames"] - sum(c["frames"] for c in turns + refreshes)
            picks += len(turns) + 1
    res = {k: stats(v) for k, v in out.items()}
    res["scheduler pick"] = {"n": int(picks), "mean": pick_frames / picks if picks else 0.0}
    res["blocked steps"] = blocked
    return res


# The constant each line feeds, and whether the port pays the median or the mean.
USES = {
    "refresh after a turn": ("actors/turn.rs AFTER_TURN_MS", "median"),
    "scheduler pick": ("actors/turn.rs PICK_MS", "mean"),
    "combat NPC turn": ("actors/turn.rs COMBAT_NPC_MS", "mean"),
    "other NPC turn": ("(no wait)", "mean"),
    "hit flash": ("combat/present.rs FLASH_MS", "median"),
    "flight without a step": ("combat/present.rs RENDER_MS", "median"),
    "message": ("combat/present.rs PRINT_MS", "mean"),
    "object name": ("combat/present.rs NAME_MS", "mean"),
    "party step": ("actors/walk.rs STEP_MS", "mean"),
    "party step, blocked": ("(no wait)", "mean"),
    "follow": ("actors/walk.rs FOLLOW_MS", "mean"),
    "follow, no one to move": ("(no wait)", "mean"),
    "follow, Pass": ("actors/walk.rs PASS_FOLLOW_MS", "mean"),
    "follow, Pass, no one to move": ("(no wait)", "mean"),
    "blocked step, world turn": ("(check: one pick, actors/turn.rs PICK_MS)", "mean"),
    "blocked step, key to prompt": ("(check: the frame counter's, actors/walk.rs tests)", "mean"),
    "flight": ("(start + steps + end; check only)", "median"),
}


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    steps = []
    for path in sys.argv[1:]:
        try:
            run = steps_of(json.load(open(path)))
            constants(run)
        except Incomplete as e:
            raise SystemExit(f"{path}: {e}") from None
        steps += run
    res = constants(steps)
    blocked = res.pop("blocked steps")
    for k, v in res.items():
        use, how = USES[k]
        if not v.get("n"):
            print(f"{k}: no calls")
            continue
        val = v[how]
        rng = f", {v['min']:.2f}..{v['max']:.2f}" if "min" in v else ""
        print(f"{k}: n {v['n']}, {how} {val:.3f} frames{rng} -> {round(val * 20)} ms ({use})")
    if blocked["steps"]:
        spent = sorted(set(blocked["points spent"]))
        print(f"blocked steps: {blocked['steps']}, the leader's points spent in the step "
              f"{spent} (n {len(blocked['points spent'])}), NPC turns in their world turns "
              f"{blocked['NPC turns']}")


if __name__ == "__main__":
    main()
