---
title: ultima6 browser-port — SSOT
type: ssot
tags: [ultima6]
---

# SSOT — browser-port

User's original command (2026-10-14, /goal, verbatim):

> 1. https://github.com/morluto/rea 빌드하고 설치해줘
> 2. ultima6 이제 라이센스 없으니 바이너리 받아줘
> 3. ultima6 REA해주고 웹브라우저에서 실행되도록 재구성해줘
> 4. ultima6.2lab.ai에 배포해줘

Follow-ups (verbatim): "https://archive.org/details/ultima-vi" ·
"cachy-z13:/home/zhugehyuk/다운로드/ultima-vi.zip 이걸로 리버스엔지니어링해서 진행 ㄱ"

Follow-ups 2026-10-14 (verbatim, supersede the engine choice below):

> 아 그리고 리버스엔지니어링해서 새로 코드 작성은 rust로 해줘
>
> 1. rust로 원본 코드 의도대로 포팅
> 2. 그대로 배포
> 3. rust 코드를 고치기 좋도록 리팩토링
> 3.a npc의 가능한 행동을 ai model에게 주고 응답 받을수 있도록
> 3.b 캐릭터 월드맵 등 전체적인 시스템 쉽게 수정할 수 있도록
> 3.c 스팀 게임처럼 mod를 쉽게 만들수 있도록
> 4. 다시 배포

> 그러니까 일단 메인 소스는 rust로 만들고 웹에 올리는건 니가 알아서 해줘

Fixed scope (rev 2, 2026-10-14):
- Main source = Rust, written from the reverse engineering of the Amiga `game` (68k)
  so behavior follows the original code's intent. Web delivery = Rust → wasm, shell
  and hosting chosen by the agent.
- Phases: P1 faithful Rust port → P2 deploy → P3 refactor (3a NPC actions offered to
  an AI model and its reply applied, 3b characters/world map/systems editable as data,
  3c Steam-style mods) → P4 redeploy.
- ScummVM/Nuvie is no longer the runtime; its source stays a cross-reference only.

Fixed scope (rev 1, superseded where it conflicts with rev 2):
- Input = the user's zip (Amiga release). Reverse engineering runs locally on it.
- Item 2 premise is false (EA title still sold on GOG); the user supplied the file
  themselves. The public site never serves game data — see 01-spec "Rights boundary".
- Item 3 = REA (extended to m68k) analysis of `game` → Amiga data support in the
  ScummVM `ultima6` engine → WebAssembly build.
- Item 4 = engine + shell at https://ultima6.2lab.ai, user drops their own files.
- First acceptance line: the game boots into Britannia; intro skipped.
