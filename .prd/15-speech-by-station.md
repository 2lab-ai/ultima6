---
title: ultima6 — 15 speech and voice by station and manner (lords, commoners and the lowborn)
type: spec
tags: [ultima6, i18n, korean, npc, voice]
---

# 15 — Speech and voice by station and manner

Status: draft (2026-10-25). It amends spec 12 (the register by profile) and spec 09 Design 4 (the
personality voice pack). The engine, the Korean pack format and the speech tap do not change.

The owner, verbatim: "각각의 인물에 대해서 프로파일링해주고 각각의 인물의 말투(항상 하오체쓰는거 X)
영주랑 평민과 하층민이 말투가 달라야하고 이 프로파일링으로 음성 프로파일링도 이걸로 만들어줘. 말투
속도나 그런것도 남성 여성과 나이든 사람과 어린사람은 지금 어느정도 잘한거 같고 그 광대같은 사람이나
미친사람 도 잘 했음?"

## Measured facts (main 4320314, pack ko-1025b, voice pack voices-1021)

Counts only; the scripts are the owner's and stay out of the repository.

- **Most speakers speak 하오체.** A private counter of each script's quoted sentence endings in the
  pack gives the dominant level of 199 scripts as 하오 119, 합쇼 30, 해라 21, 해요 15, 하게 7, 반말 7.
  Every adult commoner script (62) is dominantly 하오, the same level as the adult nobles (9, 하오)
  and guards (17, 하오). A lord's script and the jester's script are both 97% 하오.
- **Why.** The class table maps six of its eleven classes to a 하오체 (`tools/i18n/translate.mjs:254-266`,
  `tools/i18n/style-ko.md` class table), and the guide bans modern endings for every adult
  (`style-ko.md:13-14`). The table has no station: a farmer, a beggar and a pirate are all
  `commoner`. Nothing in the profile names a manner (a jester's play, a madman's lurches).
- **The ban is in the prompt only.** No gate of `translate.mjs` reads a sentence ending; lifting it
  for a station is a change of the guide and the request, not of a gate.
- **The jester and the madmen.** The jester's note asks for a sing-song delivery with rhymes; its
  Korean lines are a plain 하오체 with no rhyme or wordplay. A mad mage's script is 77% 하오 and
  prints his own name in three forms (one of them Latin): that name is not in the names table
  (31 entries, `--names`), so no gate holds it to one form. Korean conversation lines that still
  carry a capitalised Latin word: 17 rows in 10 scripts.
- **Voices.** The pack's profiles were chosen by a model told only the presets' names
  (`tools/speech/profile.mjs` SYSTEM, `:80-92`), not what they sound like. Of 200 actors, 30 share
  one exact mix and 42 another; the jester shares his mix with a mayor, a farmer, a beggar and a
  tailor. Speeds run 0.90–1.13 of 0.90–1.15, pitches −2..+2. A profile is fixed per actor
  (`site/voice/player.js:146-153`), so no voice can lurch from line to line.
- **Current medians by age and sex** (speakers file ages, the pack's lead preset's sex; the owner
  calls these about right): child speed 1.10, pitch +2; youth F 0.98/+0.5, M 1.07/+0.9; adult F
  1.00/−0.5, M 0.98/−1.0; elder F 0.945/−1.0, M 0.96/−1.0.
- **The presets, measured** (Supertonic-3 on this machine, `total_steps` 16, speed 1.05, one made-up
  Korean sentence; median F0 by autocorrelation, spectral centroid, seconds):

  | Preset | F0 Hz | Centroid Hz | Seconds | Reading |
  |---|---|---|---|---|
  | M2 | 94 | 1639 | 6.55 | deepest, dark |
  | M5 | 102 | 1764 | 6.69 | deep |
  | M4 | 120 | 1370 | 6.27 | middle, darkest (rough) |
  | M3 | 124 | 2077 | 5.50 | middle, brisk |
  | M1 | 139 | 1993 | 6.69 | high for a man, light |
  | F5 | 166 | 2177 | 6.48 | low, mature |
  | F3 | 168 | 3137 | 6.90 | low, very bright (airy) |
  | F1 | 196 | 2075 | 6.69 | middle |
  | F4 | 204 | 2075 | 5.71 | middle, brisk |
  | F2 | 214 | 1760 | 6.27 | highest, warm |

## Design

