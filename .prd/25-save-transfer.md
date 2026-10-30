# Saved-game transfer

## User instruction and scope

Latest instruction, preserved verbatim:

> 일단 qa는 니 컴퓨터에서 계속 진행하고 맥북에서 게임 플레이 오디오 포함해서 테스트하고 그거 하면서 세이브 익스포트 임포트 기능 구현해서 qa 세이브 옮길수 있게 해줘

This supersedes the earlier pause of the proposed migration feature. Existing completed work
stays complete. This feature adds a normal browser UI for portable saves; it does not transfer
any user's files itself, alter the engine's save format, or replace the separate cloud QA and
Mac gameplay/audio tasks. Original resources and personal saved games stay out of public Git,
test fixtures, hosting and model requests.

| Exact instruction clause | Acceptance and evidence owner |
|---|---|
| “qa는 니 컴퓨터에서 계속 진행하고” | Parent keeps the existing cloud QA session; this implementation never controls that tab. |
| “맥북에서 게임 플레이 오디오 포함해서 테스트하고” | Parent/Mac executor verifies normal gameplay and audible output independently. Import preserves browser audio preferences; no new model or licence flow is implied. |
| “세이브 익스포트 임포트 기능 구현해서 qa 세이브 옮길수 있게 해줘” | The normal UI exports a last committed save ZIP and imports it with complete validation, an immutable backup and explicit new-session consent. UI round-trip and failure cases below are required. |

## UI contract

- Use the existing monochrome canvas Settings → Files surface, its bitmap labels, keyboard
  focus/accessibility mirror and at least 44 CSS-pixel controls. Stable action IDs are
  `save-export`, `save-import`, `backup-choice` and `backup-export`.
- Before original files exist, a discreet **Import saved game / 가져온 저장으로 이어가기**
  control opens the same flow. It prepares required files, rather than duplicating game settings.
- A dedicated hidden `#save-picker` accepts one ZIP. It never routes the archive through the
  original disk/pack importer. Existing folder import remains a separate legacy path.
- Export calls `checkSave()` only to enqueue an already completed normal engine Save, then
  queues a strict persisted snapshot read after earlier writes. It never sends Ctrl-S, steps,
  forces Save, restarts, modifies RNG or exports current unsaved state.
- Exported download filenames are `ultima6-save-<UTC>.zip` or `ultima6-save-backup-<UTC>.zip`,
  without avatar names or game text. Only validated `savegame/` records enter the archive;
  `extracted`, packs, model caches, settings, credentials and original resources do not.
- `manifest.createdAt` is labelled **Exported / 내보낸 시각**. It is not the actual game Save
  time. Backup metadata labels its pre-import backup creation time.
- File selection produces a staged token. Size, complete archive/checksum/native-content
  validation and a strict current-save read finish before the canvas preview appears.
  Preview contains counts, byte totals and export time, not original game content.
- **Back up & import / 백업 후 가져오기** explicitly says that a new session starts and
  current unsaved play is lost. The existing committed save is retained as an immutable backup.
  A valid save may be imported before original resources; after reload the launcher still asks
  for those original resources before play.
- Cancel, Escape, Close and a new file choice invalidate the staged token. A queued confirm
  checks that token before beginning its storage transaction. Cancellation before commit is
  a no-op. Once the atomic transaction starts, controls are disabled and the UI does not imply
  that cancellation can undo a commit.
- Starting an unrelated original-file or pack import also invalidates a pending ZIP decode
  and closes its preview. A late confirmation cannot apply to a game replaced by that flow.
  Async modal operations hold individual input-lock leases; a cancelled operation completing
  later cannot release a newer operation's lock.
- `replaceSavesWithBackup(candidate, expectedFiles)` atomically backs up and replaces, rejects
  changes since preview, and leaves all prior records on failure. Byte-identical re-import is
  a no-op with no duplicate backup or reload. On success, the old game's save ownership is
  removed before the explicitly authorized normal reload.
- Backups are shown latest first and can be exported without changing the active save. No
  automatic backup deletion or silent removal of malformed/unknown save entries is allowed.
  Latest-first ordinal/total and second-resolution creation time distinguish backups made
  during the same minute. Unreadable backup storage is distinct from an empty list.
