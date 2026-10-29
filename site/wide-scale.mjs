// Spec 13 Design 6: retain 4× only while a full game-frame composition fits 8 ms.
// The caller advances this once per rendered frame: one warm-up, then five samples. Restore
// its scale and presented layers before every yield (and after a failed probe), so intermediate
// frames never show. A scale change composes the base; present also rebuilds the actor layers.
// The probe changes presentation only, never the game clock or state.
export function* probeWideScale(game, now = () => performance.now()) {
  const times = [];
  for (let i = -1; i < 5; i += 1) {
    const before = game.hd_scale();
    try {
      game.set_hd(true, 2);
      game.present();
      const start = now();
      game.set_hd(true, 4);
      game.present();
      if (i >= 0) times.push(now() - start);
    } finally {
      game.set_hd(true, before);
      game.present();
    }
    if (i < 4) yield;
  }
  times.sort((a, b) => a - b);
  const medianMs = times[2];
  return { scale: medianMs > 8 ? 2 : 4, medianMs };
}
