// The bookkeeping of trace.mjs without the emulator: which console prompt is the current stop,
// and what a stop at a traced routine's return address does to the calls open there. Pure
// functions, tested by test_trace.mjs.

// The console prompt of the current stop: the last one printed at or past `mark` (the console's
// length right after the last resume was typed). The emulator does not run while a command is
// typed, so a prompt past the mark is a new stop, and the one the resume was typed at, which
// shows the old PC, is never taken for it. `{ v: beam line, pc }`, or null when none is past it.
export function stopPrompt(text, mark) {
  const m = [...text.matchAll(/\((\d+),(\d+)\) \$([0-9a-f]+):/g)].filter((x) => x.index >= mark).at(-1);
  return m ? { v: Number(m[1]), pc: parseInt(m[3], 16) } : null;
}

// A stop at `pc` with the stack at `a7`, `pc` being a return address of a traced call: only the
// youngest open call (the last of `open`, each `{ ret, a7 }` as its entry saw them) may return,
// to its own return address with A7 back above its return long. That call is taken off `open`
// and returned. That address reached otherwise (a loop through the call site, a recursion's
// outer frame) is no return: null, `open` unchanged. An older call returning with younger ones
// open is a trace gone wrong: throws, naming the calls by `name(fn)`.
export function closeAt(open, pc, a7, name) {
  const top = open.at(-1);
  if (top && top.ret === pc && top.a7 + 4 === a7) return open.pop();
  const k = open.findIndex((c) => c.ret === pc && c.a7 + 4 === a7);
  if (k >= 0) {
    const younger = open.slice(k + 1).map((c) => name(c.fn));
    throw new Error(`${name(open[k].fn)} returned with ${younger.length} younger calls open (${younger.join(', ')})`);
  }
  return null;
}
