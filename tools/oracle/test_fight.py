#!/usr/bin/env python3
"""Tests for fight.py (frames of a step) and trace.py (constants from a trace): synthetic
captures only, no game data.

Run: python3 -I -B tools/oracle/test_fight.py
"""
import importlib.util
import json
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f"{name}.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


fight = load("fight")
trace = load("trace")


def row(frame, reading, gap=0):
    """A fight.mjs row with no CPU sample: [frame, reading, skipped, view changed, scroll
    changed, panel changed, actors moved, actors hurt, sample, centre changed]."""
    return [frame, reading, gap, False, False, False, 0, 0, None, False]


def step(before=5, turn=30, after=60, first=1000):
    """A capture: `before` frames still reading, `turn` frames of the turn, `after` reading."""
    rows, f = [], first
    for n, reading in ((before, 1), (turn, 0), (after, 1)):
        for _ in range(n):
            rows.append(row(f, reading))
            f += 1
    return {"key": "UP", "pressedAt": first, "rows": rows}


IMAGE = fight.Image([0] * len(fight.BASES), [0x100000], [0x100] * len(fight.BASES))


class FramesOfStep(unittest.TestCase):
    def test_a_whole_capture_gives_the_turns_frames(self):
        frames = fight.frames_of(step(), IMAGE)
        self.assertEqual(len(frames), 30)
        self.assertEqual(frames[0][0], 1005)

    def test_a_capture_that_starts_after_the_key_was_taken_is_refused(self):
        with self.assertRaises(fight.Incomplete):
            fight.frames_of(step(before=0), IMAGE)

    def test_a_capture_with_a_frame_missing_is_refused(self):
        s = step()
        del s["rows"][20]
        with self.assertRaises(fight.Incomplete):
            fight.frames_of(s, IMAGE)

    def test_a_skipped_frame_in_the_settling_tail_is_refused(self):
        s = step()
        s["rows"][-10][2] = 1
        with self.assertRaises(fight.Incomplete):
            fight.frames_of(s, IMAGE)

    def test_a_row_outside_the_contract_is_refused(self):
        # fight.mjs writes booleans and counts, never a picture hash, the view centre or the
        # actors (spec 01 §Rights); a capture with any such row, or a malformed one, is refused.
        self.assertEqual(len(fight.frames_of(step(), IMAGE)), 30)
        for name, field, value in [
            ("a view hash", 3, 1743465692),
            ("a scroll hash", 4, 1945367669),
            ("a panel hash", 5, 4264608019),
            ("the moved actors' list", 6, [10, 11]),
            ("the hurt actors with their hit points", 7, [[10, 30, 24]]),
            ("the view centre", 9, [307, 352]),
            ("a count as a flag", 6, True),
            ("a sample without its words", 8, ["U", 0x2EF000, 0x358B58, 0x358B60]),
        ]:
            t = step()
            t["rows"][3][field] = value
            with self.assertRaises(fight.Incomplete, msg=name):
                fight.frames_of(t, IMAGE)
        t = step()
        t["rows"][3] = t["rows"][3][:9]
        with self.assertRaises(fight.Incomplete, msg="an early 9-field row"):
            fight.frames_of(t, IMAGE)

    def test_a_capture_that_ends_before_the_prompt_settled_is_refused(self):
        with self.assertRaises(fight.Incomplete):
            fight.frames_of(step(after=59), IMAGE)

    def test_hunk_lengths_come_from_the_json_when_its_bases_match(self):
        # Synthetic lengths: the n-th hunk n * 0x10 + 8 bytes, its base given as int or hex.
        segs = [{"base": b if i % 2 else f"0x{b:08x}", "size": i * 0x10 + 8}
                for i, b in enumerate(fight.BASES)]
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "image.json")
            with open(path, "w") as f:
                json.dump({"segments": segs}, f)
            self.assertEqual(fight.hunk_sizes(path), [i * 0x10 + 8 for i in range(len(segs))])
            segs[3]["base"] = fight.BASES[3] + 8
            with open(path, "w") as f:
                json.dump({"segments": segs}, f)
            with self.assertRaises(SystemExit):
                fight.hunk_sizes(path)

    def test_runs_break_at_a_gap_in_frame_numbers(self):
        frames = [(1, "flight", [], False, False), (2, "flight", [], False, False),
                  (4, "flight", [], False, False)]
        self.assertEqual([fight.length(r) for r in fight.runs(frames, "flight")], [2, 1])


def call(fn, at, frames, depth=1, followers=None, **more):
    """A traced call; `more` adds an NPC turn's `combat`, a followers' pass's `strict` and
    `moved`, a party step's `spent`."""
    return {"fn": fn, "at": at, "frames": frames, "depth": depth, "followers": followers, **more}


