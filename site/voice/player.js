// The page's voices (spec 09 Design 3): the worker that runs the model (site/voice/worker.js) and
// the player of what it makes, on the page's AudioContext through the voice gain beside the
// game's.
//
// - One synthesis at a time: an utterance waits until the one before it is spoken out of the
//   worker. A newer turn (the player's next input in a conversation) or `cancel` (Enter, Escape,
//   Space: the more key and the answer keys) ends the old speech with an 80 ms fade and empties
//   the queue. Speech never makes the game wait: the engine's queue is only read.
// - Above 4x (and at Max) nothing is synthesized: the lines go by faster than they can be spoken.
// - Steps adapt: halved while synthesis takes more than half the audio's length, doubled back
//   below 0.15 of it. On the GPU from 16, 4 to 16; on the CPU (no WebGPU adapter) from 4, 3 to
//   16: 16 steps there take twice the audio's length, 3 under half of it, and below 3 words are
//   lost (spec 09 Measured facts).
// - The seed of an utterance is a hash of its speaker and its text, so a line repeated sounds the
//   same.
//
// Rights (spec 01): an utterance's text is the user's game text. It goes to the worker and
// nowhere else: not to the console, the page's text, its data attributes or an error. What the
// page learns is states, counts and timings (`stats`), and failures as codes.

const FADE_S = 0.08; // the fade of a cancelled speech
const LOUD_UP_TO = 4; // above this speed nothing is synthesized (as the music fades out)
const STEPS = { start: 16, min: 4, max: 16 };
const CPU_STEPS = { start: 4, min: 3 }; // without WebGPU (the same max)
const SPEED = 1.05; // the sample code's default pace
const LEAD_S = 0.02; // a new utterance starts this far after now

