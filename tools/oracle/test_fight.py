#!/usr/bin/env python3
"""Tests for fight.py (frames of a step) and trace.py (constants from a trace): synthetic
captures only, no game data.

Run: python3 -I -B tools/oracle/test_fight.py
"""
import importlib.util
import os
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


IMAGE = fight.Image([0] * len(fight.HUNKS), [0x100000])


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

    def test_runs_break_at_a_gap_in_frame_numbers(self):
        frames = [(1, "flight", [], False, False), (2, "flight", [], False, False),
                  (4, "flight", [], False, False)]
        self.assertEqual([fight.length(r) for r in fight.runs(frames, "flight")], [2, 1])


def call(fn, at, frames, depth=1, actor=None, worktype=None):
    return {"fn": fn, "at": at, "frames": frames, "depth": depth, "actor": actor,
            "worktype": worktype}


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
        self.assertAlmostEqual(c["schedule NPC turn"]["mean"], 0.02)
        # Picks: the two NPC turns and the player's; outside them and the refreshes 100 -
        # 40 - 21 - 0.02 - 0 frames.
        self.assertAlmostEqual(c["scheduler pick"]["mean"], (100 - 40 - 21 - 0.02) / 3)
        self.assertAlmostEqual(c["hit flash"]["median"], 15.5)
        self.assertAlmostEqual(c["object name"]["mean"], 1.5)


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
