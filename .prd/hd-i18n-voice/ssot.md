---
title: ultima6 hd-i18n-voice — SSOT
type: ssot
tags: [ultima6]
---

# SSOT — hd-i18n-voice

User command (2026-10-15, /goal, verbatim):

> ultima6.2lab.ai 개선
>
> 1. hd 이미지 업스케일을 이미지젠 스킬로 해주고 hd 업스케일해줘
> 2. 처음 시작시에 영어, 한글 고를수 있게 해주고 i18n 구현해줘. 고대 영어틱하게 번역해줘
> 3. 각각의 npc에 대해서 이름과 대사를 따로 묶어줘 나중에 NpcBrain 구현할시에 이 데이터로 말투 따라하도록 해야함. 그리고 각각의 npc에 대해서 성격으로 온디바이스 tts구현할수 있게 목소리 프로파일 만들어줘. supertonic-3 이용해서 실시간 tts 붙여주고 npc가 대사할때 tts 플레이해줘.
>
> 각각에 대해서 리서치하고 usingprd 부터 명확히 하고 작업 순차적으로 진행

Added the same day (verbatim):

> 4. 게임 루프(틱)을 고정해서 속도 유지해줘. 이거 키보드 -+로 조절할 수 있도록해줘. (지금 너무 느린데 최적화도 해야할 것 같음)

Added 2026-10-21 (verbatim):

> 2. ultima6-deploy 비공개 저장소 만들고 네가 가공한 2차 리소스들 올리고 배포할때 쓰도록해줘

Order: research each item, then fix the specs (using-dotprd), then build the items one after another.

Fixed constraints these items inherit (not reopened here):

- `.prd/01-spec.md` §Rights. The public site serves only our engine. Game bytes, text, names, art and values never go into the repo, tests, logs or the hosted site.
- `.prd/05-hd-output.md` holds the HD/4K seam design (scene output, logical input, assets by id, view size as a parameter).
