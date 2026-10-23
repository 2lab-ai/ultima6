---
title: ultima6 — 12 Korean register by each NPC's profile (children speak as children)
type: spec
tags: [ultima6, i18n, korean, npc]
---

# 12 — Korean register by profile: each speaker's age and station decide the speech level

Status: shipped (2026-10-23, in production at https://ultima6.2lab.ai with the Korean pack ko-1023f; deploys 1, 5, 7 and 9 of `hd-layout-font-register/loop.md`: by age (ko-1023a), every script by its speaker with keyword words per script (ko-1023d), block keys by meaning and quotes kept (ko-1023e), one Korean form per name (ko-1023f); tool changes in spec 08 revs 15-19). User item 3 of `hd-layout-font-register/ssot.md`, verbatim: "npc들의 프로파일에 맞게 영어 다시 번역해줘. 특히 어린얘들이 어른처럼 말하는거 수정해줘." It amends 08 §7 (the register guide) and the translation tool; the engine and the pack format do not change.

## Measured facts (main 6e4e337)

- The guide gives every speaker an archaic adult level and forbids modern endings for all: "no ~요 (해요체), no ~해 / ~야 / ~거든 / ~잖아 (modern 반말)" (`tools/i18n/style-ko.md:13-14`); its classes are royalty, noble, mage, scholar, clergy, guard, merchant, commoner, thief, spirit, gargoyle (`style-ko.md:19-33`, `translate.mjs:174-186`). There is no age: a child is a commoner and speaks a low 하오체 (`날이 벌써 저물었소`).
- The class comes from words of the NPC's name and look text (`translate.mjs:188-201`, `speakerClass` `:984-987`), or from `--class` for the whole run (`:1705`). The request's `npc` is `{name, description, class, register}` (`bodyOf`, `:1063-1072`).
- Each script already has a personality note, made from its lines by `tools/speech/profile.mjs` for the voices (09): `~/Games/ultima6/voice/notes.json` (`u6.voice-notes/1`, 200 notes, one per exported script; local, never in git). 16 of them name the speaker young, a child, a girl, tiny or the like.
- `--send` sends without the typed `yes` (`translate.mjs:1951-1962`); a run merges into the pack and keeps what it does not replace.

## Design

1. **Speaker file (`u6.speakers/1`, local only).** Per script `(archive, item)`: `age` one of `child`, `youth`, `adult`, `elder`; `class` one of the guide's classes; `note`, the personality note. A new `tools/i18n/speakers.mjs` makes it from the NPC bundles and `notes.json`: for each script the name, the look text and the note, in batches, through the same route and model as `translate.mjs`, the answer gated to the two enums (a bad answer is that script's Pass A class and `adult`, counted). It writes the file and prints counts by age and class only.
2. **The tool takes it.** `translate.mjs --speakers <file> [--ages a,b]` (`--ages` needs `--speakers` and chooses the scripts of those ages, alone or with `--first`): a script it lists takes that class and age (Pass A otherwise, `adult`); the request's `npc` gains `age` and `profile` (the note), and its `register` is the one for that class at that age. Without `--speakers` a request's `npc`, lines and topics are byte for byte as before; its system prompt carries the new guide (Design 3 changes `style-ko.md` on purpose, as 08 K5b did), which reads a request with no `age` as an adult's, so such a run asks for the same registers as before.
3. **The guide by age (`style-ko.md`).** Age decides first, then class:
   - `child`: a child's speech. Short sentences and easy native words, no Sino-Korean diction and none of the archaic adult endings (~노라, ~도다, ~하오, ~이오, ~하게, ~사옵니다). To a grown-up, a polite child's 해요체 (~요, ~어요, ~예요: "아저씨, 우리 고양이 못 봤어요?"); to another child or an animal, 반말. The hero is 아저씨, 언니, 오빠, 기사님 or 그쪽 as the line suits, never 그대.
   - `youth`: a young person's lighter level: 해요체 or a soft 하오체 by station (an apprentice or a young servant 해요체; a young noble or a squire a soft 하오체), fewer archaic endings than an adult of the class.
   - `adult`: the class table as today.
   - `elder`: the class's level with an old person's turns (~구먼, ~하네, ~일세, 하게체 to a younger hero where the station allows).
   The guide's ban on modern endings becomes "for `adult` and `elder` speakers"; everything else in the guide (escapes, keywords, particles, table strings) is unchanged. Made-up examples only, as now.
   The age is the speaker's, for its own words only (added 2026-10-23, after the first run gave a child's narration and a companion's interjection the child's 해요체): text outside double quotes is the narrator's 하오체 (or a description ending on a noun) with the hero 그대, and a quoted line given to someone else is in that person's own level.
4. **The run, in two deploys.** First the speakers that are not `adult` (the owner's "특히 어린얘들이"): `translate.mjs <bundles> --speakers <file> --ages child,youth,elder --send`, where `--ages` keeps only the scripts the speaker file gives those ages; then every script with `--first 200 --speakers <file> --send` (every conversation script, its table strings and its keywords), merged into a copy of the current pack. The pack is gated as before (the strict reader, the engine's install check through the page). It gets a new file name (`ko-<date>.u6lp`) in the private `ultima6-deploy` repository's `packs/index.json`, since the page downloads the Korean pack again only when the listing names another file. Keyword words may change with the new lines (the merge rule of 08 K5b); the report says how many did.

## Acceptance

1. Tests, synthetic only: the speaker file's reader (enums, refusals), `--speakers` puts `age`, `profile` and the age's register in the body, a request without it is byte for byte the old one, and `speakers.mjs`'s gate and counts.
2. The run's coverage: scripts asked, translated, left English; ages counted; keyword words changed counted.
3. The dispatcher reads every `child` script's new Korean beside its English (locally, never committed): none of them speaks with an adult archaic ending; and a sample of `adult` and `elder` scripts keeps its class's level.
4. Live check on the deployed site: a fresh visitor, the zip only, 한글: the new pack is loaded (its file name), and a conversation shows the new lines.
