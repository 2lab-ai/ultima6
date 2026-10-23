---
title: ultima6 hd-layout-font-register — SSOT
type: ssot
tags: [ultima6]
---

# SSOT — hd-layout-font-register

User command (2026-10-22, /goal, verbatim):

> ultima6 포팅한거 개선해줘
>
> 1. hd 해상도에 맞게 화면 ui 재구성해주고 hd pack을 최대한 사용핤 윘도록 개선
> 2. hd 해상도에 맞게 한글 폰트 개선해줘. 처음 설정에 니가 만든 기본 폰트랑 hd 화면에 맞는 적절한 폰트 사용해줘.
> 3. npc들의 프로파일에 맞게 영어 다시 번역해줘. 특히 어린얘들이 어른처럼 말하는거 수정해줘.
>
> 이 각각에 대해서 개선하고 배포하고 개선하고 배포하고 빠른 배포위주로 테스트해볼수 있게 진행

Restated by the owner a few minutes later, item 1 refined (verbatim):

> 1. hd 해상도에 맞게 화면 ui 재구성해주고 hd pack을 최대한 사용핤 윘도록 개선 (전체 화면 모드 지원; 최대 해상도는 hd고 그거보다 화면이 크면 알아사 스트레칭)

Specs: item 1 = `10-hd-layout.md`, item 2 = `11-hd-font.md`, item 3 = `12-register-by-profile.md`.

Decisions taken here (reversible; the owner vetoes on review):

- **"HD" is 1280 × 720.** The HD pack stores ×4 images; the map window is 160 px, so at ×4 it is 640 px and fits 720 with the pack's pixels shown one to one ("hd pack을 최대한 사용"). At 1920 × 1080 the map would be ×6 and the pack upscaled. Larger screens stretch the 1280 × 720 picture, keeping 16:9 ("그거보다 화면이 크면 알아서 스트레칭").
- **"배포" is production** (`scripts/deploy-web.sh main` → https://ultima6.2lab.ai), once per item as soon as that item's receipt is green, then again for each follow-up ("개선하고 배포하고"). Cloudflare Pages keeps every earlier deployment for a rollback.
- **Item 3's upload authority** is the owner's own line above, "npc들의 프로파일에 맞게 영어 다시 번역해줘": the conversation scripts are sent again to the model, as on 2026-10-20 (`hd-i18n-voice/loop.done.md`, "The translation run"). Nothing it makes enters the repository.

Fixed constraints inherited (not reopened): `.prd/01-spec.md` §Rights (no game bytes, text, names, art or values in the repo, tests or logs); the packs stay out of git and are served under `/packs/` (07, 08, 09).

## Round 2 — the wide view (2026-10-23)

User, after deploy 9 (verbatim):

> ui 개 좇같이 해놨네 시발년아 지금 11x11 표시하고 있나 그걸 늘려달라고 개같은새끼야 hd해상도로 변경하면서 씨발 비율을 그대로 좇같이하고 ui도 개 좇같이 다 해놨네 시발새끼야

> 시물레이션을 60x60으로 늘리고 뷰포트를 2배로 늘려줘 21x21 정도로

Read: item 1's "화면 ui 재구성" asked for the map view itself to grow (spec 05 §4, the wider view, was the open item); deploys 2, 6 and 8 kept the original 11 × 11 view. Round 2 grows the simulation window from 40 × 40 to 60 × 60 cells and the map view from 11 × 11 to 21 × 21 cells (spec 13).

Decision (reversible): the HD canvas becomes 1920 × 1080, so a 21-cell map column shows the pack's 64 px tiles at about 51 px; at 1280 × 720 it would be 34 px.

> 원본 모드랑 신규 모드 둘다 지원하도록해줘

So the view and window sizes are a mode: Original keeps 11 × 11 / 40 × 40 / the 320 × 200 frame (every parity and oracle test runs there), Wide is 21 × 21 / 60 × 60.

> https://github.com/nuvie/nuvie/blob/master/docs/images/nuvie.png 여기처럼 구현해줘 울티마7 처럼 ui 변경 저기 확인해서

The reference (kept outside the repo, `~/Games/ultima6/work/dispatch/r1023-wide/nuvie.png`; its right half is Nuvie's new style): the map fills the screen; over it float Ultima 7-style windows, each framed with a title, a close tick at its left edge: a paper doll window (equipment slots, "Wielded", the combat mode button), a portrait and attributes window, a backpack window (item grid, weight line), and the sun and moons bar at the top right. Round 2 builds that look in Wide mode in two deploys: first the full-screen map with the engine's own panels in floating windows, then separate doll, attributes and backpack windows open at once.
