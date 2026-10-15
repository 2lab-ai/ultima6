---
title: ultima6 — 09 NPC bundles, voice profiles, Supertonic-3 TTS
type: spec
tags: [ultima6, npc, tts, supertonic]
---

# 09 — Per-NPC dialogue bundles, voice profiles, on-device Supertonic-3 TTS

Status: planned (rev 3, 2026-10-16. rev 2 folded in the gpt-6-astra review: the bundle exporter before profiles, a consent boundary for the profile LLM, exact-text tap tests, defined latency, failure recovery, click-through model terms. rev 3 folds in the opus review, checked against the code: the tap flags only the converse text emitters, a defined speaker after `0xbf`, bundles keyed by script with 08's run catalog, an end-to-end receipt on the preview site). This is user item 3 (`hd-i18n-voice/ssot.md`, verbatim):
- Bundle each NPC's name and dialogue separately, for a later NpcBrain to imitate the NPC's speech.
- Give each NPC a voice profile from its personality, for on-device TTS.
- Attach real-time Supertonic-3 TTS, played when an NPC speaks.

Research notes stay outside the repo: `~/Games/ultima6/research/2026-10-15/r3-npc-voice-findings.md`.

## Measured facts

- **Who is talking.** The NPC talked to is `ConverseState.npc` (`converse/mod.rs:85`); `talker` (`:87`) is the party member. Both are set at `start.rs:253-254`. The name starts after the script's first 2 bytes (`start.rs:205-207`).
  - Actor number and script are not one-to-one: actors from 0xe0 take their script from their object type (`script.rs:20-24,94-99`), shrines and statues remap `npc` (`start.rs:262-264`), and some archive items are type scripts.
  - Op `0xbf` (`exec.rs:114-117`) shows another actor's portrait panel and prints that actor's name into the Status window (`inventory/portrait.rs:138-140`). It never writes `npc` and prints no speech.
- **Where speech is printed.** All of these call the same `run_text`, so `run_text` alone cannot tell speech from look text: look text (`start.rs:224,433`), the greeting (`:235`), answers (`:436`, nested at `run.rs:156`). Inside it, speech is emitted at three places:
  - text bytes through `put_char` (`run.rs:86`);
  - `0xb5`'s table form through `put_char` (`exec.rs:228`);
  - `0xb5`'s slot form through `printf` (`exec.rs:241`); its `d5` form prints nothing.

  Engine messages are printed inside `run_text` too (`run.rs:82,154`, `exec.rs:206-207`). The player's echo is printed outside `run_text`.
- **Waits inside a conversation:** the `*` wait (`printf.rs:108-117`), the more prompt (`print.rs:117`) and op `0xcb`'s key wait (`exec.rs:147`, `seams.rs:51-55`).
- **Audio graph.** One `AudioContext` created from a gesture (`site/app.js:124-131`): worklet → game gain → destination (`:136-141`). `setMuted` sets only that gain (`:175-177`).
- **Storage.** `site/store.js` has one object store, `files` (DB version 1, `:9,14`), with data separated by key prefix (`save:` at `:67`; 08 adds `locale:ko`).
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
- Committed: the voice-profile schema, the mixer, and a **type-class table**. The table is keyed only by what the engine's code already names (the object types `start.rs:13-18` names, worktype and alignment buckets) and maps them to a voice class with speed and pitch offsets. A table of which object types are human, gargoyle or female would encode knowledge read from game data, so that one is built locally, with the voice pack. Without the local pack, the voice's sex comes from the actor number's parity, as a neutral tie-breaker.
- Synthesis runs in the visitor's browser on text from their own files. Audio is kept in memory only.

## Design

1. **Engine speech tap (`u6-core::speech`).** It is in place first and changes no frame.
   - **Kind.** The `start.rs` call sites pass a kind to `run_text`: Look at `:224,433`, Speech at `:235,436`.
   - **Capture.** A port-only flag is raised only around the three speech emitters (`run.rs:86`, `exec.rs:228,241`), and only while the kind is Speech and the current window is the Messages window (as `transcript_end` checks, `print.rs:32`). In `put_raw`, text is captured after the capitalise step and before the runic bit, as code points (08 widens the path to `i32`). Look text, engine messages, the `0xbf` name and the player's echo are never captured.
   - **Speaker.** The speaker is `npc`. After `0xbf`, it is that op's actor (after `resolve_npc`) until the next input op, when it reverts to `npc`.
   - **Flush.** The buffer is flushed at each input op, at the `*` wait, the more prompt and `0xcb`'s key wait, at `0xbf`, and at the conversation's end.
   - **Event.** Each flush emits `{seq, turn, speaker, script, text, lang, traits}`. `turn` increments each time the player gives input, and `script` is (archive, item). `lang` is `ko` when any part of the utterance came from the Korean pack, otherwise `en`.
   - **Queue.** The tap runs only while voices are on (an enable flag set by the page). Its queue is capped like the transcript (`print.rs:35-38`), dropping the oldest.
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
   - **Player output.** A "voice" gain node runs parallel to the game gain, on the existing `AudioContext`. `setMuted` sets both. Above 4× speed (06), speech is not synthesized, because lines go by faster than they can be spoken.
   - **Queue and cancel.** One synthesis runs at a time. A newer `turn` cancels the old queue with an 80 ms fade, and the player's Enter, Escape, Space or more key also cancels. Speech never makes the game wait.
   - **Opt-in.** A "Voices (~400 MB)" opt-in, with a storage estimate and `navigator.storage.persist()`. A persistent "voice generated on this device" chip, and a "Voice model terms" notice linked from the page. "Forget voices" deletes the cached model, and "Forget NPC bundles" deletes the bundles; both sit next to the other forget buttons.
   - **Without WebGPU.** No speech, and a note explains why.
4. **Voice profiles (`site/voice/profile.js`).**
   - A profile is `{mix: [{style, w} ≤ 3], speed 0.9–1.15, pitch −2..+2 st, steps, seedSalt}`.
   - The default comes from the type-class table plus the actor number as a tie-breaker, so that neighbours in one place get distinct voices.
   - A local voice pack, `voices.u6vp`, holds a personality-derived profile for every exported actor. It is built by `tools/speech/profile` from the NPC bundles (5) and dropped like the other packs.
     - The LLM receives the actor's bundle and returns profile parameters only, plus a short personality note that stays local.
     - The LLM runs through the owner's llmux, so bundle text leaves the machine. The tool uses 08's upload boundary unchanged: it names the route, writes the exact first payload to a private 0600 file under `~/Games/ultima6/voice/preview/` and prints only its path and size, and sends nothing without a typed `yes` or `--send`. The same zero-request tests apply.
     - The tool writes a coverage receipt that reconciles exported scripts against generated profiles and lists failures by (archive, item). It holds counts and numbers only.
   - The seed is `hash(actor, line)`, so a repeated line sounds the same.
5. **NPC bundles (`u6.npc-bundle/1`).** One JSON per conversation script, keyed by (archive, item), with the actors that use it as a field:
   - the actors and the portrait sources;
   - the name and locale;
   - `narration.look`;
   - `speech.greeting`;
   - `turns[{input, topics[{keywords, requires, answer}]}]`;
   - `traits` (numbers only);
   - segments `text | var | op | if`, where a `text` segment is one 08 run, carrying its key (archive, item, run offset), so a bundle and the Korean pack line up.

   The exporter reads 08's catalog of runs and table strings, which 08's conversation disassembler (04 W3a's op table) builds. It runs in wasm in the browser from the kept files, stores the bundles in the existing store under the key prefix `npc-bundle:`, and offers a download button. NpcBrain (04 W7b) reads them later; dialogue leaves the device only per NPC, with consent and the exact JSON shown first (04 §3.a).

## Order

1. The speech tap.
2. Model hosting, on the preview deploy.
3. The worker and player in English, with type-class voices: the generic loader, no LLM.
4. The NPC bundle exporter, after 08's conversation disassembler (L3).
5. Personality profile generation from the bundles, then the local voice pack, with its coverage receipt.
6. Korean speech, after 08.

## Acceptance

1. **No frame changes.** With voices off, every parity receipt is identical and nothing is downloaded.
2. **Tap.** On synthetic scripts, the test asserts the exact captured text, the speaker, the turn and the flush order for each case:
   - a greeting;
   - an answer, and a nested answer;
   - look text, which is excluded;
   - the player's echo, which is excluded;
   - an engine message printed inside an answer, which is excluded;
   - both printing `0xb5` forms;
   - `0xbf` in the middle of a turn: its name is not captured, the speaker becomes its actor, and the speaker reverts to `npc` at the next input;
   - a `0xcb` wait, which flushes;
   - with voices off, nothing is queued.

   A sentinel string in a synthetic conversation never appears in the console, in `document.body.dataset` or in an error payload.

   The real-data receipt prints pass/fail and counts only.
3. **Hosting.** Every file under `site/models` is ≤ 24 MiB and the manifest hashes match. `_headers` is unchanged. The preview deploy serves every part.
4. **Speech.**
   - The page shows a ready state after download, verification and warm-up.
   - Reference: headless Chromium with WebGPU on the owner's Mac, a 60-character synthetic English line.
   - Cold start, from opt-in to ready, is reported separately.
   - In the ready state, first audio comes within 1 s of the line appearing, and ASR on the synthetic line matches.
   - A key press cancels the speech.
   - Two NPCs of different classes sound different: their median F0 differs by at least 2 semitones, or their style embeddings by a cosine distance of at least 0.1.
   - **End to end, on the preview site with the owner's files:** talk to one NPC; its greeting plays, and ASR of the played audio matches the greeting text held locally. The receipt prints pass/fail only.
   - The machine-generated chip and the terms notice are visible.
5. **Local outputs.**
   - The owner's voice pack gives every exported actor its personality profile; the coverage receipt shows 0 unexplained gaps.
   - The NPC bundles export and download.
   - None of it is in git or under `site/`.
6. **Failures are recoverable, and the game keeps running in every case.**
   - A failed download, a corrupted cached part (hash mismatch, then refetch), a storage quota denial and a lost GPU device each disable voices with a content-free message.
7. **Upload boundary.** 08's zero-request tests pass for the profile tool.
8. **Model terms.** Before the first download, the page shows the model's use restrictions, and the visitor accepts them by clicking through. That acceptance is the agreement OpenRAIL-M §4(a) asks for. The licence and the notice are served beside the parts.
9. **Gates.** All gates pass.