1. **Speaker profile `u6.speakers/2`** (local only, `tools/i18n/speakers.mjs`). Per script, the
   v1 fields and four more:
   - `sex`: `female`, `male` or `none` (a shrine, a statue, a thing);
   - `station`: `royalty` (the crown); `lord`, the 양반 tier (the gentle: those who govern, such as
     lords, mayors and the heads of orders, and the knighted and the learned, such as knights,
     court mages, scholars, high clergy and the hero's companions); `commoner` (free townsfolk and
     villagers: farmers, craftsmen, shopkeepers, innkeepers, sailors, bards, town healers, soldiers
     of the ranks); `lowborn` (beggars, thieves, pirates, drunks, outcasts, ruffians); `beyond`
     (spirits, shrines, statues, the otherworldly). The owner's three tiers (영주, 평민, 하층민) are
     `lord`, `commoner` and `lowborn`;
   - `manner`: one of `plain`, `jester` (play, rhyme, puns), `mad` (lurching, broken thought),
     `pompous`, `drunk`, `sly`, `gruff`, `timid`, `pious`;
   - `name_ko`: the NPC's own name in Hangul (at most 8 syllables), or null for a generic one
     (a guard, a farmer).
   `class` stays (the role: mage, clergy, merchant…). The model reads each script's name, look text,
   note and the first lines of its greeting (bounded), and answers the enums; an answer outside an
   enum is that field's default (`none`, `plain`, null; the station of the class read: royalty →
   `royalty`, noble → `lord`, spirit → `beyond`, thief → `lowborn`, any other → `commoner`),
   counted. The tool prints counts
   by age, sex, station and manner only. `translate.mjs` reads `/1` and `/2`; a `/1` file reads as
   before.
2. **The register by station, then age, then manner** (`style-ko.md`, `registerOf`). The age still
   decides first for a child and a youth (spec 12; a youth's lighter level goes by its station when
   the speaker file gives one: `commoner` and `lowborn` 해요체, the others a soft 하오체). For `adult`
   and `elder` the station decides:

   | Station | Adult | Elder | The hero is |
   |---|---|---|---|
   | royalty | 해라체 (~노라, ~하라, ~느니라) | the same | 그대 |
   | lord | 하오체 (~하오, ~이오, ~시오), formal | 하게체 (~하게, ~하네, ~일세) | 그대, 귀공 (an elder lord: 그대, 자네) |
   | commoner | a humble 사극 합쇼 (~습니다, ~습죠, ~굽쇼, ~입니다요) | a rural old person's turns (~구먼, ~했지, ~하게) | 나리, 나으리 (an elder: 젊은이, 나리) |
   | lowborn | rough low talk: 반말 and 하게 mixed (~하슈, ~했수, ~다고, ~하라고, 이봐); a beggar begs in a whining 합쇼 (~줍쇼) | rough 반말 | 이보쇼, 형씨, 당신 (나리 in mockery) |
   | beyond | archaic 해라체 (~로다, ~도다, ~니라) | the same | 그대 |

   - The role colours the station's level: a merchant keeps the shop's formulas (어서 옵쇼,
     ~하옵니다); a scholar, a cleric or a mage speaks with Sino-Korean diction; a guard is clipped;
     a gargoyle keeps the guide's gargoyle register, its station setting only whom it defers to.
   - The modern-ending ban (해요체 ~요; 반말 ~해, ~야, ~지, ~냐, ~거든, ~잖아) holds for every adult
     and elder except the `lowborn`, who may use its 반말 endings but not 해요체. An elder's old
     turns (~구먼, ~했지, ~하네, ~일세) are not the modern ending. No speaker uses modern loanwords
     or slang.
   - The manner shapes diction and rhythm inside that level: a `jester` makes Korean rhymes, puns
     and a sung rhythm of his own (the line's sense and every keyword kept, not the English's
     wordplay word for word); a `mad` speaker lurches (fragments, repetitions, sudden shouts and
     whispers, talk to himself) and may break the one-level rule; `pompous` inflates (에헴,
     본관, Sino-Korean titles); `drunk` slurs and trails (~하는 거여…) and may slip into a slurred
     반말; `sly` flatters and hints;
     `gruff` is short; `timid` hesitates (저, 저기…); `pious` blesses and intones.
   - The narrator is as before: 하오체 or a noun ending, the hero 그대. The station, the manner and
     the address are for the speaker's own words only.
   - The request's `npc` gains `station`, `manner` and `address` (the table's hero words); a
     request from a `/1` file or without `--speakers` is byte for byte as before.
3. **One Korean form per name, for every speaker.** The dispatcher merges each profile's
   `name_ko` into the owner's names table (local; an entry already there wins; a name that is an
   ordinary English word or phrase is left out) before the run, so `--names` holds each NPC's
   name to one form.
