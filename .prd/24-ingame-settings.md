# 24 — Pixel text, a focused launcher, and in-game settings

Status: implemented (2026-10-30); release validation is recorded with the pull request.

This change supersedes spec 11's selectable HD typeface and the external settings chooser.
The game always uses its original pixel characters and the existing Galmuri7 Hangul bitmap.
HD tiles, portraits, layout, and Korean translations remain available. Old `u6.font` preferences
have no effect. The Gowun font asset and custom text overpainting are removed.
Glyph and symbol metadata remain diagnostic APIs, computed only when requested from the
last composed source snapshot. Normal rendering does not scan text cells for a removed
overlay, expand dirty rectangles because metadata changed, or reject reusable glyph blocks.

The launcher follows the monochrome, restrained typography and spacing of the owner's
[profile](https://github.com/icedac/icedac), inspected at asset revision
`02aa6fabfedec97393768fd90800dd0a48a4f476`. It puts original-file import and restoration first;
it does not require choosing game settings or accepting voice terms before starting.
Loading, failed import, recovery, and narrow screens are first-class states. No original game
resources are shipped with the engine.

## Settings contract

Settings are painted inside the game canvas. F10, the visible in-game entry, and touch open
the same menu. Escape and a visible close action return to the same game. A hidden semantic
mirror provides accessible names and keyboard traversal; it is not a second visible panel.

| Group | Existing preference or action |
| --- | --- |
| Display | Language, screen layout, HD rendering and pack source, remembered sight, motion, window placement reset, fullscreen |
| Play | Playback speed and turn presentation speed |
| Audio | Game mute, NPC voices, model terms and current preparation status |
| Files | Original/pack import and management of stored originals, saves, HD/Korean packs and voice models |

While settings are open, no engine step, game clock progression, gameplay pointer command,
held key or RNG update is allowed. Closing releases input and resets the wall-time anchor;
the time spent in settings is not caught up afterward. Display-only preference changes do
not alter saved game state. A language or build change that needs a restart is explicitly
marked for the next start instead of discarding unsaved progress. Destructive file removal
requires a separate deliberate confirmation. Starting or opening settings never deletes a save.

Original-file preparation remains available in the launcher. Game preferences have no
duplicate external controls. Voice activation retains the actual model licence/notice and
the visitor's explicit consent; opening settings alone does not consent or download models.

## Verification

- Whole-frame desktop and phone captures before/after, including import and error states.
- Actual canvas/keyboard/touch menu navigation, readable Korean, focus and close/re-entry.
- State, tick and RNG invariance while open and during preference changes; no queued input
  or elapsed-time burst on return; persisted choices and stored saves preserved on reload.
- Pixel glyphs in both layouts and languages, no HD font request or chooser, HD art retained.
- Storage, dialogue, movement, attack, pointer, audio, fullscreen and layout regression checks.
- Rust Original/Wide tests, formatting, strict Clippy, both WASM builds and web checks.
- Publication diff contains engine/UI only; production reuses existing pack/model hashes.
  A repository without CI is reported as having zero checks, not as a passing CI run.

Private screenshots, owner data and runtime save fixtures remain outside version control.
