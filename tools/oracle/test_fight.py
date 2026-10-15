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
    """A fight.mjs row with no CPU sample: [frame, reading, skipped, view, scroll, panel,
    moved, hurt, sample, centre]."""
    return [frame, reading, gap, 1, 1, 1, [], [], None, [0, 0]]


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


def call(fn, at, frames, depth=1, actor=None, worktype=None, followers=None, **more):
    """A traced call; `more` adds a followers' pass's `strict` or a party step's `mp`."""
    return {"fn": fn, "at": at, "frames": frames, "depth": depth, "actor": actor,
            "worktype": worktype, "followers": followers, **more}


class TraceConstants(unittest.TestCase):
    def test_picks_npc_turns_and_refreshes(self):
        # One step: a world turn of 100 frames with two NPC turns (one combat, one schedule),
        # a refresh of 21 frames after the combat one and an unchanged view after the other;
        # a hit flash and a name read inside the combat turn.
        calls = [
            call(0x1213DE, 0.0, 100.0, depth=0),
            call(0x1200B0, 10.0, 40.0, actor=5, worktype=3),
            call(0x1230CE, 12.0, 15.5, depth=2),
            call(0x10EEF8, 30.0, 1.5, depth=2),
            call(0x102F2A, 50.0, 21.0),
            call(0x1200B0, 71.0, 0.02, actor=6, worktype=0x92),
            call(0x102F2A, 71.1, 0.0),
        ]
        c = trace.constants([{"key": "UP", "read": 0, "prompt": 160, "calls": calls}])
        self.assertEqual(c["refresh after a turn"]["n"], 1)
        self.assertAlmostEqual(c["refresh after a turn"]["median"], 21.0)
        self.assertAlmostEqual(c["combat NPC turn"]["mean"], 40.0 - 15.5 - 1.5)
        self.assertAlmostEqual(c["other NPC turn"]["mean"], 0.02)
        # Picks: the two NPC turns and the player's; outside them and the refreshes 100 -
        # 40 - 21 - 0.02 - 0 frames.
        self.assertAlmostEqual(c["scheduler pick"]["mean"], (100 - 40 - 21 - 0.02) / 3)
        self.assertAlmostEqual(c["hit flash"]["median"], 15.5)
        self.assertAlmostEqual(c["object name"]["mean"], 1.5)

    def test_npc_turns_are_combat_ones_by_the_routine_their_worktype_runs(self):
        # Worktypes 0x9b (assault, at 0x80 and up) and 0x13 run combat routines; 15 (a random
        # walk) and 12 (a wander, below 0x80) do not.
        calls = [call(0x1213DE, 0.0, 100.0, depth=0)]
        for i, (wt, frames) in enumerate([(0x9B, 3.0), (0x13, 1.0), (15, 0.5), (12, 0.25)]):
            calls.append(call(0x1200B0, 10.0 * (i + 1), frames, actor=10 + i, worktype=wt))
        c = trace.constants([{"key": "UP", "read": 0, "prompt": 160, "calls": calls}])
        self.assertEqual(c["combat NPC turn"]["n"], 2)
        self.assertAlmostEqual(c["combat NPC turn"]["mean"], 2.0)
        self.assertEqual(c["other NPC turn"]["n"], 2)
        self.assertAlmostEqual(c["other NPC turn"]["mean"], 0.375)

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

    def test_passs_strict_pass_is_counted_apart_from_the_steps(self):
        # A step whose pass (D1 0, 3 members) takes 4 frames; Pass's strict passes (D1 1) at
        # the top: one of 5 frames with 3 members, one of 0.2 with 3, one of 0.05 with none.
        calls = [
            call(0x11D4EE, 0.0, 5.0, depth=0),
            call(0x11C910, 0.5, 4.0, followers=3, strict=0),
        ]
        steps = [{"key": "UP", "read": 0, "prompt": 100, "calls": calls}]
        for at, frames, followers in ((10.0, 5.0, 3), (20.0, 0.2, 3), (30.0, 0.05, 0)):
            calls = [call(0x11C910, at, frames, depth=0, followers=followers, strict=1)]
            steps.append({"key": "SPACE", "read": 0, "prompt": 100, "calls": calls})
        c = trace.constants(steps)
        self.assertEqual(c["follow"]["n"], 1)
        self.assertAlmostEqual(c["follow"]["mean"], 4.0)
        self.assertEqual(c["follow, Pass"]["n"], 2)
        self.assertAlmostEqual(c["follow, Pass"]["mean"], 2.6)
        self.assertEqual(c["follow, Pass, no one to move"]["n"], 1)
        self.assertEqual(c["party step"]["n"], 1)

    def test_a_followers_pass_outside_a_step_without_its_d1_is_refused(self):
        # A trace from before D1 was recorded only had the steps' passes: one at the top is
        # neither a step's nor known to be Pass's.
        calls = [call(0x11C910, 0.0, 1.0, depth=0, followers=3)]
        with self.assertRaises(trace.Incomplete):
            trace.constants([{"key": "SPACE", "read": 0, "prompt": 100, "calls": calls}])

    def test_a_blocked_step_is_counted_apart_with_its_key_to_prompt(self):
        # A blocked step (no followers' pass inside it, the leader's points 10 before and
        # after) whose world turn of 1.5 frames has its one refresh and no NPC turn; its key
        # read 3 frames after the key. Then a step that moved, which is not blocked.
        blocked = [
            call(0x11D4EE, 0.0, 0.06, depth=0, mp=[10, 10]),
            call(0x1213DE, 0.1, 1.5, depth=0),
            call(0x102F2A, 1.5, 0.0),
        ]
        moved = [
            call(0x11D4EE, 0.0, 6.0, depth=0, mp=[15, 10]),
            call(0x11C910, 1.0, 4.0, followers=3, strict=0),
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
        self.assertEqual(c["blocked steps"], {"steps": 1, "points spent": [0], "NPC turns": 0})

    def test_a_blocked_step_without_its_prompt_frame_is_refused(self):
        calls = [call(0x11D4EE, 0.0, 0.06, depth=0)]
        with self.assertRaises(trace.Incomplete):
            trace.constants([{"key": "UP", "read": 0, "prompt": 100, "calls": calls}])


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