4. **Voices from the profile** (`tools/speech/profile.mjs --speakers <file>`, no model request).
   For each actor of each script, the profile of its speaker gives:
   - presets by sex, then station and age, from the measured table: male `royalty`, `lord`,
     `beyond` lead M2 or M5; male `commoner` M3 or M1; male `lowborn` M4; a young man or a boy M1;
     female `royalty`, `lord`, `beyond` and the elderly F5 or F3; female `commoner` and `lowborn`
     F1 or F4; a young woman or a girl F2 or F4; `none` the actor number's parity, as today. The
     actor number picks within the family and blends a second preset (weight 0.6–0.8), so
     neighbours differ;
   - speed and pitch: the age and sex median above, plus a station offset (royalty −0.05, lord
     −0.03, commoner 0, lowborn +0.02, beyond −0.06 speed; royalty −0.5, lord −0.3 pitch) and a
     manner offset (jester +0.10 speed, +1.0 pitch; mad +0.03; pompous −0.04, −0.4; drunk −0.07;
     sly +0.02; gruff +0.01, −0.3; timid −0.02, +0.4; pious −0.03), plus a seeded spread of ±0.02
     speed and ±0.3 pitch, each into the ranges (0.90–1.15, −2..+2; the ranges do not change);
   - `vary`, new: how far one line may move from the actor's speed and pitch: jester 0.04 / 0.8,
     mad 0.10 / 1.2, drunk 0.05 / 0.4, every other manner 0 / 0.
   The pack is `u6.voice-pack/2`: each profile has `vary: {speed, pitch}` (0..0.15, 0..1.5). The page
   reads `/1` (no vary) and `/2`; its other checks and `RANGES_REVISION` are as before.
5. **The player varies a line** (`site/voice/player.js` `say`): the queued line's speed and pitch
   are the actor's moved by two draws `a`, `b` in −1..1 from the line's seed (`seedOf(speaker,
   text)`): speed + `a` × min(`vary.speed`, the room the range leaves on `a`'s side), and the
   same for pitch, and toward a side with no room (the actor's own speed or pitch on that end)
   the line moves the same distance the other way; so the actor's own speed and pitch (its age,
   sex and station) stay as derived and no line is held at a range's end (a slow or low voice
   moves less toward that end); the same line always sounds the same, the session keeps each
   speaker's profile as now. (Measured 2026-10-25 on the owner's pack: centred on the derived
   base and clamped, one drunk voice's lines sat at the 0.90 floor 41% of the time and the elder
   mad voice's at the −2 floor 29%; moving the base inward instead lifted that elder above the
   adults and gave both madmen one pitch, so the base stays and each side scales; a base on an
   end, common for an elder drunk or a child mad voice, reflects.)
6. **Deploys.** 19: the tools, the page's `/2` reader and line variation, the new speaker file,
   `voices-<date>.u6vp` (`/2`, every actor) and a pilot pack with about 16 scripts re-translated,
   one or more for each station and each manner present (`translate.mjs --item` takes a list,
   `a:5,b:30`, for it). 20: every conversation script
   re-translated (`--first 200 --speakers <v2> --names <merged>`), merged into a copy of the pilot
   pack, under a new pack name.

## Acceptance

1. Tests, synthetic only: the `/2` reader (enums, refusals, `/1` still read); `registerOf` per
   station × age, `address` in the body, a `/1` or no-speakers request byte for byte as before;
   `speakers.mjs`'s gate, defaults and counts; the voice derivation (families, offsets, ranges,
   spread, `vary` by manner); the page's `/2` reader and a `/1` pack still read; the player's
   per-line variation (deterministic per line, two lines of a `mad` actor differ, a `plain`
   actor's lines do not).
2. The profile run's counts by station and manner, and the dispatcher's private fixtures: the jester
   script `jester`, the mad mage `mad`, the beggar and the pirate `lowborn`, the pompous mayor
   `lord` and `pompous`, the old farmer `commoner` and `elder`.
3. The pilot and the full pack, measured by the dispatcher's private ending counter: by station,
   the dominant level of the adult scripts differs (lord 하오/하게, commoner 합쇼, lowborn 반말/하게),
   and at most 20% of each station's adult scripts take another station's level. The jester's and
   the madmen's lines are read privately for play and lurching. No NPC name of the merged table in
   two Korean forms; capitalised Latin words left only where the English keeps a word of the
   gargoyles' tongue.
4. The voice pack: counts of distinct mixes, the mean pitch and speed by station and manner, and
   every `jester`, `mad` and `drunk` actor with `vary` above 0.
5. Live, on the deployed site (a fresh visitor, the zip only, 한글): the new Korean and voice packs
   are the ones loaded (their file names), the page reads the `/2` pack with no error, and the
   player's queued lines for a `mad` actor carry two different speeds.
