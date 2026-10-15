---
title: ultima6 — 09 NPC bundles, voice profiles, Supertonic-3 TTS
type: spec
tags: [ultima6, npc, tts, supertonic]
---

# 09 — Per-NPC dialogue bundles, voice profiles, on-device Supertonic-3 TTS

Status: planned (rev 1, 2026-10-15). This is user item 3 (`hd-i18n-voice/ssot.md`, verbatim):
- Bundle each NPC's name and dialogue separately, for a later NpcBrain to imitate the NPC's speech.
- Give each NPC a voice profile from its personality, for on-device TTS.
- Attach real-time Supertonic-3 TTS, played when an NPC speaks.

Research notes stay outside the repo: `~/Games/ultima6/research/2026-10-15/r3-npc-voice-findings.md`.

## Measured facts

- **Who is talking.** The speaker is the actor number `ConverseState.npc` (`converse/mod.rs:85`). The script and portrait items derive from it (`converse/script.rs:92-103,183-215`), and the name is the script bytes before `f1` (`start.rs:200`). Op `0xbf` switches the portrait actor mid-conversation (`exec.rs:114`).
- **Where speech is printed.** Speech reaches the screen through `put_raw` (`ui/print.rs:72`), from `run_text` and `printf`. Look text goes through the same function; it is printed at `start.rs:224,433`, the greeting at `:235` and answers at `:436`.
- **Voices.** Supertonic-3 presets F1–F5 and M1–M5 are each `style_ttl [1,50,256]` + `style_dp [1,8,16]`, and every token has L2 norm 1.0.
  - A weighted sum with per-token renormalisation is a valid new voice: ASR transcripts were exact for 2-way, 3-way and extrapolated mixes.
  - Pitch ranges about 85–205 Hz across presets and mixes.
  - `speed` divides the duration.
  - There is no pitch parameter; playback-rate resampling gives ±2 semitones.
  - Below 16 `steps`, timbre degrades, but the words stay exact.
- **In the browser (onnxruntime-web 1.30.0, WebGPU, one thread).**
  - 32 steps runs at a real-time factor of 0.05–0.09, with first audio after ~0.6 s for a 9 s chunk.
  - Korean and English both decode.
  - It works under our CSP (`connect-src 'self'`, `wasm-unsafe-eval`) with no COOP/COEP.
- **Hosting.** The models total about 398 MB:
  - `vector_estimator` 256.5 MB, `vocoder` 101.4 MB, `text_encoder` 36.4 MB;
  - the ort wasm is 26.8 MB.
  - Cloudflare Pages caps a file at 25 MiB, so all of these must be served as parts.
- **Licences.** The model is OpenRAIL-M, under which hosting the weights is distribution. That requires the licence beside the model, the use restrictions passed on, a notice for files served in parts, and disclosure that the content is machine-generated. ort is MIT. Supertonic's sample code is MIT.

## Rights (fixed)

- NPC bundles hold game text, so they are local only. The schema is committed; the data is built in the visitor's browser from their own files and kept in IndexedDB. It is never uploaded, and it is downloaded only on a button press.
- Per-NPC personality notes and per-NPC voice tables are derived from reading the game, so they are local only. They are built by a local tool and stored or dropped like the other packs.
- Committed: the voice-profile schema, the mixer, and a **type-class table**. The table maps actor object type, worktype and alignment buckets to a voice class with speed and pitch offsets. These are numbers the original's code defines, and the table holds no names.
- Synthesis runs in the visitor's browser on text from their own files. Audio is kept in memory only.

## Design

1. **Engine speech tap (`u6-core::speech`).** It is in place first and changes no frame.
   - **Capture.** In `put_raw`, text is captured after the capitalise step and before the runic bit, into a port-only `State.speech` buffer. A depth gate raised by `run_text` restricts capture to greetings, answers and the `0xb5`/`0xbf` text; look text and the player's echo are not captured.
   - **Flush.** The buffer is flushed at each input op, at the `*` and more waits, at a portrait switch and at the conversation's end.
   - **Event.** Each flush emits `{seq, turn, speaker, text, lang, traits}`. `turn` increments each time the player gives input.
   - **Shared path.** Korean text from 08 goes through the same `speech::emit(&str)`.
   - **Export.** `Game::take_speech()` is a separate channel. Speech text never reaches the console, `dataset`, error reports or test output.