// FNV-1a over the speaker and the text: the noise seed.
function seedOf(speaker, text) {
  let h = 0x811c9dc5;
  for (const c of `${speaker}:${text}`) {
    h ^= c.codePointAt(0);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/// The voices of the page. `speed()`: the game's speed now (0 = Max). `onState(state, code)`:
/// 'loading' (with the progress in `stats`), 'ready' (`stats.path`: 'gpu' or 'cpu', where the
/// model runs), or 'failed' with an error code; after `stop` nothing more is reported.
/// `onStats()`: `stats` changed.
export function createVoices({ workerUrl, speed, onState, onStats }) {
  const stats = {
    got: 0, total: 0, stage: '', coldMs: 0, refetched: 0, spoken: 0, firstMs: -1, cancels: 0,
    fadeMs: 0, playing: 0, steps: STEPS.start, dropped: 0, path: '',
  };
  let worker = null;
  let ready = false;
  let audio = null; // { ctx, output }
  let queue = []; // utterances waiting: { id, turn, speaker, text, lang, at }
  // The utterance in the worker: { id, gain, sources, live, done, cancelled, started, at }.
  let current = null;
  let playing = []; // utterances with a gain still on the output (current among them)
  let turn = -1;
  let nextId = 1;
  let endAt = 0; // when the audio scheduled so far ends (context time)

  const changed = () => onStats?.();

  function fast() {
    const s = speed();
    return s === 0 || s > LOUD_UP_TO;
  }

  function fail(code) {
    const was = worker;
    stop();
    if (was) onState('failed', code);
  }

  function onMessage(e) {
    const m = e.data;
    if (m.type === 'progress') {
      Object.assign(stats, { got: m.got, total: m.total, stage: 'download' });
      onState('loading');
    } else if (m.type === 'stage') {
      stats.stage = m.stage;
      onState('loading');
    } else if (m.type === 'ready') {
      ready = true;
      const path = m.path === 'cpu' ? 'cpu' : 'gpu';
      Object.assign(stats, { coldMs: Math.round(m.ms), refetched: m.refetched, stage: 'ready', path });
      if (path === 'cpu') stats.steps = Math.min(stats.steps, CPU_STEPS.start);
      onState('ready');
    } else if (m.type === 'audio') {
      play(m);
    } else if (m.type === 'done' || m.type === 'cancelled') {
      finished(m.id);
    } else if (m.type === 'error') {
      if (m.code === 'speak' && ready) {
        stats.dropped += 1;
        finished(m.id);
      } else {
        fail(m.code);
      }
    }
    changed();
  }

  /// Starts the worker and loads the model (`cachedOnly`: from this browser's cache only).
  function load({ cachedOnly = false } = {}) {
    stop();
    Object.assign(stats, { got: 0, total: 0, stage: 'start', coldMs: 0, refetched: 0, path: '' });
    try {
      worker = new Worker(workerUrl, { type: 'module' });
    } catch {
      worker = null;
      onState('failed', 'model');
      return;
    }
    worker.onmessage = onMessage;
    worker.onerror = (e) => {
      e.preventDefault?.();
      fail('model');
    };
    worker.onmessageerror = () => fail('model');
    worker.postMessage({ type: 'load', cachedOnly });
    onState('loading');
  }

  /// Ends the voices: the worker stopped, the speech cancelled, the queue empty.
  function stop() {
    cancel(false);
    worker?.terminate();
    worker = null;
    ready = false;
    current = null;
    playing = [];
    turn = -1;
  }

  /// The AudioContext and the node the speech goes to (the voice gain).
  function attach(ctx, output) {
    audio = { ctx, output };
  }

  /// What the engine's speech tap queued (Game.take_speech's JSON): each utterance is queued, a
  /// newer turn first cancelling the old speech. Dropped without a ready model, without an
  /// AudioContext, or above 4x.
  function say(json) {
    let list;
    try {
      list = JSON.parse(json);
    } catch {
      stats.dropped += 1;
      return;
    }
    if (!Array.isArray(list)) return;
    for (const u of list) {
      if (typeof u?.text !== 'string' || !Number.isInteger(u.turn)) continue;
      if (!ready || !audio || fast() || u.turn < turn) {
        stats.dropped += 1;
        continue;
      }
      if (u.turn > turn) {
        if (turn >= 0) cancel();
        turn = u.turn;
      }
      queue.push({
        id: nextId, turn: u.turn, text: u.text, lang: u.lang === 'ko' ? 'ko' : 'en',
        seed: seedOf(u.speaker, u.text), at: performance.now(),
      });
      nextId += 1;
    }
    pump();
    changed();
  }

  // The next utterance to the worker, when none is there. It plays through a gain of its own
  // (its fade when cancelled).
  function pump() {
    if (current || !worker || !ready || !audio) return;
    const u = queue.shift();
    if (!u) return;
    const gain = audio.ctx.createGain();
    gain.connect(audio.output);
    current = { ...u, gain, live: 0, done: false, cancelled: false, started: false };
    playing.push(current);
    worker.postMessage({ type: 'speak', id: u.id, text: u.text, lang: u.lang, seed: u.seed, steps: stats.steps, speed: SPEED });
  }

  // Off the list of what plays once its last source ended and nothing more comes for it.
  function settle(u) {
    if (u.live > 0 || !u.done) return;
    playing = playing.filter((p) => p !== u);
    u.gain.disconnect();
  }

  // One chunk of the current utterance: scheduled after what is scheduled already; the steps
  // adapt to how long it took. A chunk of a cancelled utterance is dropped.
  function play(m) {
    const u = current;
    if (!u || u.id !== m.id || u.cancelled || !audio) return;
    const { ctx } = audio;
    const buffer = ctx.createBuffer(1, m.pcm.length, m.sampleRate);
    buffer.copyToChannel(m.pcm, 0);
    const source = ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(u.gain);
    const at = Math.max(ctx.currentTime + LEAD_S, endAt);
    source.start(at);
    u.live += 1;
    stats.playing += 1;
    source.onended = () => {
      u.live -= 1;
      stats.playing = Math.max(0, stats.playing - 1);
      settle(u);
      changed();
    };
    (u.sources ??= []).push(source);
    endAt = at + buffer.duration + (m.gap ?? 0);
    if (!u.started) {
      u.started = true;
      stats.spoken += 1;
      stats.firstMs = Math.round(performance.now() - u.at + (at - ctx.currentTime) * 1000);
    }
    const ratio = m.synthMs / 1000 / Math.max(m.audioSec, 1e-3);
    const min = stats.path === 'cpu' ? CPU_STEPS.min : STEPS.min;
    if (ratio > 0.5 && stats.steps > min) stats.steps = Math.max(min, Math.floor(stats.steps / 2));
    else if (ratio < 0.15 && stats.steps < STEPS.max) stats.steps = Math.min(STEPS.max, stats.steps * 2);
  }

  // The worker finished the current utterance (spoken out, or cancelled): the next one goes.
  function finished(id) {
    if (!current || current.id !== id) return;
    current.done = true;
    settle(current);
    current = null;
    pump();
  }

  /// Ends what is spoken now and what waits: each utterance's gain fades to 0 over 80 ms and its
  /// sources stop then; the worker is told to drop the utterance it makes, and what it still
  /// sends of it is not played (`count`: a cancel the stats count).
  function cancel(count = true) {
    queue = [];
    const had = current !== null || playing.length > 0;
    if (current) {
      current.cancelled = true;
      worker?.postMessage({ type: 'cancel', id: current.id });
    }
    if (audio) {
      const now = audio.ctx.currentTime;
      for (const u of playing) {
        const g = u.gain.gain;
        g.cancelScheduledValues(now);
        g.setValueAtTime(g.value, now);
        g.linearRampToValueAtTime(0, now + FADE_S);
        for (const source of u.sources ?? []) source.stop(now + FADE_S);
      }
      endAt = Math.min(endAt, now + FADE_S);
    }
    if (count && had) {
      stats.cancels += 1;
      stats.fadeMs = FADE_S * 1000;
    }
    changed();
  }

  /// Above 4x: what is spoken stops.
  function speedChanged() {
    if (fast()) cancel();
  }

  /// Another game is on screen: its speech tap counts its turns from the start again, so what is
  /// said of the old one ends and the next turn is taken whatever its number.
  function newGame() {
    cancel(false);
    turn = -1;
  }

  return {
    stats, load, stop, attach, say, cancel, speedChanged, newGame,
    get ready() { return ready; },
    get running() { return worker !== null; },
  };
}
