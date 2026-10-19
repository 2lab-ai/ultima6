# Korean register guide (사극체) for Ultima VI's conversations

This guide is spec 08 §7, written for the translator. `tools/i18n/translate.mjs` sends it, as it
is, with every request. Every example below is made up for this guide; none is the game's text.

## The register

- The English is a faux-archaic English: *thou*, *thee*, *art*, *hast*. Those words choose a
  speech level and a form of address; they have no word-for-word Korean. Carry the same feel
  with the archaic Korean speech levels of historical drama (사극체).
- Address the hero as **그대** unless the line says otherwise (a title the game fills in, such as
  `$G`, stays as it is).
- Never use modern endings: no ~요 (해요체), no ~해 / ~야 / ~거든 / ~잖아 (modern 반말), no
  modern loanwords or slang.
- One speaker keeps one level through the whole conversation.

## Speaker classes and their endings

The request names the speaker's class (guessed from the description, or given by the owner).

| Class | Speech level | Made-up example |
|---|---|---|
| royalty | 해라체 (~노라, ~느니라, ~하라) | 짐은 그대를 오래 기다렸노라. |
| noble | 하오체 (~하오, ~이오, ~시오) | 그대가 찾는 책은 저 탑에 있소. |
| mage | 하오체 | 그 주문은 함부로 외지 마시오. |
| scholar | 하오체, Sino-Korean diction (in Hangul) | 그 이치는 참으로 심오하오. |
| clergy | 하오체, Sino-Korean diction (in Hangul) | 덕을 닦는 길은 멀고도 험하오. |
| guard | a clipped 하오체 | 멈추시오. 용건을 말하시오. |
| merchant | an archaic 합쇼체 (~하옵니다, ~사옵니다, ~하십시오) | 좋은 물건이 많사옵니다. |
| commoner | a low, plain 하오체 | 날이 벌써 저물었소. |
| thief | 하게체 or 해라체 (~하게, ~이네, ~는가) | 자네, 여기서 무얼 하는가? |
| spirit | 해라체 with archaic forms (~로다, ~도다, ~니라) | 나는 이곳을 오래 지켰도다. |
| gargoyle | provisional, until the owner approves it from samples: a terse 하오체 with archaic endings (~이로다, ~하도다) and no honorifics | 너희 땅은 우리 것이로다. |

Incantation syllables, runic text and the credits are not translated.

## What each line is, and what to keep

- A request holds the lines of one conversation script, in the order the script holds them.
  The script splits sentences where the game prints something itself (a name, a number, a
  choice): a line marked `"joins": true` goes on from the line before it without a break.
  Translate every line so that the lines, read in order, make the Korean sentence. Give exactly
  one Korean line for each line, with its `n`; never merge, split, drop or add a line.
- The escapes the game reads must stay, each as many times as in the English line:
  - `$x` (a word the game fills in: a name, a title, a time of day) and `#x` (a number) — the
    letter or digit after `$` or `#` is part of it;
  - `^x`, `*` (the game waits for a key there), `&` (hidden text starts or ends), `%`, the
    back-quote `` ` `` (the next letter is a capital);
  - `<` and `>` (the text between them is drawn in runes): keep everything between them exactly
    as it is in the English;
  - `/` and `\` with the letters after them (English plural letters): keep them where the
    sentence has them; in Korean they print nothing.
- `@` marks a keyword the player can type. The player types English keywords, so write the
  Korean word, then the English keyword highlighted in parentheses: `일(@{job})`. Keep one `@`
  for each `@` of the English line; every `@` is followed at once by `{`, and its `}` closes it
  in the same line.
- Particles (조사): after a Korean word write the particle itself (책을, 왕이). After `$x` or `#x`,
  whose sound you cannot know, write one of these markers and nothing else in braces: `{이/가}`,
  `{은/는}`, `{을/를}`, `{과/와}`, `{(으)로}`.
- Keep the line breaks at the start and at the end of a line exactly as they are; inside a line,
  break only where the English breaks.
- Use only Hangul syllables and ASCII: no hanja, no lone jamo, no other symbols. Punctuation is
  ASCII (`. , ! ? " ' - :`).
- Transliterate names by 외래어 표기법, the same way every time in the script.

## Made-up examples

| Class | English | Korean |
|---|---|---|
| merchant | `"Hast thou come to buy @bread, $G?"` | `"빵(@{bread})을 사러 오셨사옵니까, $G?"` |
| guard | `Thou seekest $N?` | `$N{을/를} 찾으시오?` |
| royalty | `I have #1 coin\s for thee.*` | `그대에게 줄 금화가 #1닢\s 있노라.*` |
| thief | `Keep thy voice down.` | `목소리를 낮추게.` |

## Table strings and the game's own messages (slice K2)

- A line of a conversation marked `"table": true` is a string the script picks at run time (a
  word, a name of a thing, a short phrase) and prints inside another line. Translate it as that
  word or phrase, with no ending of its own unless the English has one. One also marked
  `"copied": true` is stored and printed later as it is: it may hold no particle marker and no
  `@{…}`, and its UTF-8 must fit the `"capacity"` bytes it names (a syllable is 3 bytes).
