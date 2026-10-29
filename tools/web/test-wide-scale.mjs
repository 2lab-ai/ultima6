import assert from 'node:assert/strict';
import { probeWideScale } from '../../site/wide-scale.mjs';

// Different whole-composition costs include an outlier: the median decides, no threshold
// relaxation, and neither the warm-up nor 2× compositions enter it.
for (const [costs, want] of [
  [[7, 7, 100, 7, 7], 4],
  [[8, 8, 8, 8, 8], 4],
  [[9, 9, 1, 9, 9], 2],
]) {
  let [scale, time, probe] = [2, 0, -1];
  const game = {
    hd_scale: () => scale,
    present() {},
    set_hd(on, next) {
      assert.equal(on, true);
      scale = next;
      if (next === 4) {
        time += probe < 0 ? 100 : costs[probe];
        probe += 1;
      }
    },
  };
  const sampling = probeWideScale(game, () => time);
  let result;
  let samples = 0;
  do {
    result = sampling.next();
    samples += 1;
    assert.equal(scale, 2, 'each sample restores scale before the next rendered frame');
  } while (!result.done);
  assert.equal(samples, 6, 'one warmup and five samples, spread over six frames');
  const measured = result.value;
  assert.equal(measured.scale, want);
  assert.equal(measured.medianMs, costs.filter((n) => n !== 1 && n !== 100)[0]);
  assert.equal(scale, 2, 'the caller scale restored');
}
let scale = 2;
assert.throws(() => probeWideScale({
  hd_scale: () => scale,
  present() {},
  set_hd(on, next) {
    scale = next;
    if (next === 4) throw new Error('probe failed');
  },
}).next());
assert.equal(scale, 2, 'the caller scale restored after a failed probe');


// A cheap base composition can exceed the budget only when the B actor layers are rebuilt.
// Model separate base/layer generations so returning to a scale alone cannot pass restoration.
function layeredGame({ before = 2, baseCost = 2, layerCost = 7, fail = false } = {}) {
  let scale = before, generation = 0, layerGeneration = 0, time = 0;
  const game = {
    hd_scale: () => scale,
    set_hd(on, next) {
      assert.equal(on, true);
      if (scale === next) return;
      scale = next;
      generation += 1;
      if (scale === 4) time += baseCost;
    },
    present() {
      if (scale === 4 && fail) throw new Error('layer probe failed');
      if (layerGeneration !== generation && scale === 4) time += layerCost;
      layerGeneration = generation;
    },
  };
  return { game, now: () => time, state: () => [scale, generation, layerGeneration] };
}
{
  const f = layeredGame();
  const sampling = probeWideScale(f.game, f.now);
  let result;
  do { result = sampling.next(); } while (!result.done);
  assert.equal(result.value.medianMs, 9, 'the budget includes 2 ms base plus 7 ms B layers');
  assert.equal(result.value.scale, 2, '9 ms chooses S 2 without relaxing the 8 ms threshold');
}
for (const before of [2, 4]) {
  const f = layeredGame({ before });
  const sampling = probeWideScale(f.game, f.now);
  let result;
  do {
    result = sampling.next();
    const [scale, generation, layerGeneration] = f.state();
    assert.equal(scale, before, 'every yield restores the caller scale');
    assert.equal(layerGeneration, generation, 'every yield restores the corresponding layers');
  } while (!result.done);
}
{
  const f = layeredGame({ fail: true });
  assert.throws(() => probeWideScale(f.game, f.now).next(), /layer probe failed/);
  const [scale, generation, layerGeneration] = f.state();
  assert.equal(scale, 2, 'a failed layer probe restores the caller scale');
  assert.equal(layerGeneration, generation, 'a failed layer probe restores its layers too');
}
console.log('Wide scale policy: 8 cases passed');