class TraceConstants(unittest.TestCase):
    def test_picks_npc_turns_and_refreshes(self):
        # One step: a world turn of 100 frames with two NPC turns (one combat, one schedule),
        # a refresh of 21 frames after the combat one and an unchanged view after the other;
        # a hit flash and a name read inside the combat turn.
        calls = [
            call(0x1213DE, 0.0, 100.0, depth=0),
            call(0x1200B0, 10.0, 40.0, combat=True),
            call(0x1230CE, 12.0, 15.5, depth=2),
            call(0x10EEF8, 30.0, 1.5, depth=2),
            call(0x102F2A, 50.0, 21.0),
            call(0x1200B0, 71.0, 0.02, combat=False),
            call(0x102F2A, 71.1, 0.0),
        ]
        c = trace.constants([{"key": "UP", "read": 0, "prompt": 160, "calls": calls}])
        self.assertEqual(c["refresh after a turn"]["n"], 1)
        self.assertAlmostEqual(c["refresh after a turn"]["median"], 21.0)
        self.assertAlmostEqual(c["combat NPC turn"]["mean"], 40.0 - 15.5 - 1.5)
        self.assertAlmostEqual(c["other NPC turn"]["mean"], 0.02)
        # Picks: the two NPC turns and the player's; outside them and the refreshes 100 -
        # 40 - 21 - 0.02 - 0 frames. No setup traced: no passes apart.
        self.assertAlmostEqual(c["scheduler pick, setup and minutes in"]["mean"],
                               (100 - 40 - 21 - 0.02) / 3)
        self.assertEqual(c["scheduler pass"]["n"], 0)
        self.assertAlmostEqual(c["hit flash"]["median"], 15.5)
        self.assertAlmostEqual(c["object name"]["mean"], 1.5)

    def test_the_setup_and_clock_minutes_are_counted_apart_from_the_passes(self):
        # A world turn of 50 frames: its setup (0x121056, 0x120b64, 0x120a1c ending at 1.0),
        # an NPC turn of 0.5, the bookkeeping after it, a clock minute of 10 with a 0.75 print
        # inside, a refresh of 21; picks: the NPC turn and the player's, plus the pass after
        # the minute's refill. A second turn, the first since the boot (its setup 8 frames, a
        # one-off): left out of the setup's cost, still taken out of its passes.
        calls = [
            call(0x1213DE, 0.0, 50.0, depth=0),
            call(0x121056, 0.0, 0.01),
            call(0x120B64, 0.1, 0.5),
            call(0x120A1C, 0.7, 0.3),
            call(0x1200B0, 2.0, 0.5, combat=False),
            call(0x121056, 2.6, 0.01),
            call(0x10368A, 5.0, 10.0),
            call(0x108658, 14.0, 0.75, depth=2),
            call(0x102F2A, 20.0, 21.0),
        ]
        one_off = [
            call(0x1213DE, 0.0, 10.0, depth=0),
            call(0x120A1C, 0.5, 7.5),
        ]
        c = trace.constants([
            {"key": "UP", "read": 0, "prompt": 160, "since_boot": False, "calls": calls},
            {"key": "UP", "read": 0, "prompt": 160, "since_boot": True, "calls": one_off},
        ])
        self.assertEqual(c["world turn setup"]["n"], 1)
        self.assertAlmostEqual(c["world turn setup"]["mean"], 1.0)
        self.assertEqual(c["clock minute"]["n"], 1)
        self.assertAlmostEqual(c["clock minute"]["mean"], 10.0 - 0.75)
        # Passes: 3 in the first turn (50 - 1 - 0.5 - 10 - 21 frames), 1 in the second
        # (10 - 8).
        self.assertEqual(c["scheduler pass"]["n"], 4)
        self.assertAlmostEqual(c["scheduler pass"]["mean"], ((50 - 1 - 0.5 - 10 - 21) + 2) / 4)

    def test_the_first_setup_since_the_boot_is_known_by_the_run_or_refused(self):
        # steps_of marks the first step of a fresh run (no command before its first key) as the
        # first since the boot, of another run as not; a run that does not say leaves it
        # unknown, and a setup traced there is refused.
        turn = [call(0x1213DE, 0.0, 10.0, depth=0), call(0x120A1C, 0.5, 0.5)]
        for fresh, first in ((True, True), (False, False)):
            d = doc(["UP", "UP"], 2)
            d["fresh"] = fresh
            self.assertEqual([s["since_boot"] for s in trace.steps_of(d)], [first, False])
        d = doc(["UP", "UP"], 2)
        for s in d["steps"]:
            s["calls"] = [dict(c) for c in turn]
        steps = trace.steps_of(d)
        self.assertEqual([s["since_boot"] for s in steps], [None, False])
        with self.assertRaises(trace.Incomplete):
            trace.constants(steps)
        self.assertEqual(trace.constants(steps[1:])["world turn setup"]["n"], 1)

    def test_npc_turns_are_combat_ones_by_their_flag_and_one_without_it_is_refused(self):
        calls = [call(0x1213DE, 0.0, 100.0, depth=0)]
        for i, (combat, frames) in enumerate([(True, 3.0), (True, 1.0), (False, 0.5),
                                              (False, 0.25)]):
            calls.append(call(0x1200B0, 10.0 * (i + 1), frames, combat=combat))
        c = trace.constants([{"key": "UP", "read": 0, "prompt": 160, "calls": calls}])
        self.assertEqual(c["combat NPC turn"]["n"], 2)
        self.assertAlmostEqual(c["combat NPC turn"]["mean"], 2.0)
        self.assertEqual(c["other NPC turn"]["n"], 2)
        self.assertAlmostEqual(c["other NPC turn"]["mean"], 0.375)
        del calls[1]["combat"]
        with self.assertRaises(trace.Incomplete):
            trace.constants([{"key": "UP", "read": 0, "prompt": 160, "calls": calls}])

    def test_a_party_step_pays_its_followers_pass_and_messages_apart(self):
        # A step of 6 frames whose followers' pass (3 members to move) takes 4 and a message
        # 0.25; a step of 0.75 whose pass has no one to move (0.05); a step that waited on the
        # floppy (80 frames), left out.
        calls = [
            call(0x11D4EE, 0.0, 6.0, depth=0),
            call(0x11C910, 1.0, 4.0, followers=3),
            call(0x108658, 5.5, 0.25),
            call(0x11D4EE, 10.0, 0.75, depth=0),
            call(0x11C910, 10.5, 0.05, followers=0),
            call(0x11D4EE, 20.0, 80.0, depth=0),
        ]
        c = trace.constants([{"key": "UP", "read": 0, "prompt": 160, "calls": calls}])
        self.assertEqual(c["party step"]["n"], 2)
        self.assertAlmostEqual(c["party step"]["mean"], ((6.0 - 4.0 - 0.25) + (0.75 - 0.05)) / 2)
        self.assertEqual(c["follow"]["n"], 1)
        self.assertAlmostEqual(c["follow"]["mean"], 4.0)
        self.assertEqual(c["follow, no one to move"]["n"], 1)
        self.assertAlmostEqual(c["follow, no one to move"]["mean"], 0.05)
        self.assertAlmostEqual(c["message"]["mean"], 0.25)

    def test_a_followers_pass_without_its_count_is_refused(self):
        calls = [call(0x11D4EE, 0.0, 2.0, depth=0), call(0x11C910, 0.5, 1.0)]
        del calls[1]["followers"]
        with self.assertRaises(trace.Incomplete):
            trace.constants([{"key": "UP", "read": 0, "prompt": 100, "calls": calls}])

    def test_passs_strict_pass_is_counted_apart_by_whether_it_moved_anyone(self):
        # A step whose pass (D1 0, 3 members) takes 4 frames; Pass's strict passes (D1 1) at
        # the top: two that moved a member (5 and 4.5 frames), one that moved none (0.2), one
        # with no one to move (0.05). Then one with members to move but no `moved`: refused.
        calls = [
            call(0x11D4EE, 0.0, 5.0, depth=0, spent=True),
            call(0x11C910, 0.5, 4.0, followers=3, strict=0, moved=True),
        ]
        steps = [{"key": "UP", "read": 0, "prompt": 100, "calls": calls}]
        for at, frames, followers, moved in ((10.0, 5.0, 3, True), (15.0, 4.5, 3, True),
                                             (20.0, 0.2, 3, False), (30.0, 0.05, 0, False)):
            calls = [call(0x11C910, at, frames, depth=0, followers=followers, strict=1,
                          moved=moved)]
            steps.append({"key": "SPACE", "read": 0, "prompt": 100, "calls": calls})
        c = trace.constants(steps)
        self.assertEqual(c["follow"]["n"], 1)
        self.assertAlmostEqual(c["follow"]["mean"], 4.0)
        self.assertEqual(c["follow, Pass, moved"]["n"], 2)
        self.assertAlmostEqual(c["follow, Pass, moved"]["mean"], 4.75)
        self.assertEqual(c["follow, Pass, none moved"]["n"], 1)
        self.assertAlmostEqual(c["follow, Pass, none moved"]["mean"], 0.2)
        self.assertEqual(c["follow, Pass, no one to move"]["n"], 1)
        self.assertEqual(c["party step"]["n"], 1)
        del steps[2]["calls"][0]["moved"]
        with self.assertRaises(trace.Incomplete):
            trace.constants(steps)

    def test_a_followers_pass_outside_a_step_without_its_d1_is_refused(self):
        # A trace from before D1 was recorded only had the steps' passes: one at the top is
        # neither a step's nor known to be Pass's.
        calls = [call(0x11C910, 0.0, 1.0, depth=0, followers=3)]
        with self.assertRaises(trace.Incomplete):
            trace.constants([{"key": "SPACE", "read": 0, "prompt": 100, "calls": calls}])

    def test_a_blocked_step_is_counted_apart_with_its_key_to_prompt(self):
        # A blocked step (no followers' pass inside it, the leader's points not spent) whose
        # world turn of 1.5 frames has its one refresh and no NPC turn; its key read 3 frames
        # after the key. Then a step that moved, which is not blocked.
        blocked = [
            call(0x11D4EE, 0.0, 0.06, depth=0, spent=False),
            call(0x1213DE, 0.1, 1.5, depth=0),
            call(0x102F2A, 1.5, 0.0),
        ]
        moved = [
            call(0x11D4EE, 0.0, 6.0, depth=0, spent=True),
            call(0x11C910, 1.0, 4.0, followers=3, strict=0, moved=True),
        ]
        c = trace.constants([
            {"key": "UP", "read": 10, "back": 13, "prompt": 73, "calls": blocked},
            {"key": "DOWN", "read": 80, "back": 110, "prompt": 170, "calls": moved},
        ])
        self.assertEqual(c["party step, blocked"]["n"], 1)
        self.assertAlmostEqual(c["party step, blocked"]["mean"], 0.06)
        self.assertEqual(c["party step"]["n"], 1)
        self.assertAlmostEqual(c["party step"]["mean"], 2.0)
        self.assertEqual(c["blocked step, key to prompt"]["max"], 3)
        self.assertAlmostEqual(c["blocked step, world turn"]["mean"], 1.5)
        self.assertEqual(c["blocked steps"],
                         {"steps": 1, "party steps": 1, "points spent": 0, "NPC turns": 0})

    def test_a_blocked_step_without_its_prompt_frame_or_its_points_flag_is_refused(self):
        calls = [call(0x11D4EE, 0.0, 0.06, depth=0, spent=False)]
        with self.assertRaises(trace.Incomplete):
            trace.constants([{"key": "UP", "read": 0, "prompt": 100, "calls": calls}])
        for spent in (None, 0, [10, 10]):
            calls = [call(0x11D4EE, 0.0, 0.06, depth=0, spent=spent)]
            with self.assertRaises(trace.Incomplete):
                trace.constants([{"key": "UP", "read": 0, "back": 2, "prompt": 100,
                                  "calls": calls}])


