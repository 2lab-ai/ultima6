// The game's sound output: plays the interleaved stereo chunks the page renders in wasm (the
// emulated Amiga audio machine) and posts back how much it holds, so the page keeps it a little
// ahead. Silence while it runs dry; it never asks the engine anything.
class U6Audio extends AudioWorkletProcessor {
  constructor() {
    super();
    this.chunks = [];
    this.at = 0; // read position in chunks[0], in samples
    this.queued = 0; // frames held
    this.received = 0; // chunks received so far
    this.blocks = 0;
    this.port.onmessage = (e) => {
      this.chunks.push(e.data);
      this.queued += e.data.length / 2;
      this.received += 1;
    };
  }

  process(_inputs, outputs) {
    const [left, right] = outputs[0];
    for (let i = 0; i < left.length; i += 1) {
      const chunk = this.chunks[0];
      if (!chunk) {
        left[i] = 0;
        if (right) right[i] = 0;
        continue;
      }
      left[i] = chunk[this.at];
      if (right) right[i] = chunk[this.at + 1];
      this.at += 2;
      this.queued -= 1;
      if (this.at >= chunk.length) {
        this.chunks.shift();
        this.at = 0;
      }
    }
    this.blocks += 1;
    if (this.blocks % 8 === 0) this.port.postMessage({ queued: this.queued, received: this.received });
    return true;
  }
}

registerProcessor('u6-audio', U6Audio);
