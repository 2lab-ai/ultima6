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

Fixed scope:
- Input = the user's zip (Amiga release). Reverse engineering runs locally on it.
- Item 2 premise is false (EA title still sold on GOG); the user supplied the file
  themselves. The public site never serves game data — see 01-spec "Rights boundary".
- Item 3 = REA (extended to m68k) analysis of `game` → Amiga data support in the
  ScummVM `ultima6` engine → WebAssembly build.
- Item 4 = engine + shell at https://ultima6.2lab.ai, user drops their own files.
- First acceptance line: the game boots into Britannia; intro skipped.
