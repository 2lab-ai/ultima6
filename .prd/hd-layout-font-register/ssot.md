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
