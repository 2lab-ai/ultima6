// Tests for tracecore.mjs (trace.mjs's stop bookkeeping): synthetic console text and stacks
// only, no emulator, no game data.
//
//   node tools/oracle/test_trace.mjs
import assert from 'node:assert/strict';
import { closeAt, stopPrompt } from './tracecore.mjs';

const tests = [];
const test = (name, fn) => tests.push({ name, fn });
const name = (fn) => `fn ${fn}`;
// An open call as trace.mjs pushes it at its entry: the routine, its return address, A7 there.
const call = (fn, ret, a7) => ({ fn, ret, a7 });
// A console prompt line as the debugger prints it: (beam line,column) $PC: and a disassembly.
const line = (v, pc) => `(${v},4) $${pc.toString(16)}: nop\n`;

test('the youngest call returning to its own address closes it', () => {
  const open = [call(1, 0x2a40, 0x6c00), call(2, 0x3a40, 0x6bf0)];
  const top = closeAt(open, 0x3a40, 0x6bf4, name);
  assert.equal(top.fn, 2);
  assert.deepEqual(open.map((c) => c.fn), [1]);
  assert.equal(closeAt(open, 0x2a40, 0x6c04, name).fn, 1);
  assert.equal(open.length, 0);
});

test('a stop at another return address closes nothing', () => {
  const open = [call(1, 0x2a40, 0x6c00)];
  assert.equal(closeAt(open, 0x2b40, 0x6c04, name), null);
  assert.equal(open.length, 1);
});

test('the return address reached with another stack closes nothing', () => {
  // The call site looped through again, or an outer frame of a recursion: A7 not back above
  // this call's return long.
  const open = [call(1, 0x2a40, 0x6c00)];
  assert.equal(closeAt(open, 0x2a40, 0x6c00, name), null);
  assert.equal(closeAt(open, 0x2a40, 0x6bfc, name), null);
  assert.equal(open.length, 1);
});

test('an older call returning with younger ones open is refused', () => {
  const open = [call(1, 0x2a40, 0x6c00), call(2, 0x3a40, 0x6bf0), call(3, 0x4a40, 0x6be0)];
  assert.throws(() => closeAt(open, 0x2a40, 0x6c04, name), /fn 1 returned with 2 younger calls open \(fn 2, fn 3\)/);
  assert.equal(open.length, 3, 'nothing closed');
});

test('no open call: a return address closes nothing', () => {
  assert.equal(closeAt([], 0x2a40, 0x6c04, name), null);
});

test('the stop is the last prompt past the mark', () => {
  const text = line(10, 0x1a40) + line(20, 0x1b40);
  assert.deepEqual(stopPrompt(text, 0), { v: 20, pc: 0x1b40 });
});

test('a prompt before the mark (the one the resume was typed at) is no stop', () => {
  const old = line(10, 0x1a40);
  assert.equal(stopPrompt(old + 'g\n', old.length), null);
  const text = old + 'g\n' + line(30, 0x1c40);
  assert.deepEqual(stopPrompt(text, old.length), { v: 30, pc: 0x1c40 });
});

test('no prompt at all is no stop', () => {
  assert.equal(stopPrompt('', 0), null);
  assert.equal(stopPrompt('clear\n', 0), null);
});

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`ok ${t.name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${t.name}: ${e.message}`);
  }
}
console.log(`${tests.length - failed} passed, ${failed} failed`);
if (failed) process.exitCode = 1;
