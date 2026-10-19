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