def doc(keys, done, complete=True):
    """A trace.json: the steps of the first `done` of `keys`, and its completion flag."""
    steps = [{"key": k, "read": 0, "prompt": 100, "calls": []} for k in keys[:done]]
    return {"bases": [0], "keys": keys, "steps": steps, "complete": complete}


class TraceFile(unittest.TestCase):
    def test_a_complete_run_gives_its_steps(self):
        self.assertEqual(len(trace.steps_of(doc(["UP", "LEFT"], 2))), 2)

    def test_a_run_written_before_its_end_is_refused(self):
        with self.assertRaises(trace.Incomplete):
            trace.steps_of(doc(["UP", "LEFT"], 1, complete=False))

    def test_a_file_without_the_completion_flag_is_refused(self):
        d = doc(["UP", "LEFT"], 2)
        del d["complete"]
        with self.assertRaises(trace.Incomplete):
            trace.steps_of(d)

    def test_a_complete_run_missing_a_step_is_refused(self):
        with self.assertRaises(trace.Incomplete):
            trace.steps_of(doc(["UP", "LEFT"], 1))

    def test_steps_of_other_keys_are_refused(self):
        d = doc(["UP", "LEFT"], 2)
        d["steps"][1]["key"] = "RIGHT"
        with self.assertRaises(trace.Incomplete):
            trace.steps_of(d)

    def test_a_file_without_its_keys_is_refused(self):
        d = doc(["UP"], 1)
        del d["keys"]
        with self.assertRaises(trace.Incomplete):
            trace.steps_of(d)


if __name__ == "__main__":
    unittest.main()