- A request of the game's own messages holds, instead of a conversation, the strings the game
  prints itself, each `{"n", "en", "kind"}`: `message` (a line as it is), `format` (a line with
  `%` directives the game fills in: `%s` a word or a name, `%d` a number; keep every `%…`
  directive exactly, in the same order, and every other escape as above) or `label` (a word the
  game puts inside another line: a command's name before its dash, a form of address, a time of
  day). The game speaks to the player as a narrator: use the 하오체 and address the player as
  **그대**. Keep each string short (the message window is 17 cells wide); a command's name is one
  word.

| Kind | English | Korean |
|---|---|---|
| label | `Seek` | `찾기` |
| format | `\n%s is too heavy.\n` | `\n%s{은/는} 너무 무겁소.\n` |
| message | `Thou canst not.` | `그리할 수 없소.` |

## Object names, books and signs (slice K3)

- A request of `"kind": "look"` texts holds the names the game gives things when the player
  looks at them (a sword, a bed, a wall), and the game puts each one inside its own lines
  ("그대는 … 보오"). Write a **plain noun phrase**: no honorific, no ending, no article (Korean
  has none), no particle marker (`{이/가}` and the rest belong to the line around it) and no
  `@{…}`.
- English names carry plural markers: `/` with the letters for one thing, `\` with the letters
  for many (`kni/fe\ves`). Korean shows one form for any count, and in Korean the game prints
  neither marker nor its letters, so drop them: write the name once, with no `/` and no `\`.
- A request of `"kind": "book"` texts holds what a book or a sign says when it is read. Write it
  in 사극체 as a narrator or as its writer would (하오체 by default; a royal decree in 해라체),
  and keep everything the conversation lines keep: every escape and `%` directive in the same
  order, the text between `<` and `>` (runes) exactly as it is, the line breaks at its start and
  its end. A book is at most 10239 bytes of UTF-8 (a syllable is 3 bytes).
- Every name and every book needs at least one Hangul syllable; a text whose only letters are
  runes is not sent.

| Kind | English | Korean |
|---|---|---|
| look | `copper teapot` | `구리 찻주전자` |
| look | `basket\s of plums` | `자두 바구니` |
| book | `Beware the dark forest, traveller.\n` | `나그네여, 어두운 숲을 조심하시오.\n` |

## The opening, the story, the gypsy and character creation (slice K4)

- A request of these lines holds, in the order the game shows them, the text of the separate
  program that plays the opening and makes the character, each `{"n", "en", "kind"}`:
  - `opening` and `story`: the story passages, told by a narrator. Write them in 사극체 as a
    storyteller of old would (하오체 or a narrating 해라체, ~하였노라, ~이었도다), the same level
    through every passage;
  - `gypsy`: the walk to the gypsy's wagon, her questions and her farewell. The gypsy is a seer:
    she speaks in 하오체 or 하게체 (~하게, ~이네, ~는가), solemn and a little strange, and keeps
    that level in every question; the narration around her is the narrator's;
  - `creation` and `transfer`: the prompts of making a character and of bringing one from an
    earlier game (a name, a sex, a portrait, a disk, the attributes). Write them plain and
    short, in 하오체 (`그대의 이름은 무엇이오?`), each label one word.
- This program prints with its own printer, which has no escape layer: `$ # * @ & < >` are
  plain characters there, and there are no particle markers. Keep every `%` code exactly, in
  the same order: `%s` (a name or a word the game fills in), `%d` (a number), and the codes the
  game reads itself, `%B` and `%F` (colours), `%X` and `%Y` (where the text goes), `%P` (the
  passage is laid out there). Never write `{이/가}` or another marker, nor `@{…}`: they would
  print as they are.
- The name the player types is in Latin letters, so its sound is unknown: after `%s` write no
  particle (`%s, 그대여`), or the paired form itself (`%s이(가)`).
- Keep every line break where the English has it: as many at the start, at the end and in all
  (a sheet's labels stand on their own rows, beside the values the game prints). A passage that
  does not fit its box is shown in English, so keep each one no longer on screen than its
  English (a syllable is about as wide as two English letters).
- A line with `"keys"` is shown before the game waits for one of those keys. Keep each key its
  English shows standing alone (`A)`, `(M)`, `N)ew`) standing alone, in Latin letters and in the
  same order; for a key the English shows only inside a word (`Male` for `M`), put the letter
  after that word's Korean as `(M)` (`남자(M)`).
- Every line needs at least one Hangul syllable.

| Kind | English | Korean |
|---|---|---|
| story | `%F%BThe wind howled over the moor.\n%P` | `%F%B황야 위로 바람이 울부짖었노라.\n%P` |
| gypsy | `Wouldst thou A) speak the truth, or B) spare a friend?` | `그대는 A) 참을 말하겠는가, B) 벗을 감싸겠는가?` |
| creation | `What name dost thou bear, traveller?` | `나그네여, 그대의 이름은 무엇이오?` |
| transfer | `Strength\nDexterity\n` | `힘\n민첩\n` |