2. **Model hosting (`tools/speech`).**
   - `fetch-models.sh` downloads the pinned Hugging Face revision `724fb5ab…` and ort 1.30.0, then checks the sha256 pins.
   - It splits every file into parts of ≤ 24 MiB and writes `site/models/supertonic3/<rev>/manifest.json` with the part hashes and the whole-file hash.
   - `LICENSE.txt` and `NOTICE` (the use restrictions, a note that files are served in parts, the hashes) go beside the parts.
   - The parts are gitignored and uploaded by `deploy-web.sh`. They are proven on a preview deploy first.
   - Only three ort files are needed: `ort.webgpu.min.mjs` and the asyncify `.mjs` and `.wasm`.
3. **Worker and player (`site/voice/`).**
   - **Worker origin.** The worker is written from Supertonic's MIT sample code, as the owner's own browser demo is. Code from the iq.io Gucci work is not copied.
   - **Model loading.** The worker fetches the parts, reassembles them, verifies the whole-file hash, and caches the result in Cache Storage with versioned URLs and pruning. The ort wasm is loaded through `wasmBinary`; this path needs its own receipt in the browser.
   - **Runtime.** WebGPU only, one thread, with a one-word warm-up synthesis.
   - **Steps.** Adaptive, starting at 16: halved when synthesis takes more than 0.5× the audio, doubled back below 0.15×.
   - **Language.** `lang` is passed per utterance.
   - **Player output.** A "voice" gain node runs parallel to the game gain, on the existing `AudioContext`. Mute silences both.
   - **Queue and cancel.** One synthesis runs at a time. A newer `turn` cancels the old queue with an 80 ms fade, and the player's Enter, Escape, Space or more key also cancels. Speech never makes the game wait.
   - **Opt-in.** A "Voices (~400 MB)" opt-in, with a storage estimate and `navigator.storage.persist()`. A persistent "voice generated on this device" chip, and a "Voice model terms" notice linked from the page.
   - **Without WebGPU.** No speech, and a note explains why.
4. **Voice profiles (`site/voice/profile.js`).**
   - A profile is `{mix: [{style, w} ≤ 3], speed 0.9–1.15, pitch −2..+2 st, steps, seedSalt}`.
   - The default comes from the type-class table plus the actor number as a tie-breaker, so that neighbours in one place get distinct voices.
   - A local voice pack, `voices.u6vp`, can override any actor. It is built by `tools/speech/profile` from the NPC bundles through the owner's LLM (personality → profile parameters only) and dropped like the other packs.
   - The seed is `hash(actor, line)`, so a repeated line sounds the same.
5. **NPC bundles (`u6.npc-bundle/1`).** One JSON per actor, keyed by the actor number:
   - the script and portrait sources;
   - the name and locale;
   - `narration.look`;
   - `speech.greeting`;
   - `turns[{input, topics[{keywords, requires, answer}]}]`;
   - `traits` (numbers only);
   - segments `text | var | op | if`.

   The exporter is built on the conversation disassembler (04 W3a), which is shared with 08's extractor. It runs in wasm in the browser from the kept files, stores the bundles in IndexedDB under `npc-bundles`, and offers a download button. NpcBrain (04 W7b) reads them later; dialogue leaves the device only per NPC, with consent and the exact JSON shown first (04 §3.a).

## Order

1. The speech tap.
2. Model hosting, on the preview deploy.
3. The worker and player in English, with type-class voices.
4. Profiles and the local voice pack.
5. The NPC bundle exporter, after W3a's disassembler.
6. Korean speech, after 08.

## Acceptance

1. **No frame changes.** With voices off, every parity receipt is identical and nothing is downloaded.
2. **Tap.** On synthetic scripts, the event count, speakers and turns are correct and no text is printed. A real-data receipt prints pass/fail and counts only.
3. **Hosting.** Every file under `site/models` is ≤ 24 MiB and the manifest hashes match. `_headers` is unchanged. The preview deploy serves every part.
4. **Speech.**
   - On the live site with voices on, talking to an NPC plays speech within 1 s of the line appearing, measured headless with WebGPU and checked by ASR on a synthetic line.
   - A key press cancels the speech.
   - Two NPCs of different classes sound different: their F0 or style differs.
   - The machine-generated chip and the terms notice are visible.
5. **Local outputs.** The owner's voice pack overrides a profile. The NPC bundles export and download, and none of it is in git or under `site/`.
6. **Gates.** All gates pass.
