---
title: ultima6 objects-menu-fog-motion — SSOT
type: ssot
tags: [ultima6]
---

# SSOT — objects-menu-fog-motion

User command (2026-10-27, /goal, verbatim):

> ultima6 개선
>
> 1. object중에 포팅안된것 마저 모두 포팅. (예 그전에 도개교 사용이나 드래곤이 제대로 작동안하고 있던것 처럼 니가 빼먹은 번역있으면 모두 확인하고 추가
> 2. object에 우클릭시에 문맥 메뉴가 나오게 해줘.
> ㄴ 예: 문에 우크릭시에 Use를 선택할수 있고 해당 object에서 할수 있는 행동은 모두 보여주고 클릭할 수 있도록 개선
> 3. 보이다가 시야에서 안보이면 지금 안보이는 부분이 그냥 사라지는데, 회색 표시로 전환해줘. (움직이는건 안보이고 안보이게 됐을때의 모습으로 b/w 어둡게 표시하도록 개선. (화면 밖으로 나가면 당연히 렌더링에서 제거.
> 4. 턴에 대해서 리서치하고 /using-dotprd로 자세히 기술해줘. 이거 지금 되게 옛날 게임 같은 턴 처리가 되고 있음.
> 5. 유저가 이동하거나 턴을 넘길때 아바타와 동료 그리고 npc들의 움직임이 점프를 하는데, 울티마7처럼 보통 턴제 게임 처럼 아바타나 npc가 위치와 위치를 자연스럽게 걷거나 뛰는것 처럼 여러 프레임에 걸쳐서 이동하도록 개선. 이건 리서치를 충분히해주고 너무 느리거나 너무 빠르지 않게 적용해줘.

Specs: item 1 = `16-object-completion.md`, item 2 = `17-context-menu.md`, item 3 = `18-remembered-sight.md`,
item 4 = `19-turn-model.md`, item 5 = `20-smooth-motion.md`.

Decisions taken here (reversible; the owner vetoes on review):

- **Item 1 covers three kinds of gap.** "포팅안된것" and "빼먹은 번역" are read as code not yet carried over and lines
  not yet translated, both:
  - (a) original routines that game code still reaches through a `todo_stub!`. This includes a stub whose callee
    *is* ported elsewhere but whose caller was never wired (a stale seam: `port-map.md` hides these, because a
    ported address outranks its stub, `tools/rea/port_map.py:40,112`);
  - (b) routines that are ported but act unlike the original, the drawbridge and dragon class;
  - (c) lines that object code prints and the Korean pack does not cover.

  Debug-only paths (the debug teleport, 0x104140) are out of scope and named as such. The ending (0x133d1e /
  0x13513a) is in scope: it is the last object handler, the vortex cube's.
- **"배포" is production** (`https://ultima6.2lab.ai`), once per item as soon as its receipt is green, as in
  `hd-layout-font-register/ssot.md`. A pack build goes on top of the live pack (ko-1026a at the time of writing),
  after a message to the session that holds `hd-layout-font-register` row 25.
- **Item 2: a right press on the map opens the menu.** In the original the right button repeats the underlined
  command on the cell (0xa9), else walks like the left (`crates/u6-core/src/ui/click.rs:116-148`). The menu takes
  the right press on the map and on the panels' objects. The underlined command, when there is one, is the menu's
  first entry, so the original's repeat stays one click away. Left-button walking is unchanged.
- **Item 3: memory is render state only.** It is keyed by world cell, never saved (the savegame stays the
  original's format), and dropped once the cell leaves the view. A cell that was seen and is no longer seen, for
  any reason (sight line or darkness), shows its last seen ground and still objects in dark greyscale. Actors are
  not shown there.
- **Item 4's deliverable is the spec.** "리서치하고 … 자세히 기술해줘" asks for research and a written
  description, so `19-turn-model.md` ends with options and a recommendation. Item 5 is the part that changes the
  game's feel now.
- **Item 5 is presentation only.** The engine stays tile-based and on the original's timing. A slide between two
  cells lasts a share of the step's own interval, scales with the speed setting and collapses at Max.