- Empty saves, unreadable storage, quota failure, conflicts, corrupt or unsupported ZIPs are
  visible outcomes. Error messages do not echo untrusted paths, bytes or original text.
  In particular, a sight/objlist mismatch asks the player to save normally in the source game
  and export again; the sidecar is not silently dropped.
- Starting a game reads save files and their storage epoch in one strict snapshot. Normal
  saves, raw-folder replacement, deletion and ZIP replacement compare their captured epoch
  inside the write transaction. Replacement/deletion advances it; normal incremental saves
  retain it. A stale running game loses save ownership and shows a reload requirement without
  acknowledging a save commit. ZIP confirmation also compares its preview epoch to catch
  replacement cycles even when bytes became identical again.
- If ordinary startup cannot read browser saves, original files may still start a visit-only
  game with a visible warning. Its epoch and save owner stay null, so normal saves, legacy
  adoption and deletion cannot write. This is not an empty writable snapshot; ZIP transfer
  and backup operations still reject unreadable storage. Reload after storage becomes usable.
- This protection applies to pages running the new guarded code. An already open page from
  an older release does not send an epoch and cannot be made fully safe by this client patch;
  reload older tabs before moving saved games. Save file bytes and engine formats are unchanged.

## Components

`site/save-transfer.mjs` owns the bounded ZIP envelope and SHA manifest and calls the native
`SaveTransfer` validator. `site/store.js` owns strict snapshot reads, conflict detection and
atomic immutable backups. `site/app.js` owns queueing, staged tokens, downloads, confirmation
and new-session reload. `site/game-settings.mjs` adds optional accept labels/cancel actions and
an atomic-operation busy state while preserving the existing Delete and voice-terms flows.

## Required validation cases

These are acceptance cases, not a claim that checks have already run.

1. Export through real canvas keyboard/mouse/touch actions, capture the browser download,
   decode it, and compare every byte to the last persisted normal save. An unsaved movement
   remains unsaved; no engine Save/step/clock or store replacement is called by export.
2. Export waits for a pending normal save transaction; empty/unreadable storage yields an
   honest failure, and no empty or partial ZIP is downloaded.
3. Import a valid export into a disposable second browser, review and confirm on the canvas,
   then check new-session restoration and unchanged original resources/packs/preferences.
   Exercise both original-files-present and original-files-absent launchers.
4. Check Cancel, Escape, Close, cancelled file picker, second selection and cancellation while
   decoding or queued: zero save/backup writes, old game identity and progress preserved.
5. Hold the atomic transaction and confirm that pointer/keys/Close cannot falsely cancel it or
   send duplicate imports; release it and check exactly one backup and replacement.
6. Quota/transaction failure and another-tab conflict preserve the prior save and game. A
   repeated identical import creates no extra backup and does not reload.
7. Select a prior immutable backup and export it; verify bytes equal the pre-import save and
   active save bytes are unchanged. Backup timestamp means creation, not gameplay Save time.
8. Reject oversized files before whole-file read; reject path traversal, unsupported names,
   duplicates/case aliases, disallowed ZIP features, bad CRC/SHA, malformed native save content,
   missing required entries and stale sight binding without any partial storage write.
9. Capture whole English/Korean desktop and phone frames: readable labels, wrapped preview,
   pagination, separate 44px buttons, focus and paused-game behavior. The accessibility dialog
   exposes the same title and full warning via labelledby/describedby. Re-run existing deletion,
   voice terms, fullscreen, closed-menu pixel and settings pause/reentry regressions.
10. Keep actual cloud QA migration and Mac audio/play receipts distinct from disposable local
    fixtures. A passing local transfer test does not claim a user's save has been moved.
11. Hold cancelled confirmation A, begin B's atomic operation, then complete A: B must retain
    its input lock until B finishes. A different file import cancels both a pending ZIP decode
    and an existing preview without a save replacement.
12. Load a game in two contexts sharing storage, import in one, then normally Save in the stale
    other context: imported bytes/backups remain unchanged, no save_committed acknowledgement
    occurs, and a reload message appears. Check deletion, raw-folder replacement, read failure
    and preview-epoch ABA conflicts through the same guarded storage contract.
